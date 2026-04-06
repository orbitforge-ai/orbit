# Plan: Refactor `agent_tools.rs` into Per-Tool Modules

## Context

`agent_tools.rs` is 1,613 lines and growing. Every new tool adds ~50-100 lines of definition + ~50-200 lines of execution logic to this single file. With 15 current tools and 20+ planned, this file would balloon past 4,000 lines. Splitting each tool into its own module improves navigability, reduces merge conflicts, and makes it trivial to add new tools.

## Current Structure (single file)

```
src-tauri/src/executor/agent_tools.rs (1,613 lines)
├── ToolExecutionContext struct + constructors (lines 1-155)
├── build_tool_definitions() — all 15 tool schemas (lines 158-590)
├── execute_tool() — giant match with all 15 arms (lines 592-1506)
├── execute_shell_command() helper (lines 510-590)
├── validate_path() helper (lines ~460-490)
└── Web search providers: brave_search(), tavily_search() (lines 1508-1613)
```

## Target Structure (per-tool modules)

```
src-tauri/src/executor/
├── agent_tools.rs          ← slim orchestrator (~150 lines)
└── tools/
    ├── mod.rs              ← re-exports ToolHandler trait + all tools
    ├── context.rs          ← ToolExecutionContext (moved from agent_tools.rs)
    ├── helpers.rs          ← validate_path(), format helpers
    ├── shell_command.rs
    ├── read_file.rs
    ├── write_file.rs
    ├── list_files.rs
    ├── web_search.rs       ← includes brave_search(), tavily_search()
    ├── send_message.rs
    ├── spawn_sub_agents.rs
    ├── activate_skill.rs
    ├── remember.rs
    ├── forget.rs
    ├── search_memory.rs
    ├── list_memories.rs
    ├── react_to_message.rs
    └── finish.rs
```

## Design

### The `ToolHandler` trait

Each tool implements a simple trait:

```rust
// src-tauri/src/executor/tools/mod.rs

use super::agent_tools::ToolExecutionContext;
use crate::executor::llm_provider::ToolDefinition;

/// Every tool implements this trait.
pub trait ToolHandler {
    /// Tool name as it appears in the LLM tool_use block.
    fn name(&self) -> &'static str;

    /// JSON Schema definition exposed to the LLM.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool. Returns (result_text, is_finish).
    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String>;
}
```

> **Note**: Rust doesn't support `async fn` in traits without `async-trait` or RPITIT (Rust 1.75+). Since Orbit targets modern Rust, use `#[allow(async_fn_in_trait)]` or the `async-trait` crate (already common in Tauri projects).

### Per-tool module example

```rust
// src-tauri/src/executor/tools/read_file.rs

use serde_json::json;
use crate::executor::llm_provider::ToolDefinition;
use super::{ToolHandler, context::ToolExecutionContext, helpers::validate_path};

pub struct ReadFileTool;

impl ToolHandler for ReadFileTool {
    fn name(&self) -> &'static str { "read_file" }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a file from the agent's workspace...".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file within the workspace"
                    }
                },
                "required": ["path"]
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
        let path = input["path"].as_str().ok_or("read_file: missing 'path'")?;
        let full_path = validate_path(&ctx.workspace_root, path)?;
        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;
        let content = if content.len() > 100_000 {
            let mut t = content[..100_000].to_string();
            t.push_str("\n[file truncated at 100KB]");
            t
        } else {
            content
        };
        Ok((content, false))
    }
}
```

### Slim orchestrator (`agent_tools.rs`)

