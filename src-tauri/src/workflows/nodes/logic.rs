use serde_json::json;

use crate::models::project_workflow::RuleNode;
use crate::workflows::nodes::{NodeExecutionContext, NodeOutcome};
use crate::workflows::rule_eval::eval_rule;

pub(super) fn execute<R: tauri::Runtime>(
    ctx: &NodeExecutionContext<'_, R>,
) -> Result<NodeOutcome, String> {
    let rule_value = ctx
        .node
        .data
        .get("rule")
        .ok_or_else(|| "logic.if requires data.rule".to_string())?;
    let rule: RuleNode = serde_json::from_value(rule_value.clone())
        .map_err(|e| format!("invalid logic.if rule: {}", e))?;

    let result = eval_rule(&rule, ctx.outputs);
    let handle = if result { "true" } else { "false" };
    Ok(NodeOutcome {
        output: json!({ "result": result, "branch": handle }),
        next_handle: Some(handle.to_string()),
    })
}
