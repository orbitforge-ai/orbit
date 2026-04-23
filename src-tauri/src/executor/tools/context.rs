use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc;
use tracing::warn;

use crate::db::DbPool;
use crate::executor::engine::{
    AgentSemaphores, RunRequest, SessionExecutionRegistry, UserQuestionRegistry,
};
use crate::executor::memory::MemoryClient;
use crate::executor::permissions::PermissionRegistry;
use crate::executor::session_worktree::SessionWorktreeState;

#[derive(Debug, Clone)]
struct WorkspaceRouting {
    main_workspace_root: PathBuf,
    active_workspace_root: PathBuf,
    current_worktree: Option<SessionWorktreeState>,
}

#[derive(Debug, Clone)]
struct ResolvedWorkspaceRouting {
    main_workspace_root: PathBuf,
    active_workspace_root: PathBuf,
    current_worktree: Option<SessionWorktreeState>,
    invalid_worktree_reason: Option<String>,
}

/// Context for executing agent tools — provides sandboxed filesystem access
/// and optional Agent Bus capabilities.
pub struct ToolExecutionContext {
    /// The agent's ID (used for skill discovery and other lookups).
    pub agent_id: String,
    /// The agent's entire root directory (~/.orbit/agents/{agent_id}/).
    pub _agent_root: PathBuf,
    workspace_routing: Arc<RwLock<WorkspaceRouting>>,
    /// Which search provider to use for web_search (e.g. "brave", "tavily").
    pub web_search_provider: String,
    /// Skills explicitly disabled for this agent.
    pub disabled_skills: Vec<String>,
    // ─── Agent Bus fields ───────────────────────────────────────────────
    pub db: Option<DbPool>,
    pub executor_tx: Option<mpsc::UnboundedSender<RunRequest>>,
    pub app: Option<tauri::AppHandle>,
    pub current_agent_id: Option<String>,
    pub current_run_id: Option<String>,
    pub current_session_id: Option<String>,
    pub chain_depth: i64,
    pub agent_semaphores: Option<AgentSemaphores>,
    pub session_registry: Option<SessionExecutionRegistry>,
    /// Whether this context is for a sub-agent.
    pub is_sub_agent: bool,
    /// Whether this context may call spawn_sub_agents.
    pub allow_sub_agents: bool,
    /// Permission registry for gating tool execution.
    pub permission_registry: Option<PermissionRegistry>,
    /// Registry for ask_user prompts waiting on frontend responses.
    pub user_question_registry: Option<UserQuestionRegistry>,
    /// Optional memory client for long-term memory operations.
    pub memory_client: Option<MemoryClient>,
    /// User ID used for scoping memory operations (Supabase user_id when cloud, else "default_user").
    pub memory_user_id: String,
    /// Optional cloud client for syncing data to Supabase.
    pub cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
    sandbox_enabled: bool,
}

impl ToolExecutionContext {
    pub fn new_with_bus(
        agent_id: &str,
        run_id: &str,
        session_id: Option<&str>,
        chain_depth: i64,
        db: DbPool,
        executor_tx: mpsc::UnboundedSender<RunRequest>,
        app: tauri::AppHandle,
        agent_semaphores: AgentSemaphores,
        session_registry: SessionExecutionRegistry,
        worktree: Option<SessionWorktreeState>,
        project_id: Option<&str>,
    ) -> Self {
        let agent_root = crate::executor::workspace::agent_dir(agent_id);
        let ws_config = crate::executor::workspace::load_agent_config(agent_id).unwrap_or_default();
        let routing = resolve_workspace_routing(agent_id, project_id, worktree);
        if let Some(reason) = &routing.invalid_worktree_reason {
            warn!(
                agent_id = agent_id,
                reason = reason,
                "ignoring invalid session worktree"
            );
        }
        let global = crate::executor::global_settings::load_global_settings();
        Self {
            agent_id: agent_id.to_string(),
            _agent_root: agent_root,
            workspace_routing: Arc::new(RwLock::new(WorkspaceRouting {
                main_workspace_root: routing.main_workspace_root,
                active_workspace_root: routing.active_workspace_root,
                current_worktree: routing.current_worktree,
            })),
            web_search_provider: global.agent_defaults.web_search_provider,
            disabled_skills: ws_config.disabled_skills,
            db: Some(db),
            executor_tx: Some(executor_tx),
            app: Some(app),
            current_agent_id: Some(agent_id.to_string()),
            current_run_id: Some(run_id.to_string()),
            current_session_id: session_id.map(|s| s.to_string()),
            chain_depth,
            agent_semaphores: Some(agent_semaphores),
            session_registry: Some(session_registry),
            is_sub_agent: false,
            allow_sub_agents: true,
            permission_registry: None,
            user_question_registry: None,
            memory_client: None,
            memory_user_id: "default_user".to_string(),
            cloud_client: None,
            sandbox_enabled: ws_config.enable_sandbox,
        }
    }

