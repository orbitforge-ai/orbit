use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, Semaphore};
use tracing::{error, info, warn};

use crate::db::repos::{sqlite::SqliteRepos, Repos};
use crate::db::DbPool;
use crate::events::emitter::{emit_bus_message_sent_to_host, emit_run_state_changed_to_host};
use crate::executor::memory::MemoryClient;
use crate::executor::permissions::PermissionRegistry;
use crate::executor::state_machine::{transition, ExecutorEvent};
use crate::executor::{agent_loop, http, process};
use crate::models::run::RunState;
use crate::models::task::{
    AgentLoopConfig, AgentStepConfig, HttpRequestConfig, ScriptFileConfig, ShellCommandConfig, Task,
};
use crate::runtime_host::{RuntimeHost, RuntimeHostHandle};
use serde_json;

const DEFAULT_AGENT_ID: &str = "default";
const DEFAULT_MAX_CONCURRENT: usize = 10;
/// Retry delay capped at 1 hour
const MAX_RETRY_DELAY_SECS: u64 = 3600;

/// Request sent to the executor engine to start a run.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub run_id: String,
    pub task: Task,
    pub schedule_id: Option<String>,
    pub _trigger: String,
    /// Number of retries already attempted (0 for initial run)
    pub retry_count: i64,
    /// Parent run id if this is a retry
    pub _parent_run_id: Option<String>,
    /// Depth of agent-to-agent chain (0 for top-level runs)
    pub chain_depth: i64,
}

/// Newtype wrapping the sender half — stored as Tauri managed state.
#[derive(Clone)]
pub struct ExecutorTx(pub mpsc::UnboundedSender<RunRequest>);

/// Shared state for tracking active runs and cancellation tokens.
#[derive(Clone)]
pub struct ActiveRunRegistry {
    /// agent_id → set of run_ids currently executing
    pub active_runs: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    /// run_id → cancel sender
    pub cancel_senders: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
}

