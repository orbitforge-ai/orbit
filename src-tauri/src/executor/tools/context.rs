use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc;

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
    ) -> Self {
        let agent_root = crate::executor::workspace::agent_dir(agent_id);
        let main_workspace_root = crate::executor::workspace::agent_workspace_dir(agent_id);
        let active_workspace_root = worktree
            .as_ref()
            .map(|state| state.path.clone())
            .unwrap_or_else(|| main_workspace_root.clone());
        let ws_config = crate::executor::workspace::load_agent_config(agent_id).unwrap_or_default();
        let global = crate::executor::global_settings::load_global_settings();
        Self {
            agent_id: agent_id.to_string(),
            _agent_root: agent_root,
            workspace_routing: Arc::new(RwLock::new(WorkspaceRouting {
                main_workspace_root,
                active_workspace_root,
                current_worktree: worktree,
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
