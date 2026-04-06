# Plan: `grep` Tool — Content Search

> **Source**: Claude Code's `Grep` tool (built on ripgrep). Not present in OpenClaw.
> **Approach**: New tool — `list_files` expansion isn't sufficient since this searches file *contents*, not names.

## Context

Agents working with codebases need to search file contents — find function definitions, trace imports, locate error messages. Orbit agents currently must use `shell_command` with `grep` or `rg`, but a dedicated tool is safer (no command injection risk), more efficient (structured output), and works within the sandbox.

## What It Does

Search file contents using regex patterns. Supports filtering by file type/glob, context lines, and multiple output modes (matching lines, file paths only, match counts). Built on Rust's `regex` and `walkdir` crates for native performance.

## Backend Changes

### `Cargo.toml`

```toml
walkdir = "2"
regex = "1"  # likely already present
```

### New file: `src-tauri/src/executor/tools/grep.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod grep;` and `Box::new(grep::GrepTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):

```rust
ToolDefinition {
    name: "grep".to_string(),
    description: "Search file contents using regex patterns. Returns matching lines with file paths and line numbers. Use for finding function definitions, tracing imports, locating strings.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Regex pattern to search for"
            },
            "path": {
                "type": "string",
                "description": "Directory or file to search in (relative to workspace). Default: workspace root."
            },
            "glob": {
                "type": "string",
                "description": "Filter files by glob pattern (e.g., '*.rs', '*.{ts,tsx}')"
            },
            "case_insensitive": {
                "type": "boolean",
                "description": "Case insensitive search. Default: false."
            },
            "context_lines": {
                "type": "integer",
                "description": "Number of lines to show before and after each match. Default: 0."
            },
            "output_mode": {
                "type": "string",
                "enum": ["content", "files", "count"],
                "description": "'content' = matching lines (default), 'files' = file paths only, 'count' = match counts per file."
            },
            "max_results": {
                "type": "integer",
                "description": "Maximum results to return (default: 100, max: 500)"
            }
        },
        "required": ["pattern"]
    }),
}
```

**Execution** (in `execute()` method):

```rust
"grep" => {
    let pattern_str = input["pattern"].as_str().ok_or("grep: missing 'pattern'")?;
    let search_path = input["path"].as_str().unwrap_or(".");
    let full_path = validate_path(&ctx.workspace_root, search_path)?;
    let case_insensitive = input["case_insensitive"].as_bool().unwrap_or(false);
    let context = input["context_lines"].as_u64().unwrap_or(0) as usize;
    let output_mode = input["output_mode"].as_str().unwrap_or("content");
    let max_results = input["max_results"].as_u64().unwrap_or(100).min(500) as usize;
    let glob_filter = input["glob"].as_str();

    let regex = regex::RegexBuilder::new(pattern_str)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| format!("invalid regex: {}", e))?;

    let mut results = Vec::new();
    for entry in walkdir::WalkDir::new(&full_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        // Apply glob filter if specified
        if let Some(g) = glob_filter {
            if !matches_glob(e.path(), g) { continue; }
        }
        // Skip binary files
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            // Search and collect matches based on output_mode
            // ... (content/files/count logic)
        }
        if results.len() >= max_results { break; }
    }

    Ok((format_grep_results(&results, output_mode), false))
}
```

### `src-tauri/src/executor/permissions.rs`

```rust
"grep" => (RiskLevel::AutoAllow, String::new()),
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`

```ts
import { Search } from 'lucide-react';
grep: { Icon: Search, colorClass: 'text-accent-hover' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`

Add to "File System" category:

```ts
{ id: 'grep', label: 'Content Search' },
```

## Permission Level

**AutoAllow** — read-only, sandboxed to workspace.

## Dependencies

- `walkdir` crate for recursive directory traversal
- `regex` crate (likely already a dependency)
- `glob` crate (from plan 17)

## Verification

1. `grep { pattern: "fn main" }` -> find all main functions
2. `grep { pattern: "TODO", glob: "*.rs" }` -> TODOs in Rust files only
3. `grep { pattern: "error", case_insensitive: true, context_lines: 2 }` -> matches with context
4. `grep { pattern: "import", output_mode: "files" }` -> just file paths
5. `grep { pattern: "test", output_mode: "count" }` -> counts per file
6. Invalid regex -> clear error message