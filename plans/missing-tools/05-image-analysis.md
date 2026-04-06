# Plan: `image_analysis` Tool

## Context
OpenClaw's `Image` tool lets agents analyze images using a vision-capable LLM. Orbit already supports `Image` content blocks in its LLM provider (`ContentBlock::Image`), but there's no agent tool to trigger image analysis on-demand. Agents should be able to analyze screenshots, diagrams, charts, or any image file.

The key gap is not raw image transport; it is a provider-aware one-shot vision call. This tool should reuse Orbit's existing provider abstraction and only run when the current agent provider/model supports image input.

## What It Does
Analyze one or more images using the agent's configured vision-capable model. Accept image paths (from workspace) or URLs, plus an optional prompt/question. Returns the model's analysis as text.

## Backend Changes

### New file: `src-tauri/src/executor/tools/image_analysis.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod image_analysis;` and `Box::new(image_analysis::ImageAnalysisTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "image_analysis".to_string(),
    description: "Analyze images using a vision-capable AI model. Provide image paths from the workspace or URLs, plus an optional question or instruction about what to analyze.".to_string(),
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
                "description": "What to analyze or look for in the image(s). Default: 'Describe this image in detail.'"
            }
        },
        "required": ["images"]
    }),
}
```

**Execution** (in `execute()` method):
1. For each image path: read file, base64-encode, detect media type from extension
2. For each image URL: fetch with reqwest after SSRF validation, then base64-encode
3. Build a messages payload with `ContentBlock::Image` blocks + text prompt
4. Route the request through a provider-agnostic helper built on `llm_provider.rs`, using the agent's configured provider/model
5. Return the text response

```rust
"image_analysis" => {
    let images = input["images"].as_array().ok_or("image_analysis: missing 'images' array")?;
    let prompt = input["prompt"].as_str().unwrap_or("Describe this image in detail.");
    
    let mut content_blocks = Vec::new();
    for img in images {
        let img_str = img.as_str().ok_or("image_analysis: each image must be a string")?;
        if img_str.starts_with("http://") || img_str.starts_with("https://") {
            // Fetch URL, base64 encode
            let bytes = fetch_image_url(img_str).await?;
            let media_type = guess_media_type_from_bytes(&bytes);
            content_blocks.push(ContentBlock::Image {
                media_type, data: base64::encode(&bytes)
            });
        } else {
            // Read from workspace
            let full_path = validate_path(&ctx.workspace_root, img_str)?;
            let bytes = std::fs::read(&full_path)
                .map_err(|e| format!("failed to read image: {}", e))?;
            let media_type = guess_media_type(img_str);
            content_blocks.push(ContentBlock::Image {
                media_type, data: base64::encode(&bytes)
            });
        }
    }
    content_blocks.push(ContentBlock::Text { text: prompt.to_string() });
    
    // Make a one-shot provider-aware LLM call with vision
    let response = call_vision_model(&content_blocks, app).await?;
    Ok((response, false))
}
```

**Helper functions needed:**
- `guess_media_type(path: &str) -> String` — map `.png`→`image/png`, `.jpg`→`image/jpeg`, etc.
- `fetch_image_url(url: &str) -> Result<Vec<u8>, String>` — download with SSRF check
- `call_vision_model(...) -> Result<String, String>` — make a single provider-aware call using the current agent config, and fail clearly when the configured provider/model does not support image input

This likely belongs in a shared helper alongside other one-shot provider invocations, rather than hard-coding Anthropic-specific request logic inside the tool.

### `src-tauri/src/executor/permissions.rs`
```rust
"image_analysis" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { Eye } from 'lucide-react';
image_analysis: { Icon: Eye, colorClass: 'text-purple-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add new "Vision" category:
```ts
{
    label: 'Vision',
    tools: [
        { id: 'image_analysis', label: 'Image Analysis' },
    ],
},
```

## Permission Level
- **AutoAllow** — read-only analysis, no side effects

## Dependencies
- `base64` crate (likely already available or use `base64` encoding from existing code)
- Existing `reqwest` for URL fetching
- Existing provider abstraction in `llm_provider.rs`
- Shared SSRF validator extracted from `http.rs`
- Configured provider/model must support vision; otherwise return a clear runtime error

## Key Design Decisions
- Uses the agent's own configured LLM provider for the vision call (piggybacks on existing API key)
- Image size limit: start with a conservative 5MB per image cap
- Supports common formats: PNG, JPEG, GIF, WebP
- Do not expose this as "Anthropic-only" in the tool contract; capability should follow provider/model support

## Verification
1. Place a test image in workspace → `image_analysis { images: ["test.png"], prompt: "What is in this image?" }` → confirm description returned
2. Test with URL → confirm remote image fetched and analyzed
3. Test with multiple images → confirm all analyzed
4. Test with non-image file → should return appropriate error
5. Test with image > 5MB → should return size limit error
