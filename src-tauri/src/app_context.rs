//! Transport-agnostic bundle of shared application state.
//!
//! Rationale: Tauri commands receive dependencies via `tauri::State<T>`
//! extractors, which only work inside the Tauri runtime. The HTTP/WS shim
//! (and the future standalone cloud server) both need a way to hand the same
//! dependencies to command adapters without going through Tauri's extractor
//! machinery. `AppContext` is that bundle: an `Arc`-wrapped struct whose
//! fields are the `Clone`-able state objects already used by the rest of the
//! app.
//!
//! In desktop builds, `tauri` is `Some(AppHandle)` so event emission remains
//! unchanged. On a cloud server the same fields are populated from env/config
//! and `tauri` is `None`; the shim bus handles event fan-out.

use std::sync::Arc;

use crate::auth::AuthState;
use crate::commands::users::ActiveUser;
use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::executor::bg_processes::BgProcessRegistry;
use crate::executor::engine::{
    AgentSemaphores, ExecutorTx, SessionExecutionRegistry, UserQuestionRegistry,
};
use crate::executor::mcp_server::McpServerHandle;
use crate::executor::permissions::PermissionRegistry;
use crate::memory_service::MemoryServiceState;
use crate::plugins::PluginManager;

/// Shared state handed to every HTTP adapter.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AppContext {
    pub db: DbPool,
    pub auth: AuthState,
    pub cloud: CloudClientState,
    pub active_user: ActiveUser,
    pub executor_tx: ExecutorTx,
    pub agent_semaphores: AgentSemaphores,
    pub sessions: SessionExecutionRegistry,
    pub permissions: PermissionRegistry,
    pub user_questions: UserQuestionRegistry,
    pub bg_processes: BgProcessRegistry,
    pub mcp: McpServerHandle,
    pub plugins: Arc<PluginManager>,
    pub memory: Option<MemoryServiceState>,
    /// `None` when running on a cloud server with no Tauri runtime.
    pub tauri: Option<tauri::AppHandle>,
}

impl AppContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: DbPool,
        auth: AuthState,
        cloud: CloudClientState,
        active_user: ActiveUser,
        executor_tx: ExecutorTx,
        agent_semaphores: AgentSemaphores,
        sessions: SessionExecutionRegistry,
        permissions: PermissionRegistry,
        user_questions: UserQuestionRegistry,
        bg_processes: BgProcessRegistry,
        mcp: McpServerHandle,
        plugins: Arc<PluginManager>,
        memory: Option<MemoryServiceState>,
        tauri: Option<tauri::AppHandle>,
    ) -> Self {
        Self {
            db,
            auth,
            cloud,
            active_user,
            executor_tx,
            agent_semaphores,
            sessions,
            permissions,
            user_questions,
            bg_processes,
            mcp,
            plugins,
            memory,
            tauri,
        }
    }
}
