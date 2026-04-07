pub use super::tools::context::ToolExecutionContext;

use super::tools::{self, ToolHandler};
use crate::executor::llm_provider::ToolDefinition;

fn all_tools() -> Vec<Box<dyn ToolHandler>> {
    vec![
        Box::new(tools::shell_command::ShellCommandTool),
        Box::new(tools::read_file::ReadFileTool),
        Box::new(tools::write_file::WriteFileTool),
        Box::new(tools::ask_user::AskUserTool),
        Box::new(tools::edit_file::EditFileTool),
        Box::new(tools::list_files::ListFilesTool),
        Box::new(tools::grep::GrepTool),
        Box::new(tools::web_search::WebSearchTool),
        Box::new(tools::web_fetch::WebFetchTool),
        Box::new(tools::config::ConfigTool),
        Box::new(tools::task::TaskTool),
        Box::new(tools::worktree::WorktreeTool),
        Box::new(tools::session_history::SessionHistoryTool),
        Box::new(tools::session_status::SessionStatusTool),
        Box::new(tools::sessions_list::SessionsListTool),
        Box::new(tools::session_send::SessionSendTool),
        Box::new(tools::sessions_spawn::SessionsSpawnTool),
        Box::new(tools::send_message::SendMessageTool),
        Box::new(tools::activate_skill::ActivateSkillTool),
        Box::new(tools::spawn_sub_agents::SpawnSubAgentsTool),
        Box::new(tools::subagents::SubagentsTool),
        Box::new(tools::yield_turn::YieldTurnTool),
        Box::new(tools::remember::RememberTool),
        Box::new(tools::forget::ForgetTool),
        Box::new(tools::search_memory::SearchMemoryTool),
        Box::new(tools::list_memories::ListMemoriesTool),
        Box::new(tools::finish::FinishTool),
        Box::new(tools::react_to_message::ReactToMessageTool),
    ]
}

/// Build the tool definitions that are exposed to the LLM.
pub fn build_tool_definitions(allowed: &[String]) -> Vec<ToolDefinition> {
    let tools = all_tools();
    let mut definitions: Vec<ToolDefinition> = if allowed.is_empty() {
        tools.iter().map(|tool| tool.definition()).collect()
    } else {
        tools
            .iter()
            .filter(|tool| {
                tool.name() == "react_to_message" || allowed.contains(&tool.name().to_string())
            })
            .map(|tool| tool.definition())
            .collect()
    };

    if !definitions
        .iter()
        .any(|tool| tool.name == "react_to_message")
    {
        definitions.push(tools::react_to_message::ReactToMessageTool.definition());
    }

    definitions
}

/// Execute a single tool call. Returns (result_text, is_finish).
pub async fn execute_tool(
    ctx: &ToolExecutionContext,
    tool_name: &str,
    input: &serde_json::Value,
    app: &tauri::AppHandle,
    run_id: &str,
) -> Result<(String, bool), String> {
    for tool in all_tools() {
        if tool.name() == tool_name {
            return tool.execute(ctx, input, app, run_id).await;
        }
    }

    Err(format!("unknown tool: {}", tool_name))
}
