# Plan: `lsp` Tool — Language Server Protocol Integration

> **Source**: Claude Code's `LSP` tool. Not present in OpenClaw.
> **Approach**: New tool — code intelligence is a fundamentally new capability.

## Context

Claude Code's LSP tool provides deep code intelligence: go-to-definition, find-references, hover info, document symbols, call hierarchy. This transforms agents from text-pattern-matchers into code-aware navigators. For Orbit, this means agents can understand code structure, trace dependencies, and navigate large codebases with precision.

## What It Does

Interact with Language Server Protocol servers for code intelligence. Supports: `goToDefinition`, `findReferences`, `hover`, `documentSymbol`, `workspaceSymbol`, `goToImplementation`. Requires a running LSP server for the relevant language.

## Backend Changes

### `Cargo.toml`

```toml
lsp-types = "0.97"
tower-lsp = "0.20"  # or use raw JSON-RPC over stdio
```

### New module: `src-tauri/src/executor/lsp_client.rs`

Manages LSP server lifecycle per language:

```rust
pub struct LspManager {
    servers: HashMap<String, LspServer>,  // language -> server
}

pub struct LspServer {
    pub language: String,
    pub process: tokio::process::Child,
    pub stdin: tokio::io::BufWriter<ChildStdin>,
    pub pending_requests: HashMap<i64, oneshot::Sender<Value>>,
}

impl LspManager {
    pub async fn ensure_server(&mut self, language: &str, workspace: &Path) -> Result<(), String>;
    pub async fn request(&self, language: &str, method: &str, params: Value) -> Result<Value, String>;
    pub async fn shutdown_all(&mut self);
}
```

**LSP server discovery**: Check common paths for language servers:
- `rust-analyzer` for Rust
- `typescript-language-server` for TS/JS
- `pyright` / `pylsp` for Python
- `gopls` for Go

### New file: `src-tauri/src/executor/tools/lsp.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod lsp;` and `Box::new(lsp::LspTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):

```rust
ToolDefinition {
    name: "lsp".to_string(),
    description: "Code intelligence via Language Server Protocol. Go to definitions, find references, get hover info, list symbols. Requires a language server installed for the target language.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["goToDefinition", "findReferences", "hover",
                         "documentSymbol", "workspaceSymbol", "goToImplementation"],
                "description": "LSP operation to perform"
            },
            "file_path": {
                "type": "string",
                "description": "Relative path to the file (for file-scoped operations)"
            },
            "line": {
                "type": "integer",
                "description": "Line number (1-based)"
            },
            "character": {
                "type": "integer",
                "description": "Column number (1-based)"
            },
            "query": {
                "type": "string",
                "description": "Search query (for workspaceSymbol)"
            }
        },
        "required": ["action"]
    }),
}
```

### `src-tauri/src/executor/permissions.rs`

```rust
"lsp" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`

```ts
import { Code2 } from 'lucide-react';
lsp: { Icon: Code2, colorClass: 'text-purple-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`

Add new "Code Intelligence" category:

```ts
{
    label: 'Code Intelligence',
    tools: [{ id: 'lsp', label: 'LSP (Code Nav)' }],
},
```

## Permission Level

**AutoAllow** — read-only code analysis, no side effects.

## Dependencies

- `lsp-types` crate for LSP protocol types
- Language servers must be installed on the system (not bundled)
- `tokio` for async stdio communication

## Key Design Decisions

- **Language auto-detection**: Infer language from file extension, auto-start the right server
- **Server lifecycle**: Start on first use, keep alive for session duration, shutdown on agent termination
- **Graceful fallback**: If no LSP server is available, return clear error with install instructions
- **Workspace initialization**: Send `initialize` + `initialized` on first connect, with the agent's workspace as root

## Verification

1. Open a Rust file, `lsp { action: "documentSymbol", file_path: "src/main.rs" }` -> list all symbols
2. `lsp { action: "goToDefinition", file_path: "src/main.rs", line: 10, character: 5 }` -> jump to definition
3. `lsp { action: "findReferences", ... }` -> find all usages
4. `lsp { action: "hover", ... }` -> get type info and docs
5. `lsp { action: "workspaceSymbol", query: "execute_tool" }` -> find across workspace
6. Test without language server installed -> clear error with install suggestion