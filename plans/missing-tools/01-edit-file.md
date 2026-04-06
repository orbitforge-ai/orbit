# Plan: `edit_file` Tool

## Context
Orbit's `write_file` tool requires rewriting entire files. OpenClaw's `Edit` tool supports targeted find-and-replace, which is far more efficient for small changes and reduces token usage since the agent only sends the diff. This is one of the highest-value missing tools.

## What It Does
Perform exact string replacement within a file — find `old_text` and replace with `new_text`. Supports a `replace_all` flag for bulk renaming. Fails if `old_text` is not found or matches multiple locations (unless `replace_all` is set).

## Backend Changes

### New file: `src-tauri/src/executor/tools/edit_file.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod edit_file;` and `Box::new(edit_file::EditFileTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):
```rust
ToolDefinition {
    name: "edit_file".to_string(),
    description: "Edit a file by replacing exact text. The old_text must match exactly (including whitespace and indentation). Use replace_all to change every occurrence.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Relative path to the file within the workspace"
            },
            "old_text": {
                "type": "string",
                "description": "The exact text to find and replace"
            },
            "new_text": {
                "type": "string",
                "description": "The replacement text"
            },
            "replace_all": {
                "type": "boolean",
                "description": "If true, replace all occurrences. If false (default), fail when multiple matches exist."
            }
        },
        "required": ["path", "old_text", "new_text"]
    }),
}
```

**Execution** (in `execute()` method):
```rust
"edit_file" => {
    let path = input["path"].as_str().ok_or("edit_file: missing 'path'")?;
    let old_text = input["old_text"].as_str().ok_or("edit_file: missing 'old_text'")?;
    let new_text = input["new_text"].as_str().ok_or("edit_file: missing 'new_text'")?;
    let replace_all = input["replace_all"].as_bool().unwrap_or(false);

    let full_path = validate_path(&ctx.workspace_root, path)?;
    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("failed to read {}: {}", path, e))?;

    let count = content.matches(old_text).count();
    if count == 0 {
        return Err(format!("edit_file: old_text not found in '{}'", path));
    }
    if count > 1 && !replace_all {
        return Err(format!(
            "edit_file: old_text found {} times in '{}'. Use replace_all:true or provide more context.",
            count, path
        ));
    }

    let new_content = if replace_all {
        content.replace(old_text, new_text)
    } else {
        content.replacen(old_text, new_text, 1)
    };

    std::fs::write(&full_path, &new_content)
        .map_err(|e| format!("failed to write {}: {}", path, e))?;

    Ok((format!("Replaced {} occurrence(s) in '{}'", count, path), false))
}
```

### `src-tauri/src/executor/permissions.rs`
Add to `classify_tool_call()`:
```rust
"edit_file" => {
    let path = input["path"].as_str().unwrap_or("<unknown>");
    if permission_mode == "strict" {
        return (RiskLevel::Prompt, format!("Strict mode: edit_file '{}'", path));
    }
    (RiskLevel::Prompt, format!("File edit: '{}'", path))
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`
```ts
import { FileEdit } from 'lucide-react'; // or Pencil
// Add to TOOL_VISUALS:
edit_file: { Icon: FileEdit, colorClass: 'text-accent' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`
Add to the "File System" category:
```ts
{ id: 'edit_file', label: 'Edit Files' },
```

## Permission Level
- **Normal mode**: `Prompt` (same as write_file — it modifies files)
- **Strict mode**: `Prompt`
- **Permissive mode**: `AutoAllow`

## Dependencies
None — pure string operations on already-read files.

## Verification
1. Create a test file via `write_file` with known content
2. Use `edit_file` to replace a unique string → confirm content changed
3. Test `replace_all: true` with multiple occurrences
4. Test error case: `old_text` not found → should return error
5. Test error case: multiple matches without `replace_all` → should fail with count
6. Confirm permission prompt appears in Normal mode
