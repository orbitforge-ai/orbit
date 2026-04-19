pub use super::tools::context::ToolExecutionContext;

use std::sync::Arc;

use super::tools::{self, ToolHandler};
use crate::executor::llm_provider::ToolDefinition;
use crate::plugins::PluginManager;

/// Build the baseline set of builtin tools. Plugin tools are appended via
/// [`all_tools_with_plugins`]; call that when you have an AppHandle.
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
        Box::new(tools::image_analysis::ImageAnalysisTool),
        Box::new(tools::image_generation::ImageGenerationTool),
        Box::new(tools::config::ConfigTool),
        Box::new(tools::task::TaskTool),
        Box::new(tools::work_item::WorkItemTool),
        Box::new(tools::schedule::ScheduleTool),
        Box::new(tools::worktree::WorktreeTool),
        Box::new(tools::session_history::SessionHistoryTool),
        Box::new(tools::session_status::SessionStatusTool),
        Box::new(tools::sessions_list::SessionsListTool),
        Box::new(tools::session_send::SessionSendTool),
        Box::new(tools::sessions_spawn::SessionsSpawnTool),
        Box::new(tools::send_message::SendMessageTool),
        Box::new(tools::message::MessageTool),
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
        Box::new(tools::plugin_management::PluginManagementTool),
    ]
}

/// Build the tool definitions that are exposed to the LLM. Plugin-contributed
/// tools are appended unconditionally — they bypass the `allowed` filter so
/// installing a plugin doesn't require a separate settings change. Per-call
/// permission prompts gate their actual invocation.
pub fn build_tool_definitions(
    allowed: &[String],
    plugin_manager: Option<&Arc<PluginManager>>,
) -> Vec<ToolDefinition> {
    let tools = all_tools();
    let mut definitions: Vec<ToolDefinition> = if allowed.is_empty() {
        tools.iter().map(|tool| tool.definition()).collect()
    } else {
        tools
            .iter()
            .filter(|tool| {
                tool.name() == "react_to_message"
                    || tool.name() == "finish"
                    || tool.name() == "activate_skill"
                    || tool.name() == "yield_turn"
                    || allowed.contains(&tool.name().to_string())
            })
            .map(|tool| tool.definition())
            .collect()
    };

    if let Some(manager) = plugin_manager {
        let enabled_manifests: Vec<_> = manager
            .manifests()
            .into_iter()
            .filter(|m| manager.is_enabled(&m.id))
            .collect();
        let plugin_handlers = crate::plugins::tools::build_handlers(&enabled_manifests);
        for handler in &plugin_handlers {
            definitions.push(handler.definition());
        }
    }

    if !definitions
        .iter()
        .any(|tool| tool.name == "react_to_message")
    {
        definitions.push(tools::react_to_message::ReactToMessageTool.definition());
    }
    if !definitions.iter().any(|tool| tool.name == "finish") {
        definitions.push(tools::finish::FinishTool.definition());
    }
    if !definitions.iter().any(|tool| tool.name == "activate_skill") {
        definitions.push(tools::activate_skill::ActivateSkillTool.definition());
    }
    if !definitions.iter().any(|tool| tool.name == "yield_turn") {
        definitions.push(tools::yield_turn::YieldTurnTool.definition());
    }

    definitions
}

/// Execute a single tool call. Returns (result_text, is_finish). Plugin tool
/// names (containing `__`) are routed through the plugin tools layer; every
/// other tool falls back to the builtin list.
pub async fn execute_tool(
    ctx: &ToolExecutionContext,
    tool_name: &str,
    input: &serde_json::Value,
    app: &tauri::AppHandle,
    run_id: &str,
) -> Result<(String, bool), String> {
    if crate::plugins::tools::is_plugin_tool_name(tool_name) {
        let manager = crate::plugins::from_state(app);
        let enabled_manifests: Vec<_> = manager
            .manifests()
            .into_iter()
            .filter(|m| manager.is_enabled(&m.id))
            .collect();
        for handler in crate::plugins::tools::build_handlers(&enabled_manifests) {
            if handler.name() == tool_name {
                return handler.execute(ctx, input, app, run_id).await;
            }
        }
        return Err(format!("unknown plugin tool: {}", tool_name));
    }

    for tool in all_tools() {
        if tool.name() == tool_name {
            return tool.execute(ctx, input, app, run_id).await;
        }
    }

    Err(format!("unknown tool: {}", tool_name))
}

#[cfg(test)]
mod tests {
    use super::build_tool_definitions;

    #[test]
    fn finish_is_always_exposed_even_if_not_in_allowed_list() {
        let defs = build_tool_definitions(&["read_file".to_string()], None);
        assert!(defs.iter().any(|tool| tool.name == "read_file"));
        assert!(defs.iter().any(|tool| tool.name == "finish"));
        assert!(defs.iter().any(|tool| tool.name == "activate_skill"));
        assert!(defs.iter().any(|tool| tool.name == "yield_turn"));
    }

    #[test]
    fn react_to_message_is_always_exposed_even_if_not_in_allowed_list() {
        let defs = build_tool_definitions(&["read_file".to_string()], None);
        assert!(defs.iter().any(|tool| tool.name == "react_to_message"));
    }
}
