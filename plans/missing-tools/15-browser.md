# Plan: `browser` Tool

## Context
OpenClaw's `Browser` tool provides browser automation via a browser control server — navigate, click, type, screenshot, extract content. This enables agents to interact with web applications, test UIs, fill forms, scrape dynamic content, and validate deployments. This is the most complex missing tool.

## What It Does
Control a headless browser instance. Actions: `navigate`, `click`, `type`, `screenshot`, `get_content`, `evaluate` (run JS), `scroll`, `wait_for`. Returns screenshots as base64 images and page content as text/HTML.

## Backend Changes

### `Cargo.toml`
```toml
chromiumoxide = { version = "0.7", features = ["tokio-runtime"] }
```
Alternative: `headless_chrome` crate (simpler API, fewer features).

### New module: `src-tauri/src/executor/browser.rs`

```rust
use chromiumoxide::{Browser, BrowserConfig, Page};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct BrowserManager {
    browser: Option<Arc<Mutex<Browser>>>,
    pages: Arc<Mutex<HashMap<String, Page>>>,  // page_id -> Page
}

impl BrowserManager {
    pub async fn ensure_browser(&mut self) -> Result<(), String> {
        // Launch headless Chrome/Chromium if not running
    }
    
    pub async fn navigate(&self, url: &str) -> Result<PageInfo, String> { ... }
    pub async fn click(&self, page_id: &str, selector: &str) -> Result<(), String> { ... }
    pub async fn type_text(&self, page_id: &str, selector: &str, text: &str) -> Result<(), String> { ... }
    pub async fn screenshot(&self, page_id: &str) -> Result<String, String> { ... } // base64
    pub async fn get_content(&self, page_id: &str) -> Result<String, String> { ... }
    pub async fn evaluate(&self, page_id: &str, js: &str) -> Result<String, String> { ... }
    pub async fn scroll(&self, page_id: &str, direction: &str) -> Result<(), String> { ... }
    pub async fn wait_for(&self, page_id: &str, selector: &str, timeout_ms: u64) -> Result<(), String> { ... }
    pub async fn close_page(&self, page_id: &str) -> Result<(), String> { ... }
}
```

### New file: `src-tauri/src/executor/tools/browser.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod browser;` and `Box::new(browser::BrowserTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "browser".to_string(),
    description: "Control a headless browser. Navigate pages, click elements, type text, take screenshots, extract content, and run JavaScript. Use CSS selectors to target elements.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["navigate", "click", "type", "screenshot", "get_content", "evaluate", "scroll", "wait_for", "close"],
                "description": "Browser action to perform"
            },
            "url": {
                "type": "string",
                "description": "URL to navigate to (for 'navigate' action)"
            },
            "selector": {
                "type": "string",
                "description": "CSS selector for the target element (for click, type, wait_for)"
            },
            "text": {
                "type": "string",
                "description": "Text to type (for 'type' action)"
            },
            "script": {
                "type": "string",
                "description": "JavaScript to evaluate (for 'evaluate' action)"
            },
            "page_id": {
                "type": "string",
                "description": "Target page ID (from navigate result). Default: last active page."
            },
            "direction": {
                "type": "string",
                "enum": ["up", "down"],
                "description": "Scroll direction (for 'scroll' action)"
            },
            "timeout_ms": {
                "type": "integer",
                "description": "Timeout in milliseconds for wait_for (default: 5000)"
            }
        },
        "required": ["action"]
    }),
}
```

**Execution** (in `execute()` method):
```rust
"browser" => {
    let manager = app.state::<BrowserManager>();
    let action = input["action"].as_str().ok_or("browser: missing action")?;
    
    match action {
        "navigate" => {
            let url = input["url"].as_str().ok_or("browser: navigate requires url")?;
            let page_info = manager.navigate(url).await?;
            Ok((json!({"page_id": page_info.id, "title": page_info.title}).to_string(), false))
        }
        "screenshot" => {
            let page_id = input["page_id"].as_str();
            let base64_img = manager.screenshot(page_id.unwrap_or("default")).await?;
            // Return as image content block (or save to workspace)
            Ok((json!({"image": base64_img, "saved_to": "screenshot.png"}).to_string(), false))
        }
        // ... other actions
    }
}
```

### `src-tauri/src/executor/permissions.rs`
```rust
"browser" => {
    let action = input["action"].as_str().unwrap_or("unknown");
    match action {
        "screenshot" | "get_content" | "scroll" => (RiskLevel::AutoAllow, String::new()),
        "navigate" => {
            let url = input["url"].as_str().unwrap_or("<unknown>");
            (RiskLevel::Prompt, format!("Browser navigate: {}", truncate_for_display(url, 60)))
        }
        "evaluate" => (RiskLevel::PromptDangerous, "Execute JavaScript in browser".to_string()),
        "click" | "type" => (RiskLevel::Prompt, format!("Browser {}", action)),
        _ => (RiskLevel::Prompt, format!("Browser action: {}", action)),
    }
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { Monitor } from 'lucide-react';
browser: { Icon: Monitor, colorClass: 'text-blue-400' },
```

### `src/components/chat/ToolUseBlock.tsx`
Use the shared tool presentation foundation from plan [26-tool-use-ui](26-tool-use-ui.md). This tool should add browser-specific rendering for screenshot previews, page-state presentation, and action-specific result sections once the core browser actions are working.

### `src/screens/AgentInspector/ConfigTab.tsx`
Add new "Browser" category:
```ts
{
    label: 'Browser',
    tools: [{ id: 'browser', label: 'Browser Control' }],
},
```

## Permission Level
- `screenshot`, `get_content`, `scroll`: **AutoAllow** (read-only)
- `navigate`, `click`, `type`: **Prompt** (interacts with web pages)
- `evaluate`: **PromptDangerous** (runs arbitrary JavaScript)

## Dependencies
- `chromiumoxide` crate (requires Chrome/Chromium installed on system)
- Chromium binary: either bundled (increases app size ~100MB) or require system install
- Alternative: use `headless_chrome` crate for simpler integration

## Key Design Decisions
- **System Chrome vs. bundled**: Recommend requiring system Chrome installation initially, with optional bundled Chromium later
- **Page management**: Support multiple concurrent pages with page_id tracking
- **Screenshot format**: Save to workspace as PNG + return base64 for inline preview
- **SSRF protection**: Apply same URL validation as web_fetch for navigate
- **Resource limits**: Max 5 concurrent pages, auto-close idle pages after 5 minutes

## Verification
1. `browser { action: "navigate", url: "https://example.com" }` → confirm page loaded, returns page_id and title
2. `browser { action: "screenshot" }` → confirm screenshot PNG saved and preview shown
3. `browser { action: "get_content" }` → confirm page text content returned
4. `browser { action: "click", selector: "a" }` → confirm navigation
5. `browser { action: "evaluate", script: "document.title" }` → confirm JS result returned
6. Test permission prompts for navigate, click, evaluate
7. Test with invalid URL → error
8. Test without Chrome installed → clear error about missing dependency