    /// Set the permission registry on this context (builder pattern).
    pub fn with_permission_registry(mut self, registry: PermissionRegistry) -> Self {
        self.permission_registry = Some(registry);
        self
    }

    /// Set the user question registry on this context.
    pub fn with_user_question_registry(mut self, registry: UserQuestionRegistry) -> Self {
        self.user_question_registry = Some(registry);
        self
    }

    /// Override whether this context may spawn sub-agents.
    pub fn with_allow_sub_agents(mut self, allow_sub_agents: bool) -> Self {
        self.allow_sub_agents = allow_sub_agents;
        self
    }

    /// Set the memory client on this context (builder pattern).
    pub fn with_memory_client(mut self, client: Option<MemoryClient>) -> Self {
        self.memory_client = client;
        self
    }

    /// Set the user ID for memory scoping (builder pattern).
    pub fn with_memory_user_id(mut self, user_id: String) -> Self {
        self.memory_user_id = user_id;
        self
    }

    /// Set the cloud client for syncing (builder pattern).
    pub fn with_cloud_client(
        mut self,
        client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
    ) -> Self {
        self.cloud_client = client;
        self
    }

    pub fn new_for_sub_agent(
        agent_id: &str,
        run_id: &str,
        session_id: Option<&str>,
        chain_depth: i64,
        db: DbPool,
        executor_tx: mpsc::UnboundedSender<RunRequest>,
        app: tauri::AppHandle,
        agent_semaphores: AgentSemaphores,
        session_registry: SessionExecutionRegistry,
        worktree: Option<SessionWorktreeState>,
        project_id: Option<&str>,
    ) -> Self {
        let mut ctx = Self::new_with_bus(
            agent_id,
            run_id,
            session_id,
            chain_depth,
            db,
            executor_tx,
            app,
            agent_semaphores,
            session_registry,
            worktree,
            project_id,
        );
        ctx.is_sub_agent = true;
        ctx.allow_sub_agents = false;
        ctx
    }

    pub fn workspace_root(&self) -> PathBuf {
        self.workspace_routing
            .read()
            .expect("workspace routing poisoned")
            .active_workspace_root
            .clone()
    }

    pub fn main_workspace_root(&self) -> PathBuf {
        self.workspace_routing
            .read()
            .expect("workspace routing poisoned")
            .main_workspace_root
            .clone()
    }

    pub fn current_worktree(&self) -> Option<SessionWorktreeState> {
        self.workspace_routing
            .read()
            .expect("workspace routing poisoned")
            .current_worktree
            .clone()
    }

    pub fn sandbox_enabled(&self) -> bool {
        self.sandbox_enabled
    }

    pub fn set_current_worktree(&self, worktree: Option<SessionWorktreeState>) {
        let mut routing = self
            .workspace_routing
            .write()
            .expect("workspace routing poisoned");
        routing.active_workspace_root = worktree
            .as_ref()
            .map(|state| state.path.clone())
            .unwrap_or_else(|| routing.main_workspace_root.clone());
        routing.current_worktree = worktree;
    }
}

