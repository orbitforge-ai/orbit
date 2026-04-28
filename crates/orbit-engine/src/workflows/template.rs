use serde_json::Value;

use crate::models::project_workflow::WorkflowGraph;

pub(crate) const OUTPUT_ALIASES_KEY: &str = "__aliases";

pub(crate) fn render_template(template: &str, outputs: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = find_close(&bytes[i + 2..]) {
                let path = template[i + 2..i + 2 + end].trim();
                let value = lookup_path(path, outputs);
                out.push_str(&value);
                i += 2 + end + 2;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_close(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn lookup_path(path: &str, outputs: &Value) -> String {
    let mut segments = path.split('.');
    let Some(first) = segments.next() else {
        return format!("{{{{{}}}}}", path);
    };

    let Some(resolved_first) = resolve_output_root_segment(first, outputs) else {
        return format!("{{{{{}}}}}", path);
    };

    let Some(mut cur) = outputs.get(resolved_first) else {
        return format!("{{{{{}}}}}", path);
    };

    for segment in segments {
        match cur.get(segment) {
            Some(v) => cur = v,
            None => return format!("{{{{{}}}}}", path),
        }
    }
    match cur {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub(crate) fn lookup_json_path(path: &str, outputs: &Value) -> Option<Value> {
    let mut segments = path.split('.');
    let first = segments.next()?;
    let resolved_first = resolve_output_root_segment(first, outputs)?;
    let mut cur = outputs.get(resolved_first)?;
    for segment in segments {
        cur = cur.get(segment)?;
    }
    Some(cur.clone())
}

pub(crate) fn build_reference_aliases(graph: &WorkflowGraph) -> serde_json::Map<String, Value> {
    let mut aliases = serde_json::Map::new();
    for node in &graph.nodes {
        if let Some(reference_key) = workflow_node_reference_key(&node.data) {
            aliases.insert(reference_key.to_string(), Value::String(node.id.clone()));
        }
    }
    aliases
}

fn workflow_node_reference_key(data: &Value) -> Option<&str> {
    data.get("referenceKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn resolve_output_root_segment<'a>(
    segment: &'a str,
    outputs: &'a Value,
) -> Option<&'a str> {
    if outputs.get(segment).is_some() {
        return Some(segment);
    }
    outputs.get(OUTPUT_ALIASES_KEY)?.get(segment)?.as_str()
}

pub(crate) fn render_agent_prompt(
    template: &str,
    context: Option<&str>,
    output_mode: &str,
    outputs: &Value,
) -> String {
    let prompt = render_template(template, outputs);
    match output_mode {
        "proposal_candidates" => {
            let context_block = context
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("\n\nFit context:\n{}", value))
                .unwrap_or_default();
            format!(
                "{}{}\n\nReturn only valid JSON as an array. Each array item must include: listing, fitScore, fitReason, proposalDraft, shouldReview.",
                prompt, context_block
            )
        }
        "json" => {
            let context_block = context
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("\n\nContext:\n{}", value))
                .unwrap_or_default();
            format!("{}{}\n\nReturn only valid JSON.", prompt, context_block)
        }
        _ => match context {
            Some(value) if !value.trim().is_empty() => format!("{}\n\nContext:\n{}", prompt, value),
            _ => prompt,
        },
    }
}

pub(crate) fn parse_agent_output(output_mode: &str, text: &str) -> Result<Value, String> {
    match output_mode {
        "proposal_candidates" => {
            let parsed: Value = serde_json::from_str(text)
                .map_err(|e| format!("agent.run expected JSON array output: {}", e))?;
            let items = parsed.as_array().ok_or_else(|| {
                "agent.run proposal_candidates output must be an array".to_string()
            })?;
            for (idx, item) in items.iter().enumerate() {
                let obj = item.as_object().ok_or_else(|| {
                    format!(
                        "agent.run proposal_candidates item {} must be an object",
                        idx
                    )
                })?;
                for field in [
                    "listing",
                    "fitScore",
                    "fitReason",
                    "proposalDraft",
                    "shouldReview",
                ] {
                    if !obj.contains_key(field) {
                        return Err(format!(
                            "agent.run proposal_candidates item {} is missing '{}'",
                            idx, field
                        ));
                    }
                }
            }
            Ok(parsed)
        }
        "json" => {
            serde_json::from_str(text).map_err(|e| format!("agent.run expected JSON output: {}", e))
        }
        _ => Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))),
    }
}

