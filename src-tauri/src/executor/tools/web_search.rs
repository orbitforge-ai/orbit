use serde_json::json;

use crate::executor::llm_provider::ToolDefinition;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct WebSearchTool;

async fn execute_web_search(provider: &str, query: &str, count: u32) -> Result<String, String> {
    match provider {
        "brave" => brave_search(query, count).await,
        "tavily" => tavily_search(query, count).await,
        other => Err(format!("unsupported search provider: {}", other)),
    }
}

async fn brave_search(query: &str, count: u32) -> Result<String, String> {
    let api_key = crate::executor::keychain::retrieve_api_key("brave")
        .map_err(|_| "No API key for Brave Search. Set it in Settings.".to_string())?;

    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", &api_key)
        .header("Accept", "application/json")
        .query(&[("q", query), ("count", &count.to_string())])
        .send()
        .await
        .map_err(|e| format!("Brave search request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Brave search returned {}: {}", status, body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Brave search response: {}", e))?;

    let mut results = Vec::new();

    if let Some(web_results) = json["web"].get("results").and_then(|r| r.as_array()) {
        for (i, item) in web_results.iter().enumerate() {
            let title = item["title"].as_str().unwrap_or("(no title)");
            let url = item["url"].as_str().unwrap_or("");
            let description = item["description"].as_str().unwrap_or("(no description)");
            results.push(format!(
                "{}. {}\n   {}\n   {}",
                i + 1,
                title,
                url,
                description
            ));
        }
    }

    if results.is_empty() {
        Ok("No results found.".to_string())
    } else {
        Ok(results.join("\n\n"))
    }
}

async fn tavily_search(query: &str, count: u32) -> Result<String, String> {
    let api_key = crate::executor::keychain::retrieve_api_key("tavily")
        .map_err(|_| "No API key for Tavily. Set it in Settings.".to_string())?;

    let client = reqwest::Client::new();
    let body = json!({
        "query": query,
        "max_results": count,
        "search_depth": "basic"
    });

    let resp = client
        .post("https://api.tavily.com/search")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Tavily search request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Tavily search returned {}: {}", status, body));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Tavily search response: {}", e))?;

    let mut results = Vec::new();

    if let Some(items) = json["results"].as_array() {
        for (i, item) in items.iter().enumerate() {
            let title = item["title"].as_str().unwrap_or("(no title)");
            let url = item["url"].as_str().unwrap_or("");
            let content = item["content"].as_str().unwrap_or("(no content)");
            results.push(format!("{}. {}\n   {}\n   {}", i + 1, title, url, content));
        }
    }

    if results.is_empty() {
        Ok("No results found.".to_string())
    } else {
        Ok(results.join("\n\n"))
    }
}

#[async_trait::async_trait]
impl ToolHandler for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search the web for information. Returns a list of results with titles, URLs, and descriptions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of results to return (default: 5, max: 10)"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String> {
        let query = input["query"]
            .as_str()
            .ok_or("web_search: missing 'query' field")?;
        let count = input["count"].as_u64().unwrap_or(5).min(10) as u32;

        tracing::info!(
            run_id = run_id,
            query = query,
            provider = %ctx.web_search_provider,
            "agent tool: web_search"
        );

        let result = execute_web_search(&ctx.web_search_provider, query, count).await?;
        Ok((result, false))
    }
}
