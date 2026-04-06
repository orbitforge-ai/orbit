# Plan: `image_generation` Tool

## Context
OpenClaw's `Image Generation` tool lets agents generate or edit images using AI models. This enables agents to create diagrams, mockups, illustrations, or edit reference images — useful for design, documentation, and creative tasks.

Orbit does not currently have any image-generation provider integration, image-specific API key storage, or related settings UI. This plan therefore includes a provider/settings expansion in addition to the tool itself.

## What It Does
Generate new images from text prompts, and later support editing existing images with instructions. Uses a configurable image-generation backend, with OpenAI's Images API (`gpt-image-1`) as the initial provider. Saves results to the agent's workspace.

To keep the first implementation tractable, v1 should be prompt-to-image generation only. Reference-image editing can follow once the provider plumbing and UX are proven out.

## Backend Changes

### New file: `src-tauri/src/executor/tools/image_generation.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod image_generation;` and `Box::new(image_generation::ImageGenerationTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "image_generation".to_string(),
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
```

**Execution** (in `execute()` method):
1. Resolve the configured image-generation provider and credentials
2. Build request to the provider API
3. Receive image bytes or base64 image data in response
4. Save to workspace at `output_path` (or auto-generated name)
5. Return path and preview info

### New helper: `src-tauri/src/executor/image_gen.rs`
```rust
pub async fn generate_image(
    prompt: &str,
    size: &str,
    api_key: &str,
) -> Result<Vec<u8>, String> {
    // POST to the configured image provider.
    // Initial implementation: OpenAI Images API (gpt-image-1).
}
```

### Provider/settings expansion

Orbit currently exposes only `anthropic` and `minimax` as chat providers. Image generation should not overload the chat-provider setting. Instead, add a dedicated image-generation provider configuration path:

- store image API credentials separately from chat credentials
- add image generation provider/model settings in app settings
- keep the agent's chat provider/model unchanged

This can start with a single provider:

- `provider = "openai"`
- `model = "gpt-image-1"`

Future providers can fit behind the same helper interface.

### `src-tauri/src/executor/keychain.rs`
Add image-provider credential storage (initially OpenAI).

### `src-tauri/src/executor/permissions.rs`
```rust
"image_generation" => {
    (RiskLevel::Prompt, "Generate image (uses API credits)".to_string())
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { ImagePlus } from 'lucide-react';
image_generation: { Icon: ImagePlus, colorClass: 'text-purple-400' },
```

### `src/components/chat/ToolUseBlock.tsx`
Use the shared tool presentation foundation from plan [26-tool-use-ui](26-tool-use-ui.md). This tool should add its own formatter so saved-image metadata renders as an inline preview thumbnail / image result card instead of raw payload text.

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Vision" category:
```ts
{ id: 'image_generation', label: 'Image Generation' },
```

### `src/screens/Settings/index.tsx`
Add image generation provider settings and API key field.

## Permission Level
- **Prompt** — costs money (API credits) and creates files

## Dependencies
- Image provider credentials (stored in keychain)
- `reqwest` for API calls (already available)
- `base64` for image encoding

## Key Design Decisions
- Initial provider: OpenAI Images API with `gpt-image-1`
- Keep image-generation provider config separate from the agent's chat LLM provider
- Start with generate-only; add image editing in a follow-up pass
- Keep the v1 schema narrow so the model does not attempt unsupported edit flows
- Could later add other providers via the same helper abstraction
- Images saved to workspace so other tools (read_file, image_analysis) can access them
- Prompt permission since it costs API credits

## Verification
1. `image_generation { prompt: "A simple red circle on white background" }` → confirm image saved to workspace
2. Verify image is valid PNG and viewable
3. Test with custom `output_path` → confirm saved at specified location
4. Test without image-provider API key → clear error message about missing key
5. Confirm permission prompt appears
6. Defer `reference_image` editing tests until the follow-up editing phase lands