fn resolve_workspace_routing(
    agent_id: &str,
    project_id: Option<&str>,
    worktree: Option<SessionWorktreeState>,
) -> ResolvedWorkspaceRouting {
    let main_workspace_root = match project_id {
        Some(pid) => crate::executor::workspace::project_workspace_dir(pid),
        None => crate::executor::workspace::agent_workspace_dir(agent_id),
    };

    let (current_worktree, invalid_worktree_reason) = match worktree {
        Some(state) => match validate_worktree_scope(agent_id, &main_workspace_root, &state) {
            Ok(valid) => (Some(valid.clone()), None),
            Err(reason) => (None, Some(reason)),
        },
        None => (None, None),
    };

    let active_workspace_root = current_worktree
        .as_ref()
        .map(|state| state.path.clone())
        .unwrap_or_else(|| main_workspace_root.clone());

    ResolvedWorkspaceRouting {
        main_workspace_root,
        active_workspace_root,
        current_worktree,
        invalid_worktree_reason,
    }
}

fn validate_worktree_scope(
    agent_id: &str,
    main_workspace_root: &Path,
    state: &SessionWorktreeState,
) -> Result<SessionWorktreeState, String> {
    if !state.path.exists() {
        return Err(format!(
            "worktree path no longer exists: {}",
            state.path.display()
        ));
    }

    let canonical_worktree = state
        .path
        .canonicalize()
        .map_err(|e| format!("failed to resolve worktree path: {}", e))?;
    let managed_root = crate::executor::workspace::agent_worktrees_dir(agent_id)
        .canonicalize()
        .map_err(|e| format!("failed to resolve managed worktrees dir: {}", e))?;
    if !canonical_worktree.starts_with(&managed_root) {
        return Err(format!(
            "worktree path escapes managed worktrees dir: {}",
            canonical_worktree.display()
        ));
    }

    let expected_common_dir = main_workspace_root
        .join(".git")
        .canonicalize()
        .map_err(|e| format!("failed to resolve workspace git dir: {}", e))?;
    let actual_common_dir = resolve_git_common_dir(&canonical_worktree)?;
    if actual_common_dir != expected_common_dir {
        return Err(format!(
            "worktree repo does not match scoped workspace (expected {}, got {})",
            expected_common_dir.display(),
            actual_common_dir.display()
        ));
    }

    Ok(SessionWorktreeState {
        name: state.name.clone(),
        branch: state.branch.clone(),
        path: canonical_worktree,
    })
}

