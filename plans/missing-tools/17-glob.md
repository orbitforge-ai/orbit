# Plan: Expand `list_files` with Glob Pattern Matching

> **Source**: Claude Code's `Glob` tool. Not present in OpenClaw.
> **Approach**: Expand existing `list_files` rather than adding a new tool.

## Context

Orbit's `list_files` does a flat directory listing. Claude Code's `Glob` tool supports full glob patterns like `**/*.rs` or `src/**/*.tsx`, returning matches sorted by modification time. This is essential for agents navigating codebases — finding files by extension, locating test files, discovering config files, etc.

## What Changes

Add a `pattern` parameter to `list_files`. When `pattern` is provided, perform a glob search instead of a flat listing.

### Expand `list_files` schema

```rust
"pattern": {
    "type": "string",
    "description": "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.tsx'). When provided, searches recursively from path."
}
```

### How it works

```
list_files { path: "src", pattern: "**/*.rs" }
→ ["src/main.rs", "src/lib.rs", "src/commands/chat.rs", ...]

list_files { path: "." }
→ (existing flat listing behavior, unchanged)
```

## Backend Changes

### `Cargo.toml`

```toml
glob = "0.3"
```

### Modify existing: `src-tauri/src/executor/tools/list_files.rs`

> After plan 00 refactor, `list_files` lives in its own file. Expand the existing `ListFilesTool` — update `definition()` to add the `pattern` field, update `execute()` to handle glob mode.

Modify the `list_files` execute method:

```rust
"list_files" => {
    let path = input["path"].as_str().ok_or("list_files: missing 'path'")?;
    let full_path = validate_path(&ctx.workspace_root, path)?;
    let pattern = input["pattern"].as_str();

    if let Some(glob_pattern) = pattern {
        // Glob mode: recursive pattern matching
        let search_pattern = full_path.join(glob_pattern)
            .to_string_lossy().to_string();
        let mut entries: Vec<(String, std::time::SystemTime)> = Vec::new();
        for entry in glob::glob(&search_pattern)
            .map_err(|e| format!("invalid glob pattern: {}", e))?
        {
            if let Ok(p) = entry {
                let mtime = p.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let relative = p.strip_prefix(&ctx.workspace_root)
                    .unwrap_or(&p)
                    .to_string_lossy().to_string();
                entries.push((relative, mtime));
            }
        }
        // Sort by modification time (newest first)
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        // Cap at 500 results
        entries.truncate(500);
        let result: Vec<String> = entries.into_iter().map(|(p, _)| p).collect();
        Ok((result.join("\n"), false))
    } else {
        // Existing flat listing behavior (unchanged)
        // ...
    }
}
```

### `src-tauri/src/executor/permissions.rs`

No changes — `list_files` is already `AutoAllow`.

## Frontend Changes

No changes needed — `list_files` visual already exists.

## Dependencies

- `glob` crate (lightweight, well-maintained)

## Verification

1. `list_files { path: ".", pattern: "**/*.rs" }` -> all Rust files found recursively
2. `list_files { path: "src", pattern: "*.tsx" }` -> only TSX files in src/
3. `list_files { path: "." }` -> unchanged flat listing (backward compatible)
4. Invalid pattern -> clear error message
5. Pattern matching 1000+ files -> truncated at 500 with note