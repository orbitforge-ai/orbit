mod agent;
mod board;
mod code;
mod feed;
mod http;
mod logic;
mod plugin;

use serde_json::Value;

use crate::db::repos::Repos;
use crate::db::DbPool;
use crate::models::project_workflow::WorkflowNode;
use crate::runtime_host::RuntimeHost;

pub(crate) struct NodeExecutionContext<'a> {
    pub db: &'a DbPool,
    pub repos: &'a dyn Repos,
    pub host: &'a dyn RuntimeHost,
    pub run_id: &'a str,
    pub workflow_id: &'a str,
    pub project_id: &'a str,
    pub node: &'a WorkflowNode,
    pub outputs: &'a Value,
}

impl<'a> NodeExecutionContext<'a> {
    pub fn app_handle(&self) -> Option<tauri::AppHandle> {
        self.host.app_handle()
    }
}

pub(crate) struct NodeOutcome {
    pub output: Value,
    pub next_handle: Option<String>,
}

pub(crate) struct NodeFailure {
    pub message: String,
    pub partial_output: Option<Value>,
}

impl NodeFailure {
    pub fn with_output(message: impl Into<String>, output: Value) -> Self {
        Self {
            message: message.into(),
            partial_output: Some(output),
        }
    }
}

impl From<String> for NodeFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            partial_output: None,
        }
    }
}

impl From<&str> for NodeFailure {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
            partial_output: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeExecutorKind {
    Trigger,
    Agent,
    Logic,
    Code,
    Feed,
    Http,
    Plugin,
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
        "integration.com_orbit_discord.send_message" => Some(NodeExecutorKind::Plugin),
        "board.proposal.enqueue" => Some(NodeExecutorKind::ProposalQueue),
        "board.work_item.create" => Some(NodeExecutorKind::WorkItem),
        _ => None,
    }
}

pub fn node_type_has_executor(node_type: &str) -> bool {
    route_node_type(node_type).is_some()
}

pub(crate) async fn execute(ctx: NodeExecutionContext<'_>) -> Result<NodeOutcome, NodeFailure> {
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
        Some(NodeExecutorKind::Plugin) => plugin::execute(&ctx).await,
        Some(NodeExecutorKind::ProposalQueue) => board::execute_proposal_enqueue(&ctx).await,
        Some(NodeExecutorKind::WorkItem) => board::execute_work_item(&ctx).await,
        None => Err(format!("unknown node type `{}`", ctx.node.node_type).into()),
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
                "integration.com_orbit_discord.send_message",
                Some(NodeExecutorKind::Plugin),
            ),
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