fn resolve_git_common_dir(worktree_path: &Path) -> Result<PathBuf, String> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--git-common-dir")
        .current_dir(worktree_path)
        .output()
        .map_err(|e| format!("failed to run git rev-parse: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if stderr.is_empty() { stdout } else { stderr };
        return Err(if message.is_empty() {
            "git rev-parse --git-common-dir failed".to_string()
        } else {
            format!("git rev-parse --git-common-dir failed: {}", message)
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Err("git rev-parse --git-common-dir returned an empty path".to_string());
    }

    let common_dir = PathBuf::from(&raw);
    let resolved = if common_dir.is_absolute() {
        common_dir
    } else {
        worktree_path.join(common_dir)
    };

    resolved
        .canonicalize()
        .map_err(|e| format!("failed to resolve git common dir: {}", e))
}

pub async fn sanitize_session_worktree_state(
    db: &DbPool,
    session_id: &str,
    agent_id: &str,
    project_id: Option<&str>,
    worktree: Option<SessionWorktreeState>,
    cloud_client: Option<std::sync::Arc<crate::db::cloud::SupabaseClient>>,
) -> Result<Option<SessionWorktreeState>, String> {
    let routing = resolve_workspace_routing(agent_id, project_id, worktree);

    if let Some(reason) = routing.invalid_worktree_reason.as_deref() {
        warn!(
            session_id = session_id,
            agent_id = agent_id,
            ?project_id,
            reason = reason,
            "clearing invalid session worktree"
        );
        crate::executor::session_worktree::set_session_worktree_state(db, session_id, None).await?;

        if let Some(client) = cloud_client {
            let session_id = session_id.to_string();
            tokio::spawn(async move {
                let body = serde_json::json!({
                    "worktree_name": serde_json::Value::Null,
                    "worktree_branch": serde_json::Value::Null,
                    "worktree_path": serde_json::Value::Null,
                    "updated_at": chrono::Utc::now().to_rfc3339(),
                });
                if let Err(err) = client.patch_by_id("chat_sessions", &session_id, body).await {
                    tracing::warn!("cloud patch session worktree {}: {}", session_id, err);
                }
            });
        }
    }

    Ok(routing.current_worktree)
}

#[cfg(test)]
mod tests {
    use super::{resolve_workspace_routing, validate_worktree_scope};
    use crate::executor::session_worktree::SessionWorktreeState;
    use crate::executor::workspace;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Mutex, OnceLock};

    fn home_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn unique_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "orbit-workspace-context-{}-{}",
            name,
            ulid::Ulid::new()
        ))
    }

    fn init_git_repo(path: &Path) {
        fs::create_dir_all(path).expect("create repo dir");
        let status = Command::new("git")
            .arg("init")
            .current_dir(path)
            .status()
            .expect("run git init");
        assert!(status.success());
        fs::write(path.join("README.md"), "workspace").expect("write seed file");
        let status = Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .status()
            .expect("run git add");
        assert!(status.success());
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=Orbit Tests",
                "-c",
                "user.email=orbit@example.com",
                "commit",
                "-m",
                "seed",
            ])
            .current_dir(path)
            .status()
            .expect("run git commit");
        assert!(status.success());
    }

    fn create_worktree(main_repo: &Path, worktree_path: &Path) {
        let status = Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(worktree_path)
            .arg("HEAD")
            .current_dir(main_repo)
            .status()
            .expect("run git worktree add");
        assert!(status.success());
    }

    #[test]
    fn project_scope_never_falls_back_to_agent_workspace() {
        let agent_id = format!("agent-{}", ulid::Ulid::new());
        let project_id = format!("project-{}", ulid::Ulid::new());
        let routing = resolve_workspace_routing(&agent_id, Some(&project_id), None);

        assert_eq!(
            routing.main_workspace_root,
            workspace::project_workspace_dir(&project_id)
        );
        assert_eq!(routing.active_workspace_root, routing.main_workspace_root);
    }

    #[test]
    fn worktree_validation_rejects_repo_from_wrong_scope() {
        let _guard = home_env_lock().lock().expect("lock HOME");
        let previous_home = env::var("HOME").ok();
        let temp_home = unique_path("home");
        fs::create_dir_all(&temp_home).expect("create temp home");
        env::set_var("HOME", &temp_home);

        let agent_id = format!("agent-{}", ulid::Ulid::new());
        let project_root = unique_path("project-main");
        let other_root = unique_path("other-main");
        init_git_repo(&project_root);
        init_git_repo(&other_root);

        let managed_root = workspace::agent_worktrees_dir(&agent_id);
        fs::create_dir_all(&managed_root).expect("create managed root");
        let worktree_path = managed_root.join("wrong-scope");
        create_worktree(&other_root, &worktree_path);

        let result = validate_worktree_scope(
            &agent_id,
            &project_root,
            &SessionWorktreeState {
                name: "wrong-scope".to_string(),
                branch: "orbit/wrong-scope".to_string(),
                path: worktree_path.clone(),
            },
        );

        assert!(result.is_err());

        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(&other_root)
            .status();
        let _ = fs::remove_dir_all(project_root);
        let _ = fs::remove_dir_all(other_root);
        let _ = fs::remove_dir_all(workspace::agent_dir(&agent_id));
        let _ = fs::remove_dir_all(temp_home);
        if let Some(home) = previous_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }
    }
}
