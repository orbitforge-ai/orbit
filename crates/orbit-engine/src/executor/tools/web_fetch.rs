use std::time::Duration;

use serde_json::json;

use crate::executor::{http::validate_url_for_ssrf, llm_provider::ToolDefinition};

use super::{context::ToolExecutionContext, ToolHandler};

pub struct WebFetchTool;

const MAX_WEB_FETCH_LEN: usize = 50_000;
const WEB_FETCH_TIMEOUT_SECS: u64 = 30;

#[async_trait::async_trait]
impl ToolHandler for WebFetchTool {
    fn name(&self) -> &'static str {
        "web_fetch"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Fetch readable content from a URL. HTML responses are converted to markdown by default; use raw for APIs or exact body access.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    },
                    "raw": {
                        "type": "boolean",
                        "description": "If true, return the raw response body instead of markdown extraction."
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Maximum characters to return. Defaults to 50000."
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(
        &self,
        _ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let url = input["url"]
            .as_str()
            .ok_or("web_fetch: missing 'url' field")?;
        let raw = input["raw"].as_bool().unwrap_or(false);
        let max_length = input["max_length"]
            .as_u64()
            .unwrap_or(MAX_WEB_FETCH_LEN as u64)
            .min(MAX_WEB_FETCH_LEN as u64) as usize;

        validate_url_for_ssrf(url).await?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(WEB_FETCH_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| format!("web_fetch: failed to create HTTP client: {}", e))?;

        let response = client
            .get(url)
            .header(
                reqwest::header::USER_AGENT,
                "Orbit/0.1 (+https://github.com/orbitforge-ai/orbit)",
            )
            .send()
            .await
            .map_err(|e| format!("web_fetch: request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let error = truncate_text(&body, 2_000);
            return Err(if error.is_empty() {
                format!("web_fetch: {} returned {}", url, status)
            } else {
                format!("web_fetch: {} returned {}\n{}", url, status, error)
            });
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        let body = response
            .bytes()
            .await
            .map_err(|e| format!("web_fetch: failed to read response body: {}", e))?;
        let body = String::from_utf8_lossy(&body).into_owned();

        let rendered = if raw || !is_html_content_type(&content_type) {
            body
        } else {
            html2md::parse_html(&body)
        };

        Ok((truncate_text(&rendered, max_length), false))
    }
}

fn is_html_content_type(content_type: &str) -> bool {
    content_type.contains("text/html") || content_type.contains("application/xhtml+xml")
}

fn truncate_text(content: &str, max_length: usize) -> String {
    if content.chars().count() <= max_length {
        return content.to_string();
    }

    let truncated: String = content.chars().take(max_length).collect();
    format!(
        "{}\n[content truncated at {} characters]",
        truncated, max_length
    )
}

#[cfg(test)]
mod tests {
    use super::{is_html_content_type, truncate_text};

    #[test]
    fn detects_html_content_types() {
        assert!(is_html_content_type("text/html; charset=utf-8"));
        assert!(is_html_content_type("application/xhtml+xml"));
        assert!(!is_html_content_type("application/json"));
    }

    #[test]
    fn truncates_long_content() {
        let truncated = truncate_text("abcdef", 3);
        assert!(truncated.starts_with("abc"));
        assert!(truncated.contains("[content truncated at 3 characters]"));
    }
}
