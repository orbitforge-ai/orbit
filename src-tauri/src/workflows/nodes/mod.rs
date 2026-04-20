mod agent;
mod board;
mod code;
mod feed;
mod http;
mod logic;

use serde_json::Value;
use tauri::Runtime;

use crate::db::DbPool;
use crate::models::project_workflow::WorkflowNode;

pub(crate) struct NodeExecutionContext<'a, R: Runtime> {
    pub db: &'a DbPool,
    pub app: &'a tauri::AppHandle<R>,
    pub run_id: &'a str,
    pub workflow_id: &'a str,
    pub project_id: &'a str,
    pub node: &'a WorkflowNode,
    pub outputs: &'a Value,
}

pub(crate) struct NodeOutcome {
    pub output: Value,
    pub next_handle: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeExecutorKind {
    Trigger,
    Agent,
    Logic,
    Code,
    Feed,
    Http,
    ProposalQueue,
    WorkItem,
}

fn route_node_type(node_type: &str) -> Option<NodeExecutorKind> {
    match node_type {
        "trigger.manual" | "trigger.schedule" => Some(NodeExecutorKind::Trigger),
        "agent.run" => Some(NodeExecutorKind::Agent),
        "logic.if" => Some(NodeExecutorKind::Logic),
        "code.bash.run" | "code.script.run" => Some(NodeExecutorKind::Code),
        "integration.feed.fetch" => Some(NodeExecutorKind::Feed),
        "integration.http.request" => Some(NodeExecutorKind::Http),
        "board.proposal.enqueue" => Some(NodeExecutorKind::ProposalQueue),
        "board.work_item.create" => Some(NodeExecutorKind::WorkItem),
        _ => None,
    }
}

pub fn node_type_has_executor(node_type: &str) -> bool {
    route_node_type(node_type).is_some()
}

pub(crate) async fn execute<R: Runtime>(
    ctx: NodeExecutionContext<'_, R>,
) -> Result<NodeOutcome, String> {
    match route_node_type(ctx.node.node_type.as_str()) {
        Some(NodeExecutorKind::Trigger) => Ok(NodeOutcome {
            output: ctx.outputs.get("trigger").cloned().unwrap_or(Value::Null),
            next_handle: None,
        }),
        Some(NodeExecutorKind::Agent) => agent::execute(&ctx).await,
        Some(NodeExecutorKind::Logic) => logic::execute(&ctx),
        Some(NodeExecutorKind::Code) => code::execute(&ctx).await,
        Some(NodeExecutorKind::Feed) => feed::execute(&ctx).await,
        Some(NodeExecutorKind::Http) => http::execute(&ctx).await,
        Some(NodeExecutorKind::ProposalQueue) => board::execute_proposal_enqueue(&ctx).await,
        Some(NodeExecutorKind::WorkItem) => board::execute_work_item(&ctx).await,
        None => Err(format!("unknown node type `{}`", ctx.node.node_type)),
    }
}

#[cfg(test)]
mod tests {
    use super::{route_node_type, NodeExecutorKind};

    #[test]
    fn route_node_type_covers_supported_nodes() {
        let cases = [
            ("trigger.manual", Some(NodeExecutorKind::Trigger)),
            ("trigger.schedule", Some(NodeExecutorKind::Trigger)),
            ("agent.run", Some(NodeExecutorKind::Agent)),
            ("logic.if", Some(NodeExecutorKind::Logic)),
            ("code.bash.run", Some(NodeExecutorKind::Code)),
            ("code.script.run", Some(NodeExecutorKind::Code)),
            ("integration.feed.fetch", Some(NodeExecutorKind::Feed)),
            ("integration.http.request", Some(NodeExecutorKind::Http)),
            (
                "board.proposal.enqueue",
                Some(NodeExecutorKind::ProposalQueue),
            ),
            ("board.work_item.create", Some(NodeExecutorKind::WorkItem)),
            ("integration.slack.send", None),
        ];

        for (node_type, expected) in cases {
            assert_eq!(
                route_node_type(node_type),
                expected,
                "node_type={node_type}"
            );
        }
    }
}
