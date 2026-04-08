use chrono::Utc;
use serde::Serialize;
use serde_json::json;

use crate::executor::{
    image_gen::{self, generate_image, OPENAI_IMAGE_MODEL, OPENAI_IMAGE_PROVIDER},
    keychain,
};

use super::{context::ToolExecutionContext, helpers::validate_path, ToolHandler};

const DEFAULT_IMAGE_SIZE: &str = "1024x1024";

pub struct ImageGenerationTool;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageGenerationResult {
    status: String,
    provider: String,
    model: String,
    prompt: String,
    size: String,
    output_path: String,
    bytes_written: usize,
}

#[async_trait::async_trait]
impl ToolHandler for ImageGenerationTool {
    fn name(&self) -> &'static str {
        "image_generation"
    }

    fn definition(&self) -> crate::executor::llm_provider::ToolDefinition {
        crate::executor::llm_provider::ToolDefinition {
            name: self.name().to_string(),
            description: "Generate images from text prompts using an AI image generation model. Saves the result into the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Text description of the image to generate"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Where to save the generated image in workspace (default: generated_{timestamp}.png)"
                    },
                    "size": {
                        "type": "string",
                        "enum": ["1024x1024", "1024x1792", "1792x1024"],
                        "description": "Image dimensions (default: 1024x1024)"
                    }
                },
                "required": ["prompt"]
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
        let prompt = input["prompt"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("image_generation: missing 'prompt' field")?;
        let size = input["size"].as_str().unwrap_or(DEFAULT_IMAGE_SIZE);
        ensure_valid_size(size)?;

        let api_key = keychain::retrieve_api_key(OPENAI_IMAGE_PROVIDER).map_err(|_| {
            "image_generation: no API key configured for openai image generation".to_string()
        })?;

        let output_path = input["output_path"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(default_output_path);
        let workspace_root = ctx.workspace_root();
        let full_path = validate_path(&workspace_root, &output_path)?;

        let bytes = generate_image(prompt, size, &api_key).await?;
        std::fs::write(&full_path, &bytes)
            .map_err(|e| format!("image_generation: failed to write {}: {}", output_path, e))?;

        let result = serde_json::to_string_pretty(&ImageGenerationResult {
            status: "saved".to_string(),
            provider: image_gen::OPENAI_IMAGE_PROVIDER.to_string(),
            model: OPENAI_IMAGE_MODEL.to_string(),
            prompt: prompt.to_string(),
            size: size.to_string(),
            output_path,
            bytes_written: bytes.len(),
        })
        .map_err(|e| format!("image_generation: failed to serialize result: {}", e))?;

        Ok((result, false))
    }
}

fn ensure_valid_size(size: &str) -> Result<(), String> {
    match size {
        "1024x1024" | "1024x1792" | "1792x1024" => Ok(()),
        other => Err(format!(
            "image_generation: unsupported size '{}'; expected 1024x1024, 1024x1792, or 1792x1024",
            other
        )),
    }
}

fn default_output_path() -> String {
    format!("generated_{}.png", Utc::now().format("%Y%m%d_%H%M%S"))
}

#[cfg(test)]
mod tests {
    use super::{default_output_path, ensure_valid_size};

    #[test]
    fn accepts_known_sizes() {
        assert!(ensure_valid_size("1024x1024").is_ok());
        assert!(ensure_valid_size("1024x1792").is_ok());
        assert!(ensure_valid_size("1792x1024").is_ok());
        assert!(ensure_valid_size("512x512").is_err());
    }

    #[test]
    fn default_output_uses_png() {
        assert!(default_output_path().starts_with("generated_"));
        assert!(default_output_path().ends_with(".png"));
    }
}
