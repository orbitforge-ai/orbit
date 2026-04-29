use chrono::Utc;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};

use crate::workflows::nodes::{NodeExecutionContext, NodeFailure, NodeOutcome};
use crate::workflows::seen_items::{filter_unseen_items, hash_text};
use crate::workflows::template::{normalize_http_body, render_template, required_template};

pub(super) async fn execute(ctx: &NodeExecutionContext<'_>) -> Result<NodeOutcome, NodeFailure> {
    let url_template = required_template(&ctx.node.data, "url", "integration.http.request")?;
    let url = render_template(&url_template, ctx.outputs)
        .trim()
        .to_string();
    if url.is_empty() {
        return Err("integration.http.request rendered an empty URL".into());
    }
    let client = reqwest::Client::builder()
        .user_agent("Orbit/0.1 workflow http request")
        .build()
        .map_err(|e| format!("integration.http.request client error: {}", e))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("integration.http.request failed: {}", e))?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let raw_body = response
        .text()
        .await
        .map_err(|e| format!("integration.http.request body read failed: {}", e))?;
    let body_text = normalize_http_body(&content_type, &raw_body);
    let parsed_json = serde_json::from_str::<Value>(&raw_body).ok();
    let fingerprint_item = json!({
        "url": url,
        "title": "",
        "publishedAt": Value::Null,
        "source": "http",
        "bodyHash": hash_text(&body_text),
    });
    let is_new = filter_unseen_items(
        ctx.repos,
        ctx.workflow_id,
        &ctx.node.id,
        &url,
        vec![fingerprint_item],
    )
    .await?
    .len()
        == 1;

    Ok(NodeOutcome {
        output: json!({
            "url": url,
            "status": status,
            "contentType": content_type,
            "bodyText": body_text,
            "json": parsed_json,
            "fetchedAt": Utc::now().to_rfc3339(),
            "isNew": is_new,
        }),
        next_handle: None,
    })
}
