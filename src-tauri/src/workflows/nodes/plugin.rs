use serde_json::{json, Map, Value};

use crate::plugins;
use crate::workflows::nodes::{NodeExecutionContext, NodeOutcome};
use crate::workflows::template::render_template;

pub(super) async fn execute<R: tauri::Runtime>(
    ctx: &NodeExecutionContext<'_, R>,
) -> Result<NodeOutcome, String> {
    let manager = plugins::from_state(ctx.app);
    let (manifest, tool_name) = manager
        .manifests()
        .into_iter()
        .find_map(|manifest| {
            manifest
                .workflow
                .nodes
                .iter()
                .find(|node| node.kind == ctx.node.node_type)
                .map(|node| (manifest.clone(), node.tool.clone()))
        })
        .ok_or_else(|| {
            format!(
                "workflow node `{}` is not declared by any installed plugin",
                ctx.node.node_type
            )
        })?;

    if !manager.is_enabled(&manifest.id) {
        return Err(format!("plugin '{}' is disabled", manifest.id));
    }

    let rendered_input = render_value(&ctx.node.data, ctx.outputs);
    let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
    let raw = manager
        .runtime
        .call_tool(&manifest, &tool_name, &rendered_input, &extra_env)
        .await?;

    Ok(NodeOutcome {
        output: json!({
            "pluginId": manifest.id,
            "tool": tool_name,
            "input": rendered_input,
            "result": unwrap_mcp_text_payload(raw),
        }),
        next_handle: None,
    })
}

fn render_value(value: &Value, outputs: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(render_template(text, outputs)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| render_value(item, outputs))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), render_value(value, outputs)))
                .collect::<Map<String, Value>>(),
        ),
        other => other.clone(),
    }
}

fn unwrap_mcp_text_payload(raw: Value) -> Value {
    let text = raw
        .as_object()
        .and_then(|obj| obj.get("content"))
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(|value| value.as_str());

    match text {
        Some(text) => {
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.to_string()))
        }
        None => raw,
    }
}
