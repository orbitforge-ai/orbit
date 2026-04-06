pub mod activate_skill;
pub mod ask_user;
pub mod config;
pub mod context;
pub mod edit_file;
pub mod finish;
pub mod forget;
pub mod grep;
pub mod helpers;
pub mod list_files;
pub mod list_memories;
pub mod react_to_message;
pub mod read_file;
pub mod remember;
pub mod search_memory;
pub mod send_message;
pub mod session_control;
pub mod session_helpers;
pub mod session_history;
pub mod session_send;
pub mod session_status;
pub mod sessions_list;
pub mod sessions_spawn;
pub mod shell_command;
pub mod spawn_sub_agents;
pub mod subagents;
pub mod web_fetch;
pub mod web_search;
pub mod worktree;
pub mod write_file;
pub mod yield_turn;

use crate::executor::llm_provider::ToolDefinition;

#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    fn name(&self) -> &'static str;

    fn definition(&self) -> ToolDefinition;

    async fn execute(
        &self,
        ctx: &context::ToolExecutionContext,
        input: &serde_json::Value,
        app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String>;
}