pub(crate) fn parse_multiline_templates(template: &str, outputs: &Value) -> Vec<String> {
    render_template(template, outputs)
        .split('\n')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn normalize_http_body(content_type: &str, raw_body: &str) -> String {
    if content_type.contains("html") {
        html2md::parse_html(raw_body)
    } else {
        raw_body.to_string()
    }
}

pub(crate) fn render_optional_template(template: Option<&str>, outputs: &Value) -> Option<String> {
    template
        .map(|value| render_template(value, outputs))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn required_template(data: &Value, field: &str, action: &str) -> Result<String, String> {
    data.get(field)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("board.work_item {} requires data.{}", action, field))
}

pub(crate) fn render_required_field(
    data: &Value,
    field: &str,
    action: &str,
    outputs: &Value,
) -> Result<String, String> {
    let template = required_template(data, field, action)?;
    let rendered = render_template(&template, outputs).trim().to_string();
    if rendered.is_empty() {
        Err(format!(
            "board.work_item {} rendered an empty {}",
            action, field
        ))
    } else {
        Ok(rendered)
    }
}

pub(crate) fn parse_work_item_labels(template: Option<&str>, outputs: &Value) -> Vec<String> {
    let Some(rendered) = render_optional_template(template, outputs) else {
        return Vec::new();
    };

    rendered
        .split([',', '\n'])
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn optional_labels(template: Option<&str>, outputs: &Value) -> Option<Vec<String>> {
    template.map(|_| parse_work_item_labels(template, outputs))
}

pub(crate) fn parse_work_item_kind(value: Option<&str>) -> Result<String, String> {
    let kind = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("task");
    if matches!(kind, "task" | "bug" | "story" | "spike" | "chore") {
        Ok(kind.to_string())
    } else {
        Err(format!("board.work_item has invalid kind '{}'", kind))
    }
}

pub(crate) fn parse_optional_work_item_kind(value: Option<&str>) -> Result<Option<String>, String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some("any") | Some("all") | None => Ok(None),
        Some(kind) => parse_work_item_kind(Some(kind)).map(Some),
    }
}

pub(crate) fn parse_work_item_status(value: Option<&str>) -> Result<String, String> {
    let status = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("backlog");
    if matches!(
        status,
        "backlog" | "todo" | "in_progress" | "blocked" | "review" | "done" | "cancelled"
    ) {
        Ok(status.to_string())
    } else {
        Err(format!("board.work_item has invalid status '{}'", status))
    }
}

pub(crate) fn parse_optional_work_item_status(
    value: Option<&str>,
) -> Result<Option<String>, String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some("any") | Some("all") | None => Ok(None),
        Some(status) => parse_work_item_status(Some(status)).map(Some),
    }
}

pub(crate) fn parse_priority(value: Option<&Value>) -> i64 {
    value.and_then(json_number_to_i64).unwrap_or(0)
}

pub(crate) fn parse_optional_priority(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(json_number_to_i64)
        .map(|priority| priority.clamp(0, 3))
}

pub(crate) fn json_number_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| value.as_f64().map(|n| n.round() as i64))
}

#[cfg(test)]
mod tests {
    use super::{
        build_reference_aliases, parse_work_item_labels, render_optional_template, render_template,
    };
    use crate::models::project_workflow::{NodePosition, WorkflowGraph, WorkflowNode};
    use serde_json::{json, Value};

    #[test]
    fn template_renders_known_paths() {
        let outputs = json!({
            "trigger": { "data": { "subject": "Hi" } }
        });
        assert_eq!(
            render_template("Subject: {{trigger.data.subject}}", &outputs),
            "Subject: Hi"
        );
    }

    #[test]
    fn template_leaves_unknown_paths_intact() {
        let outputs = json!({});
        assert_eq!(
            render_template("Hello {{missing.path}}!", &outputs),
            "Hello {{missing.path}}!"
        );
    }

    #[test]
    fn template_renders_reference_key_aliases() {
        let outputs = json!({
            "__aliases": { "triage-agent": "n2" },
            "n2": { "output": { "text": "Hello" } }
        });
        assert_eq!(
            render_template("Reply: {{triage-agent.output.text}}", &outputs),
            "Reply: Hello"
        );
    }

    #[test]
    fn optional_template_trims_empty_values() {
        let outputs = json!({ "trigger": { "data": { "value": "  hi  " } } });
        assert_eq!(
            render_optional_template(Some(" {{trigger.data.value}} "), &outputs),
            Some("hi".into())
        );
        assert_eq!(render_optional_template(Some("   "), &outputs), None);
    }

    #[test]
    fn label_parser_supports_commas_and_newlines() {
        let outputs = json!({
            "trigger": { "data": { "channel": "email" } }
        });
        assert_eq!(
            parse_work_item_labels(Some("triage, {{trigger.data.channel}}\ncustomer"), &outputs),
            vec!["triage", "email", "customer"]
        );
    }

    #[test]
    fn build_reference_aliases_collects_named_nodes() {
        let graph = WorkflowGraph {
            nodes: vec![
                WorkflowNode {
                    id: "n1".into(),
                    node_type: "agent.run".into(),
                    position: NodePosition { x: 0.0, y: 0.0 },
                    data: json!({ "referenceKey": "triage-agent" }),
                },
                WorkflowNode {
                    id: "n2".into(),
                    node_type: "agent.run".into(),
                    position: NodePosition { x: 1.0, y: 0.0 },
                    data: json!({}),
                },
            ],
            edges: Vec::new(),
            schema_version: 1,
        };

        let aliases = build_reference_aliases(&graph);
        assert_eq!(
            aliases.get("triage-agent"),
            Some(&Value::String("n1".into()))
        );
        assert_eq!(aliases.len(), 1);
    }
}
