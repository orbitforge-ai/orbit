use std::io::Cursor;

use feed_rs::parser;
use serde_json::json;

use crate::workflows::nodes::{NodeExecutionContext, NodeOutcome};
use crate::workflows::seen_items::filter_unseen_items;
use crate::workflows::template::{
    json_number_to_i64, parse_multiline_templates, required_template,
};

pub(super) async fn execute(ctx: &NodeExecutionContext<'_>) -> Result<NodeOutcome, String> {
    let feed_urls_text =
        required_template(&ctx.node.data, "feedUrlsText", "integration.feed.fetch")?;
    let feed_urls = parse_multiline_templates(&feed_urls_text, ctx.outputs);
    if feed_urls.is_empty() {
        return Err("integration.feed.fetch requires at least one feed URL".to_string());
    }

    let limit = ctx
        .node
        .data
        .get("limit")
        .and_then(json_number_to_i64)
        .filter(|value| *value > 0)
        .unwrap_or(50) as usize;
    let client = reqwest::Client::builder()
        .user_agent("Orbit/0.1 workflow feed fetch")
        .build()
        .map_err(|e| format!("integration.feed.fetch client error: {}", e))?;

    let mut unseen_items = Vec::new();
    let mut fetched = Vec::new();
    let mut total_items = 0usize;

    for url in feed_urls {
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("integration.feed.fetch request failed for {}: {}", url, e))?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("integration.feed.fetch body failed for {}: {}", url, e))?;
        let feed = parser::parse(Cursor::new(bytes))
            .map_err(|e| format!("integration.feed.fetch parse failed for {}: {}", url, e))?;
        fetched.push(url.clone());

        let mut normalized = Vec::new();
        let feed_title = feed.title.as_ref().map(|title| title.content.clone());
        for entry in feed.entries {
            let item = json!({
                "source": url,
                "feedTitle": feed_title.clone(),
                "id": entry.id,
                "title": entry.title.as_ref().map(|title| title.content.clone()).unwrap_or_default(),
                "url": entry.links.first().map(|link| link.href.clone()),
                "summary": entry.summary.as_ref().map(|summary| summary.content.clone()),
                "content": entry.content.and_then(|content| content.body),
                "publishedAt": entry.published.map(|value| value.to_rfc3339()),
                "updatedAt": entry.updated.map(|value| value.to_rfc3339()),
                "authors": entry.authors.iter().map(|author| author.name.clone()).collect::<Vec<_>>(),
            });
            normalized.push(item);
        }

        total_items += normalized.len();
        if normalized.len() > limit {
            normalized.truncate(limit);
        }
        let mut new_items =
            filter_unseen_items(ctx.db, ctx.workflow_id, &ctx.node.id, &url, normalized).await?;
        unseen_items.append(&mut new_items);
    }

    Ok(NodeOutcome {
        output: json!({
            "sourceUrls": fetched,
            "count": unseen_items.len(),
            "totalFetched": total_items,
            "items": unseen_items,
        }),
        next_handle: None,
    })
}
