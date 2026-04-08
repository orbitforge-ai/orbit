use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;

pub const OPENAI_IMAGE_PROVIDER: &str = "openai";
pub const OPENAI_IMAGE_MODEL: &str = "gpt-image-1";

pub async fn generate_image(prompt: &str, size: &str, api_key: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/images/generations")
        .bearer_auth(api_key)
        .json(&json!({
            "model": OPENAI_IMAGE_MODEL,
            "prompt": prompt,
            "size": size,
        }))
        .send()
        .await
        .map_err(|e| format!("image_generation: request failed: {}", e))?;

    let status = response.status();
    let value = response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("image_generation: failed to parse response: {}", e))?;

    if !status.is_success() {
        let message = value["error"]["message"]
            .as_str()
            .unwrap_or("unknown OpenAI image API error");
        return Err(format!(
            "image_generation: OpenAI image API returned {}: {}",
            status, message
        ));
    }

    let b64 = value["data"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["b64_json"].as_str())
        .ok_or("image_generation: response did not include image data")?;

    STANDARD
        .decode(b64)
        .map_err(|e| format!("image_generation: failed to decode image bytes: {}", e))
}

#[cfg(test)]
mod tests {
    use super::OPENAI_IMAGE_MODEL;

    #[test]
    fn model_constant_is_stable() {
        assert_eq!(OPENAI_IMAGE_MODEL, "gpt-image-1");
    }
}
