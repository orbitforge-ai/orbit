# Plan: `web_fetch` Tool

## Context
Orbit has `web_search` (returns search result snippets) but no way for agents to fetch and read the actual content of a URL. OpenClaw's `Web Fetch` tool extracts readable content from URLs as markdown/text. This is essential for agents that need to read documentation, APIs, or web pages.

## What It Does
Fetch a URL and extract readable content as markdown/text. Handles HTML-to-markdown conversion, respects timeouts, and enforces size limits. Distinct from `web_search` which queries search engines.

## Backend Changes

### `Cargo.toml`
Add dependency:
```toml
html2md = "0.2"  # HTML to Markdown conversion
```
The existing `reqwest` dependency (already used for web_search/memory) handles HTTP fetching.

### New file: `src-tauri/src/executor/tools/web_fetch.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod web_fetch;` and `Box::new(web_fetch::WebFetchTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "web_fetch".to_string(),
    description: "Fetch and extract readable content from a URL as markdown/text. Use for reading web pages, documentation, or API responses. Returns cleaned content, not raw HTML.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "The URL to fetch"
            },
            "raw": {
                "type": "boolean",
                "description": "If true, return raw response body instead of extracted markdown. Useful for JSON APIs. Default: false."
            },
            "max_length": {
                "type": "integer",
                "description": "Maximum characters to return (default: 50000). Content is truncated beyond this."
            }
        },
        "required": ["url"]
    }),
}
```

**Execution** (in `execute()` method):
1. Validate URL using the existing SSRF logic from `src-tauri/src/executor/http.rs`
2. Fetch with reqwest (30s timeout, follow redirects, user-agent header)
3. If `raw` mode or non-HTML content-type: return body as-is (truncated)
4. If HTML: convert to markdown using `html2md::parse_html()`
5. Truncate to `max_length` (default 50,000 chars)

Because the current SSRF validator in `http.rs` is private to that module, this plan should first extract it into a shared helper (for example `executor/net.rs` or a public helper in `http.rs`) rather than duplicating the logic.

**Constants:**
```rust
const MAX_WEB_FETCH_LEN: usize = 50_000;
const WEB_FETCH_TIMEOUT_SECS: u64 = 30;
```

### `src-tauri/src/executor/permissions.rs`
```rust
"web_fetch" => {
    let url = input["url"].as_str().unwrap_or("<unknown>");
    (RiskLevel::AutoAllow, String::new())
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { Link } from 'lucide-react';
web_fetch: { Icon: Link, colorClass: 'text-blue-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to "Communication" category:
```ts
{ id: 'web_fetch', label: 'Web Fetch' },
```

## Permission Level
- **AutoAllow** in normal/strict modes (read-only, no side effects — same rationale as web_search)
- SSRF validation prevents fetching internal/private IPs

## Dependencies
- `html2md` crate (lightweight, no native deps)
- Existing `reqwest` client
- Existing SSRF validation logic in `http.rs`, after extracting it into a shared helper

## Verification
1. Fetch a public URL (e.g., a GitHub README raw URL) → confirm markdown returned
2. Fetch a JSON API endpoint with `raw: true` → confirm raw JSON returned
3. Test `max_length` truncation
4. Test invalid URL → error message
5. Test SSRF protection (localhost, private IPs) → should be blocked
