use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;

use crate::executor::{
    http::validate_url_for_ssrf,
    keychain,
    llm_provider::{
        self, extract_text_response, model_supports_images, ChatMessage, ContentBlock, LlmConfig,
        ToolDefinition,
    },
    workspace,
};

use super::{context::ToolExecutionContext, helpers::validate_path, ToolHandler};

const DEFAULT_PROMPT: &str = "Describe this image in detail.";
const MAX_IMAGES: usize = 8;
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const IMAGE_FETCH_TIMEOUT_SECS: u64 = 30;
const IMAGE_ANALYSIS_MAX_TOKENS: u32 = 1_500;

pub struct ImageAnalysisTool;

#[async_trait::async_trait]
impl ToolHandler for ImageAnalysisTool {
    fn name(&self) -> &'static str {
        "image_analysis"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Analyze images using the agent's configured vision-capable AI model. Accepts workspace image paths or URLs plus an optional prompt.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "images": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of image paths (relative to workspace) or URLs to analyze"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "What to analyze or look for in the image(s). Defaults to 'Describe this image in detail.'"
                    }
                },
                "required": ["images"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let images = input["images"]
            .as_array()
            .ok_or("image_analysis: missing 'images' array")?;
        if images.is_empty() {
            return Err("image_analysis: provide at least one image".to_string());
        }
        if images.len() > MAX_IMAGES {
            return Err(format!(
                "image_analysis: at most {} images are supported per call",
                MAX_IMAGES
            ));
        }

        let prompt = input["prompt"].as_str().unwrap_or(DEFAULT_PROMPT);
        let ws_config = workspace::load_agent_config(&ctx.agent_id).unwrap_or_default();
        if !model_supports_images(&ws_config.provider, &ws_config.model) {
            return Err(format!(
                "image_analysis: the configured model '{}' for provider '{}' is not marked as vision-capable",
                ws_config.model, ws_config.provider
            ));
        }

        let api_key = keychain::retrieve_api_key(&ws_config.provider).map_err(|_| {
            format!(
                "image_analysis: no API key configured for {}",
                ws_config.provider
            )
        })?;
        let provider = llm_provider::create_provider(&ws_config.provider, api_key)?;

        let mut content = Vec::with_capacity(images.len() + 1);
        for image in images {
            let image_ref = image
                .as_str()
                .ok_or("image_analysis: each image must be a string")?;
            let asset = if is_http_url(image_ref) {
                fetch_remote_image(image_ref).await?
            } else {
                load_workspace_image(&ctx.workspace_root(), image_ref)?
            };
            content.push(ContentBlock::Image {
                media_type: asset.media_type,
                data: STANDARD.encode(asset.bytes),
            });
        }
        content.push(ContentBlock::Text {
            text: prompt.to_string(),
        });

        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content,
            created_at: None,
        }];
        let config = LlmConfig {
            model: ws_config.model.clone(),
            max_tokens: IMAGE_ANALYSIS_MAX_TOKENS,
            temperature: Some(0.2),
            system_prompt: "You are a careful visual analysis assistant. Answer the user's question about the provided image or images directly, and call out uncertainty when details are unclear.".to_string(),
        };

        let response = provider.chat_complete(&config, &messages, &[]).await?;
        let text = extract_text_response(&response)
            .map_err(|reason| format!("image_analysis: {}", reason))?;
        Ok((text, false))
    }
}

struct ImageAsset {
    media_type: String,
    bytes: Vec<u8>,
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn load_workspace_image(
    workspace_root: &std::path::Path,
    path: &str,
) -> Result<ImageAsset, String> {
    let full_path = validate_path(workspace_root, path)?;
    if !full_path.is_file() {
        return Err(format!("image_analysis: '{}' is not a file", path));
    }

    let bytes = std::fs::read(&full_path)
        .map_err(|e| format!("image_analysis: failed to read {}: {}", path, e))?;
    validate_image_size(path, bytes.len())?;
    let media_type = guess_media_type(path, &bytes)
        .ok_or_else(|| format!("image_analysis: '{}' is not a supported image format", path))?;

    Ok(ImageAsset { media_type, bytes })
}

async fn fetch_remote_image(url: &str) -> Result<ImageAsset, String> {
    validate_url_for_ssrf(url).await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(IMAGE_FETCH_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("image_analysis: failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "Orbit/0.1 (+https://github.com/orbitforge-ai/orbit)",
        )
        .send()
        .await
        .map_err(|e| format!("image_analysis: request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("image_analysis: {} returned HTTP {}", url, status));
    }

    if let Some(length) = response.content_length() {
        validate_image_size(url, length as usize)?;
    }

    let header_media_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_media_type);

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("image_analysis: failed to read response body: {}", e))?
        .to_vec();
    validate_image_size(url, bytes.len())?;
    let media_type = header_media_type
        .or_else(|| guess_media_type(url, &bytes))
        .ok_or_else(|| {
            format!(
                "image_analysis: '{}' did not resolve to a supported image",
                url
            )
        })?;

    Ok(ImageAsset { media_type, bytes })
}

fn validate_image_size(label: &str, size: usize) -> Result<(), String> {
    if size > MAX_IMAGE_BYTES {
        Err(format!(
            "image_analysis: '{}' exceeds the 5MB per-image limit",
            label
        ))
    } else {
        Ok(())
    }
}

fn guess_media_type(label: &str, bytes: &[u8]) -> Option<String> {
    guess_media_type_from_extension(label).or_else(|| guess_media_type_from_bytes(bytes))
}

fn guess_media_type_from_extension(label: &str) -> Option<String> {
    let lower = label.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some("image/png".to_string())
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg".to_string())
    } else if lower.ends_with(".gif") {
        Some("image/gif".to_string())
    } else if lower.ends_with(".webp") {
        Some("image/webp".to_string())
    } else {
        None
    }
}

fn guess_media_type_from_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png".to_string())
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg".to_string())
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif".to_string())
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp".to_string())
    } else {
        None
    }
}

fn normalize_media_type(value: &str) -> Option<String> {
    let base = value.split(';').next()?.trim().to_ascii_lowercase();
    match base.as_str() {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" => Some(base),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        guess_media_type_from_bytes, guess_media_type_from_extension, normalize_media_type,
        validate_image_size,
    };

    #[test]
    fn detects_image_formats() {
        assert_eq!(
            guess_media_type_from_extension("chart.png").as_deref(),
            Some("image/png")
        );
        assert_eq!(
            guess_media_type_from_bytes(&[0xFF, 0xD8, 0xFF, 0xDB]).as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            normalize_media_type("image/webp; charset=binary").as_deref(),
            Some("image/webp")
        );
    }

    #[test]
    fn rejects_oversized_images() {
        assert!(validate_image_size("big.png", 5 * 1024 * 1024 + 1).is_err());
        assert!(validate_image_size("small.png", 1024).is_ok());
    }
}