```rust
// src-tauri/src/executor/agent_tools.rs — reduced to ~150 lines

mod tools;

pub use tools::context::ToolExecutionContext;
use tools::*;
use crate::executor::llm_provider::ToolDefinition;

/// All registered tool handlers.
fn all_tools() -> Vec<Box<dyn ToolHandler + Send + Sync>> {
    vec![
        Box::new(shell_command::ShellCommandTool),
        Box::new(read_file::ReadFileTool),
        Box::new(write_file::WriteFileTool),
        Box::new(list_files::ListFilesTool),
        Box::new(web_search::WebSearchTool),
        Box::new(send_message::SendMessageTool),
        Box::new(spawn_sub_agents::SpawnSubAgentsTool),
        Box::new(activate_skill::ActivateSkillTool),
        Box::new(remember::RememberTool),
        Box::new(forget::ForgetTool),
        Box::new(search_memory::SearchMemoryTool),
        Box::new(list_memories::ListMemoriesTool),
        Box::new(react_to_message::ReactToMessageTool),
        Box::new(finish::FinishTool),
    ]
}

/// Build tool definitions filtered by allowed list.
pub fn build_tool_definitions(allowed: &[String]) -> Vec<ToolDefinition> {
    let tools = all_tools();
    let mut defs: Vec<ToolDefinition> = if allowed.is_empty() {
        tools.iter().map(|t| t.definition()).collect()
    } else {
        tools.iter()
            .filter(|t| allowed.contains(&t.name().to_string()))
            .map(|t| t.definition())
            .collect()
    };
    // react_to_message always included
    if !defs.iter().any(|d| d.name == "react_to_message") {
        defs.push(react_to_message::ReactToMessageTool.definition());
    }
    defs
}

/// Dispatch a tool call by name.
pub async fn execute_tool(
    ctx: &ToolExecutionContext,
    tool_name: &str,
    input: &serde_json::Value,
    app: &tauri::AppHandle,
    run_id: &str,
) -> Result<(String, bool), String> {
    let tools = all_tools();
    for tool in &tools {
        if tool.name() == tool_name {
            return tool.execute(ctx, input, app, run_id).await;
        }
    }
    Err(format!("unknown tool: {}", tool_name))
}
```

### `tools/mod.rs`

```rust
pub mod context;
pub mod helpers;

pub mod shell_command;
pub mod read_file;
pub mod write_file;
pub mod list_files;
pub mod web_search;
pub mod send_message;
pub mod spawn_sub_agents;
pub mod activate_skill;
pub mod remember;
pub mod forget;
pub mod search_memory;
pub mod list_memories;
pub mod react_to_message;
pub mod finish;

/// Trait that all tools implement.
pub trait ToolHandler {
    fn name(&self) -> &'static str;
    fn definition(&self) -> crate::executor::llm_provider::ToolDefinition;
    async fn execute(
        &self,
        ctx: &context::ToolExecutionContext,
        input: &serde_json::Value,
        app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String>;
}
```

## Files to Modify

| File | Change |
|------|--------|
| `src-tauri/src/executor/agent_tools.rs` | Gut to slim orchestrator (~150 lines) |
| `src-tauri/src/executor/tools/` (new dir) | 16 new files + mod.rs + context.rs + helpers.rs |
| `src-tauri/src/executor/mod.rs` | Add `pub mod tools;` if needed (or keep tools as submodule of agent_tools) |
| `src-tauri/src/executor/permissions.rs` | Update import path: `agent_tools::ToolExecutionContext` -> unchanged (re-exported) |
| `src-tauri/src/executor/agent_loop.rs` | No change — imports `ToolExecutionContext` from `agent_tools` (still re-exported) |
| `src-tauri/src/executor/session_agent.rs` | No change — same re-export |
| `src-tauri/src/executor/context.rs` | No change — calls `agent_tools::build_tool_definitions()` (unchanged API) |

## Migration Steps

1. Create `src-tauri/src/executor/tools/` directory
2. Create `tools/mod.rs` with `ToolHandler` trait
3. Move `ToolExecutionContext` to `tools/context.rs`, re-export from `agent_tools.rs`
4. Move `validate_path()` and helpers to `tools/helpers.rs`
5. Extract each tool's definition + execution into its own file (one at a time)
6. Update `agent_tools.rs` to use the registry pattern
7. `cargo build` after each tool extraction to catch compilation errors incrementally
8. Run existing tests to confirm no regressions

## Key Constraints

- **Public API unchanged**: `agent_tools::ToolExecutionContext`, `agent_tools::build_tool_definitions()`, and `agent_tools::execute_tool()` keep the same signatures. External callers (permissions.rs, context.rs, agent_loop.rs, session_agent.rs) don't change.
- **No behavior changes**: Pure refactor — every tool does exactly what it did before.
- **Constants stay accessible**: `MAX_CHAIN_DEPTH`, `MAX_SUB_AGENTS`, etc. move to `tools/helpers.rs` or stay in the relevant tool module.

## Why This Should Be Done First

This refactor should be completed **before** implementing any new tools from plans 01-24. Each new tool becomes a single new file in `tools/` + one line in `all_tools()`. Without this refactor, every new tool bloats `agent_tools.rs` further.

## Verification

1. `cargo build` — compiles without errors
2. `cargo test` — all existing tests pass
3. Run an agent with shell_command, read_file, web_search, send_message, spawn_sub_agents — confirm all tools work identically
4. Check that permission prompts still fire correctly
5. Verify tool definitions in frontend Config tab are unchanged