impl ActiveRunRegistry {
    pub fn new() -> Self {
        Self {
            active_runs: Arc::new(Mutex::new(HashMap::new())),
            cancel_senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register(&self, agent_id: &str, run_id: &str, cancel_tx: oneshot::Sender<()>) {
        let mut active = self.active_runs.lock().await;
        active
            .entry(agent_id.to_string())
            .or_default()
            .insert(run_id.to_string());
        drop(active);

        let mut senders = self.cancel_senders.lock().await;
        senders.insert(run_id.to_string(), cancel_tx);
    }

    pub async fn unregister(&self, agent_id: &str, run_id: &str) {
        let mut active = self.active_runs.lock().await;
        if let Some(set) = active.get_mut(agent_id) {
            set.remove(run_id);
        }
        let mut senders = self.cancel_senders.lock().await;
        senders.remove(run_id);
    }

    /// Cancel all active runs for a given agent. Returns the run IDs that were cancelled.
    pub async fn cancel_agent_runs(&self, agent_id: &str, repos: &dyn Repos) -> Vec<String> {
        let active = self.active_runs.lock().await;
        let run_ids: Vec<String> = active
            .get(agent_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        drop(active);

        let mut senders = self.cancel_senders.lock().await;
        let mut cancelled = Vec::new();
        for run_id in &run_ids {
            if let Some(tx) = senders.remove(run_id) {
                let _ = tx.send(());
                cancelled.push(run_id.clone());
            }
        }
        drop(senders);

        // Mark as cancelled in DB immediately.
        for run_id in &cancelled {
            let _ = mark_run_cancelled(repos, run_id).await;
        }

        cancelled
    }

    pub async fn _active_count(&self, agent_id: &str) -> usize {
        let active = self.active_runs.lock().await;
        active.get(agent_id).map(|s| s.len()).unwrap_or(0)
    }
}

/// Shared per-agent semaphore pool used by both executor-backed runs and
/// session-backed agent executions.
#[derive(Clone)]
pub struct AgentSemaphores {
    inner: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl AgentSemaphores {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn init(&self, db: &DbPool) {
        let pool = db.clone();
        let result: Vec<(String, usize)> =
            tokio::task::spawn_blocking(move || -> Option<Vec<(String, usize)>> {
                let conn = pool.get().ok()?;
                let mut stmt = conn
                    .prepare("SELECT id, max_concurrent_runs FROM agents WHERE tenant_id = 'local'")
                    .ok()?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })
                    .ok()?
                    .filter_map(|r| r.ok())
                    .map(|(id, n)| (id, n.max(1) as usize))
                    .collect();
                Some(rows)
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

        let mut semaphores = self.inner.lock().await;
        for (agent_id, capacity) in result {
            semaphores
                .entry(agent_id)
                .or_insert_with(|| Arc::new(Semaphore::new(capacity)));
        }
        semaphores
            .entry(DEFAULT_AGENT_ID.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT)));
    }

    pub async fn get_or_create(&self, agent_id: &str, db: &DbPool) -> Arc<Semaphore> {
        let mut semaphores = self.inner.lock().await;
        if let Some(s) = semaphores.get(agent_id) {
            return s.clone();
        }

        let id = agent_id.to_string();
        let pool = db.clone();
        let capacity = tokio::task::spawn_blocking(move || {
            let conn = pool.get().ok()?;
            let n: i64 = conn
                .query_row(
                    "SELECT max_concurrent_runs
                       FROM agents
                      WHERE id = ?1
                        AND tenant_id = COALESCE((SELECT tenant_id FROM agents WHERE id = ?1), 'local')",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .ok()?;
            Some(n.max(1) as usize)
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(DEFAULT_MAX_CONCURRENT);

        let sem = Arc::new(Semaphore::new(capacity));
        semaphores.insert(agent_id.to_string(), sem.clone());
        sem
    }
}

/// Tracks session cancellation requests for session-backed agent executions.
#[derive(Clone)]
pub struct SessionExecutionRegistry {
    cancelled_sessions: Arc<Mutex<HashSet<String>>>,
}

impl SessionExecutionRegistry {
    pub fn new() -> Self {
        Self {
            cancelled_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn cancel(&self, session_id: &str) {
        let mut cancelled = self.cancelled_sessions.lock().await;
        cancelled.insert(session_id.to_string());
    }

    pub async fn clear_cancelled(&self, session_id: &str) {
        let mut cancelled = self.cancelled_sessions.lock().await;
        cancelled.remove(session_id);
    }

    pub async fn is_cancelled(&self, session_id: &str) -> bool {
        let cancelled = self.cancelled_sessions.lock().await;
        cancelled.contains(session_id)
    }
}

#[derive(Clone)]
pub struct UserQuestionRegistry {
    pending: Arc<Mutex<HashMap<String, PendingUserQuestion>>>,
}

struct PendingUserQuestion {
    session_id: Option<String>,
    response_tx: oneshot::Sender<String>,
}

impl UserQuestionRegistry {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register(
        &self,
        request_id: &str,
        session_id: Option<&str>,
    ) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().await;
        pending.insert(
            request_id.to_string(),
            PendingUserQuestion {
                session_id: session_id.map(|value| value.to_string()),
                response_tx: tx,
            },
        );
        rx
    }

    pub async fn resolve(&self, request_id: &str, response: String) -> Result<(), String> {
        let mut pending = self.pending.lock().await;
        match pending.remove(request_id) {
            Some(entry) => {
                let _ = entry.response_tx.send(response);
                Ok(())
            }
            None => Err(format!("No pending user question with id '{}'", request_id)),
        }
    }

    pub async fn cancel(&self, request_id: &str) {
        let mut pending = self.pending.lock().await;
        pending.remove(request_id);
    }

    pub async fn cancel_for_session(&self, session_id: &str) {
        let mut pending = self.pending.lock().await;
        let ids: Vec<String> = pending
            .iter()
            .filter(|(_, value)| value.session_id.as_deref() == Some(session_id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            pending.remove(&id);
        }
    }
}

/// The background execution engine.
pub struct ExecutorEngine {
    db: DbPool,
    repos: Arc<dyn Repos>,
    rx: mpsc::UnboundedReceiver<RunRequest>,
    /// Clone of the sender so the engine can enqueue retry runs.
    tx: mpsc::UnboundedSender<RunRequest>,
    host: RuntimeHostHandle,
    agent_semaphores: AgentSemaphores,
    session_registry: SessionExecutionRegistry,
    permission_registry: PermissionRegistry,
    /// Shared active run registry for concurrency policy enforcement
    registry: ActiveRunRegistry,
    log_dir: PathBuf,
    /// Optional memory client for long-term memory integration.
    memory_client: Option<MemoryClient>,
    /// Optional cloud client for syncing data to Supabase.
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
}

impl ExecutorEngine {
    pub fn new(
        db: DbPool,
        rx: mpsc::UnboundedReceiver<RunRequest>,
        tx: mpsc::UnboundedSender<RunRequest>,
        host: RuntimeHostHandle,
        agent_semaphores: AgentSemaphores,
        session_registry: SessionExecutionRegistry,
        permission_registry: PermissionRegistry,
        log_dir: PathBuf,
        memory_client: Option<MemoryClient>,
        cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
    ) -> Self {
        let repos: Arc<dyn Repos> = Arc::new(SqliteRepos::new(db.clone()));
        Self::new_with_repos(
            db,
            repos,
            rx,
            tx,
            host,
            agent_semaphores,
            session_registry,
            permission_registry,
            log_dir,
            memory_client,
            cloud_client,
        )
    }

    pub fn new_with_repos(
        db: DbPool,
        repos: Arc<dyn Repos>,
        rx: mpsc::UnboundedReceiver<RunRequest>,
        tx: mpsc::UnboundedSender<RunRequest>,
        host: RuntimeHostHandle,
        agent_semaphores: AgentSemaphores,
        session_registry: SessionExecutionRegistry,
        permission_registry: PermissionRegistry,
        log_dir: PathBuf,
        memory_client: Option<MemoryClient>,
        cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
    ) -> Self {
        Self {
            db,
            repos,
            rx,
            tx,
            host,
            agent_semaphores,
            session_registry,
            permission_registry,
            registry: ActiveRunRegistry::new(),
            log_dir,
            memory_client,
            cloud_client,
        }
    }

    pub async fn run(mut self) {
        info!("ExecutorEngine started");
        self.agent_semaphores.init(&self.db).await;

        while let Some(req) = self.rx.recv().await {
            let agent_id = req
                .task
                .agent_id
                .clone()
                .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string());
            let policy = req.task.concurrency_policy.clone();

            let semaphore = self
                .agent_semaphores
                .get_or_create(&agent_id, &self.db)
                .await;
            let db = self.db.clone();
            let repos = self.repos.clone();
            let host = self.host.clone();
            let log_dir = self.log_dir.clone();
            let registry = self.registry.clone();
            let agent_semaphores = self.agent_semaphores.clone();
            let session_registry = self.session_registry.clone();
            let permission_registry = self.permission_registry.clone();
            let tx = self.tx.clone();
            let memory_client = self.memory_client.clone();
            let cloud_client = self.cloud_client.clone();

            match policy.as_str() {
                "skip" => {
                    // If at capacity, cancel this run immediately
                    match semaphore.clone().try_acquire_owned() {
                        Ok(permit) => {
                            tokio::spawn(async move {
                                let (cancel_tx, cancel_rx) = oneshot::channel();
                                registry.register(&agent_id, &req.run_id, cancel_tx).await;

                                if let Err(e) = run_one(
                                    req.clone(),
                                    db.clone(),
                                    repos.clone(),
                                    host.clone(),
                                    log_dir.clone(),
                                    cancel_rx,
                                    tx.clone(),
                                    agent_semaphores.clone(),
                                    session_registry.clone(),
                                    permission_registry.clone(),
                                    memory_client.clone(),
                                    cloud_client.clone(),
                                )
                                .await
                                {
                                    error!("run failed: {}", e);
                                }

                                registry.unregister(&agent_id, &req.run_id).await;
                                update_agent_heartbeat(&db, &agent_id);
                                drop(permit);

                                // Evaluate bus subscriptions
                                evaluate_bus_subscriptions(
                                    repos.as_ref(),
                                    &req.run_id,
                                    &agent_id,
                                    req.chain_depth,
                                    &tx,
                                    host.as_ref(),
                                )
                                .await;
                                // Schedule retry if needed
                                schedule_retry_if_needed(req, repos.as_ref(), &tx, host.clone())
                                    .await;
                            });
                        }
                        Err(_) => {
                            warn!(run_id = req.run_id, "skipping run — agent at capacity");
                            let _ = mark_run_skipped(repos.as_ref(), &req.run_id).await;
                            emit_run_state_changed_to_host(
                                host.as_ref(),
                                &req.run_id,
                                RunState::Pending.as_str(),
                                RunState::Cancelled.as_str(),
                            );
                        }
                    }
                }
                "cancel_previous" => {
                    // Cancel currently active runs for this agent
                    let cancelled = registry.cancel_agent_runs(&agent_id, repos.as_ref()).await;
                    for run_id in &cancelled {
                        emit_run_state_changed_to_host(
                            host.as_ref(),
                            run_id,
                            RunState::Running.as_str(),
                            RunState::Cancelled.as_str(),
                        );
                    }

                    let sem = semaphore.clone();
                    tokio::spawn(async move {
                        // Brief delay for cancelled runs to clean up their semaphore permits
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        let permit = sem.acquire_owned().await.expect("semaphore closed");

                        let (cancel_tx, cancel_rx) = oneshot::channel();
                        registry.register(&agent_id, &req.run_id, cancel_tx).await;

                        if let Err(e) = run_one(
                            req.clone(),
                            db.clone(),
                            repos.clone(),
                            host.clone(),
                            log_dir.clone(),
                            cancel_rx,
                            tx.clone(),
                            agent_semaphores.clone(),
                            session_registry.clone(),
                            permission_registry.clone(),
                            memory_client.clone(),
                            cloud_client.clone(),
                        )
                        .await
                        {
                            error!("run failed: {}", e);
                        }

                        registry.unregister(&agent_id, &req.run_id).await;
                        update_agent_heartbeat(&db, &agent_id);
                        drop(permit);

                        evaluate_bus_subscriptions(
                            repos.as_ref(),
                            &req.run_id,
                            &agent_id,
                            req.chain_depth,
                            &tx,
                            host.as_ref(),
                        )
                        .await;
                        schedule_retry_if_needed(req, repos.as_ref(), &tx, host.clone()).await;
                    });
                }
                // "allow" | "queue" — natural semaphore behavior
                _ => {
                    tokio::spawn(async move {
                        let permit = semaphore.acquire_owned().await.expect("semaphore closed");

                        let (cancel_tx, cancel_rx) = oneshot::channel();
                        registry.register(&agent_id, &req.run_id, cancel_tx).await;

                        if let Err(e) = run_one(
                            req.clone(),
                            db.clone(),
                            repos.clone(),
                            host.clone(),
                            log_dir.clone(),
                            cancel_rx,
                            tx.clone(),
                            agent_semaphores.clone(),
                            session_registry.clone(),
                            permission_registry.clone(),
                            memory_client.clone(),
                            cloud_client.clone(),
                        )
                        .await
                        {
                            error!("run failed: {}", e);
                        }

                        registry.unregister(&agent_id, &req.run_id).await;
                        update_agent_heartbeat(&db, &agent_id);
                        drop(permit);

                        evaluate_bus_subscriptions(
                            repos.as_ref(),
                            &req.run_id,
                            &agent_id,
                            req.chain_depth,
                            &tx,
                            host.as_ref(),
                        )
                        .await;
                        schedule_retry_if_needed(req, repos.as_ref(), &tx, host.clone()).await;
                    });
                }
            }
        }

        warn!("ExecutorEngine channel closed — shutting down");
    }
}

async fn run_one(
    req: RunRequest,
    db: DbPool,
    repos: Arc<dyn Repos>,
    host: RuntimeHostHandle,
    log_dir: PathBuf,
    cancel: oneshot::Receiver<()>,
    executor_tx: mpsc::UnboundedSender<RunRequest>,
    agent_semaphores: AgentSemaphores,
    session_registry: SessionExecutionRegistry,
    permission_registry: PermissionRegistry,
    memory_client: Option<MemoryClient>,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<(), String> {
    let run_id = req.run_id.clone();
    let task = req.task;

    update_run_state(
        repos.as_ref(),
        &run_id,
        &RunState::Running,
        None,
        None,
        None,
    )
    .await?;
    emit_run_state_changed_to_host(
        host.as_ref(),
        &run_id,
        RunState::Pending.as_str(),
        RunState::Running.as_str(),
    );

    let log_path = log_dir.join(format!("{}.log", run_id));
    let timeout_secs = task.max_duration_seconds as u64;

    let result = match task.kind.as_str() {
        "shell_command" => {
            let cfg: ShellCommandConfig =
                serde_json::from_value(task.config.clone()).map_err(|e| e.to_string())?;
            process::run_shell(&run_id, &cfg, &log_path, timeout_secs, host.clone(), cancel).await
        }
        "script_file" => {
            let cfg: ScriptFileConfig =
                serde_json::from_value(task.config.clone()).map_err(|e| e.to_string())?;
            process::run_script(&run_id, &cfg, &log_path, timeout_secs, host.clone(), cancel).await
        }
        "http_request" => {
            let cfg: HttpRequestConfig =
                serde_json::from_value(task.config.clone()).map_err(|e| e.to_string())?;
            http::run_http(&run_id, &cfg, &log_path, timeout_secs, host.clone(), cancel)
                .await
                .map(|r| process::ProcessResult {
                    exit_code: r.exit_code,
                    duration_ms: r.duration_ms,
                })
        }
        "agent_step" => {
            let cfg: AgentStepConfig =
                serde_json::from_value(task.config.clone()).map_err(|e| e.to_string())?;
            let agent_id = task
                .agent_id
                .clone()
                .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string());
            let app = host
                .app_handle()
                .ok_or_else(|| "agent_step requires a Tauri runtime host".to_string())?;
            agent_loop::run_agent_prompt(
                &run_id,
                &agent_id,
                &cfg,
                &log_path,
                timeout_secs,
                &app,
                cancel,
                &db,
                &executor_tx,
                req.chain_depth,
                &agent_semaphores,
                &session_registry,
                memory_client.as_ref(),
                "default_user",
                cloud_client.clone(),
            )
            .await
        }
        "agent_loop" => {
            let cfg: AgentLoopConfig =
                serde_json::from_value(task.config.clone()).map_err(|e| e.to_string())?;
            let agent_id = task
                .agent_id
                .clone()
                .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string());

            // Pulse tasks route to run_pulse (chat-session-based)
            let is_pulse = task.tags.iter().any(|t| t == "pulse");
            let is_sub_agent = task.tags.iter().any(|t| t == "sub_agent");
            let app = host
                .app_handle()
                .ok_or_else(|| "agent_loop requires a Tauri runtime host".to_string())?;
            if is_pulse {
                agent_loop::run_pulse(
                    &run_id,
                    &agent_id,
                    task.project_id.as_deref(),
                    &cfg.goal,
                    &log_path,
                    timeout_secs,
                    &app,
                    cancel,
                    &db,
                    &executor_tx,
                    req.chain_depth,
                    &agent_semaphores,
                    &session_registry,
                    &permission_registry,
                    memory_client.as_ref(),
                    "default_user",
                    cloud_client.clone(),
                )
                .await
            } else {
                agent_loop::run_agent_loop(
                    &run_id,
                    &agent_id,
                    &cfg,
                    &log_path,
                    timeout_secs,
                    &app,
                    cancel,
                    &db,
                    &executor_tx,
                    req.chain_depth,
                    is_sub_agent,
                    &agent_semaphores,
                    &session_registry,
                    &permission_registry,
                    memory_client.as_ref(),
                    "default_user",
                    cloud_client.clone(),
                )
                .await
            }
        }
        other => Err(format!("unsupported task kind: {}", other)),
    };

    match result {
        Ok(proc_result) => {
            let is_success = proc_result.exit_code == 0;
            let event = if is_success {
                ExecutorEvent::Succeeded {
                    exit_code: proc_result.exit_code,
                    duration_ms: proc_result.duration_ms,
                }
            } else {
                ExecutorEvent::Failed {
                    exit_code: Some(proc_result.exit_code),
                    reason: format!("exit code {}", proc_result.exit_code),
                }
            };

            let next_state = transition(&RunState::Running, &event).unwrap_or(RunState::Failure);

            update_run_state(
                repos.as_ref(),
                &run_id,
                &next_state,
                Some(proc_result.exit_code as i64),
                Some(proc_result.duration_ms),
                None,
            )
            .await?;

            emit_run_state_changed_to_host(
                host.as_ref(),
                &run_id,
                RunState::Running.as_str(),
                next_state.as_str(),
            );
        }
        Err(reason) if reason == "cancelled" => {
            update_run_state(
                repos.as_ref(),
                &run_id,
                &RunState::Cancelled,
                Some(-1),
                None,
                None,
            )
            .await?;
            emit_run_state_changed_to_host(
                host.as_ref(),
                &run_id,
                RunState::Running.as_str(),
                RunState::Cancelled.as_str(),
            );
        }
        Err(reason) => {
            let next_state = if reason == "timed out" {
                RunState::TimedOut
            } else {
                RunState::Failure
            };

            let metadata = serde_json::json!({ "error": reason });
            update_run_state(
                repos.as_ref(),
                &run_id,
                &next_state,
                Some(-1),
                None,
                Some(metadata),
            )
            .await?;
            emit_run_state_changed_to_host(
                host.as_ref(),
                &run_id,
                RunState::Running.as_str(),
                next_state.as_str(),
            );
        }
    }

    permission_registry
        .cancel_for_run_with_host(&run_id, host.as_ref())
        .await;

    Ok(())
}

/// Schedule a retry run if the task has retries remaining.
async fn schedule_retry_if_needed(
    req: RunRequest,
    repos: &dyn Repos,
    tx: &mpsc::UnboundedSender<RunRequest>,
    host: RuntimeHostHandle,
) {
    // Only retry if last run ended in failure
    let state = repos
        .runs()
        .get(&req.run_id)
        .await
        .ok()
        .flatten()
        .map(|run| run.state);

    if state.as_deref() != Some("failure") {
        return;
    }

    let retries_remaining = req.task.max_retries - req.retry_count;
    if retries_remaining <= 0 {
        return;
    }

    let delay_secs = {
        let base = req.task.retry_delay_seconds as u64;
        let backoff = base * (1u64 << (req.retry_count.min(6) as u32));
        backoff.min(MAX_RETRY_DELAY_SECS)
    };

    info!(
        run_id = req.run_id,
        retry_count = req.retry_count,
        delay_secs = delay_secs,
        "scheduling retry"
    );

    // Create a new run record for the retry
    let retry_run_id = ulid::Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let log_path = format!(
        "{}/.orbit/logs/{}.log",
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        retry_run_id
    );
    let retry_count = req.retry_count + 1;
    if let Err(e) = repos
        .runs()
        .create_retry_run(
            &retry_run_id,
            &req.task,
            req.schedule_id.as_deref(),
            &log_path,
            retry_count,
            &req.run_id,
            &now,
        )
        .await
    {
        error!("failed to create retry run record: {}", e);
        return;
    }

    let retry_req = RunRequest {
        run_id: retry_run_id.clone(),
        task: req.task,
        schedule_id: req.schedule_id,
        _trigger: "retry".to_string(),
        retry_count,
        _parent_run_id: Some(req.run_id),
        chain_depth: req.chain_depth,
    };

    let tx_clone = tx.clone();
    let host_clone = host.clone();

    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
        info!(run_id = retry_run_id, "firing retry run");
        emit_run_state_changed_to_host(host_clone.as_ref(), &retry_run_id, "pending", "pending");
        let _ = tx_clone.send(retry_req);
    });
}

async fn update_run_state(
    repos: &dyn Repos,
    run_id: &str,
    state: &RunState,
    exit_code: Option<i64>,
    duration_ms: Option<i64>,
    metadata: Option<serde_json::Value>,
) -> Result<(), String> {
    repos
        .runs()
        .update_state(run_id, state, exit_code, duration_ms, metadata)
        .await
}

async fn mark_run_skipped(repos: &dyn Repos, run_id: &str) -> Result<(), String> {
    repos
        .runs()
        .update_state(
            run_id,
            &RunState::Cancelled,
            None,
            None,
            Some(serde_json::json!({ "skip_reason": "agent at capacity" })),
        )
        .await
}

async fn mark_run_cancelled(repos: &dyn Repos, run_id: &str) -> Result<(), String> {
    repos.runs().cancel(run_id).await
}

fn update_agent_heartbeat(db: &DbPool, agent_id: &str) {
    if let Ok(conn) = db.get() {
        let now = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
            "UPDATE agents
                SET heartbeat_at = ?1, updated_at = ?1
              WHERE id = ?2
                AND tenant_id = COALESCE((SELECT tenant_id FROM agents WHERE id = ?2), 'local')",
            rusqlite::params![now, agent_id],
        );
    }
}

/// After a run completes, check for bus subscriptions that should trigger.
async fn evaluate_bus_subscriptions(
    repos: &dyn Repos,
    run_id: &str,
    agent_id: &str,
    chain_depth: i64,
    tx: &mpsc::UnboundedSender<RunRequest>,
    host: &dyn RuntimeHost,
) {
    let final_state = match repos.runs().get(run_id).await {
        Ok(Some(run)) => run.state,
        Ok(None) => return,
        Err(e) => {
            error!("bus subscriptions: failed to load run {}: {}", run_id, e);
            return;
        }
    };

    // Only evaluate on terminal states
    if !matches!(
        final_state.as_str(),
        "success" | "failure" | "timed_out" | "cancelled"
    ) {
        return;
    }

    let subscriptions = match repos
        .bus_subscriptions()
        .list_enabled_for_source(agent_id)
        .await
    {
        Ok(subscriptions) => subscriptions,
        Err(e) => {
            error!(
                "bus subscriptions: failed to load subscriptions for {}: {}",
                agent_id, e
            );
            return;
        }
    };

    for sub in subscriptions {
        let matches = match sub.event_type.as_str() {
            "run:completed" => final_state == "success",
            "run:failed" => final_state == "failure",
            "run:any_terminal" => true,
            _ => false,
        };
        if !matches {
            continue;
        }

        let next_depth = chain_depth + 1;
        if next_depth > sub.max_chain_depth {
            info!(
                sub_id = sub.id,
                "bus subscription skipped — chain depth {} exceeds max {}",
                next_depth,
                sub.max_chain_depth
            );
            continue;
        }

        let task = match repos.tasks().get(&sub.task_id).await {
            Ok(Some(task)) => task,
            Ok(None) => {
                error!(
                    "bus subscription {}: task {} not found",
                    sub.id, sub.task_id
                );
                continue;
            }
            Err(e) => {
                error!("bus subscription {}: {}", sub.id, e);
                continue;
            }
        };

        let msg_id = ulid::Ulid::new().to_string();
        let new_run_id = ulid::Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let log_path = format!(
            "{}/.orbit/logs/{}.log",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
            new_run_id
        );

        if let Err(e) = repos
            .runs()
            .create_bus_run(
                &new_run_id,
                &task,
                agent_id,
                run_id,
                &sub.subscriber_agent_id,
                &sub.event_type,
                &msg_id,
                &log_path,
                next_depth,
                &now,
            )
            .await
        {
            error!(
                "bus subscription {} failed to create records: {}",
                sub.id, e
            );
            continue;
        }

        // Emit event
        emit_bus_message_sent_to_host(
            host,
            &msg_id,
            agent_id,
            &sub.subscriber_agent_id,
            "event",
            serde_json::json!({ "event_type": sub.event_type.clone(), "source_run_id": run_id }),
            None,
            Some(&new_run_id),
        );

        // Send to executor
        let req = RunRequest {
            run_id: new_run_id.clone(),
            task,
            schedule_id: None,
            _trigger: "bus".to_string(),
            retry_count: 0,
            _parent_run_id: None,
            chain_depth: next_depth,
        };

        if let Err(e) = tx.send(req) {
            error!("bus subscription {}: failed to enqueue run: {}", sub.id, e);
        } else {
            info!(
                sub_id = sub.id,
                from_agent = agent_id,
                to_agent = sub.subscriber_agent_id.as_str(),
                run_id = new_run_id.as_str(),
                "bus subscription triggered"
            );
        }
    }
}
