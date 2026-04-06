# Plan: Expand `read_file` + `write_file` with Notebook Support

> **Source**: Claude Code's `NotebookEdit` + `Read` (which handles .ipynb). Not present in OpenClaw.
> **Approach**: Expand existing `read_file` and `write_file` to handle .ipynb files natively, plus add a `notebook_edit` action mode.

## Context

Claude Code's `Read` tool natively handles Jupyter notebooks, rendering all cells with outputs. Its `NotebookEdit` tool supports cell-level operations: insert, replace, delete cells by number. For Orbit agents doing data science or ML work, notebook support is essential.

## What Changes

### `read_file` expansion

When the file path ends in `.ipynb`, parse the notebook JSON and return a human-readable representation showing cell numbers, types (code/markdown), source, and outputs.

### `write_file` expansion

When writing `.ipynb` files, accept either raw JSON notebook format or a structured cell format.

### New `edit_file` mode for notebooks

The `edit_file` tool (plan 01) gains notebook awareness: when the target is `.ipynb`, support cell-level operations via special parameters.

Alternatively, add a `notebook_action` parameter to `edit_file`:

```rust
"notebook_action": {
    "type": "string",
    "enum": ["replace_cell", "insert_cell", "delete_cell"],
    "description": "Notebook cell operation (only for .ipynb files)"
},
"cell_number": {
    "type": "integer",
    "description": "Cell index (0-based) for notebook operations"
},
"cell_type": {
    "type": "string",
    "enum": ["code", "markdown"],
    "description": "Cell type (for insert/replace). Default: code."
},
"cell_source": {
    "type": "string",
    "description": "New cell content (for insert/replace)"
}
```

## Backend Changes

### Modify existing: `src-tauri/src/executor/tools/read_file.rs` + `tools/edit_file.rs`

> After plan 00 refactor, these live in their own files. Expand the existing `ReadFileTool` to detect `.ipynb` and format notebooks. Expand `EditFileTool` (from plan 01) with notebook cell actions.

**Modify `read_file` execute method:**

```rust
"read_file" => {
    let path = input["path"].as_str().ok_or("read_file: missing 'path'")?;
    let full_path = validate_path(&ctx.workspace_root, path)?;

    if path.ends_with(".ipynb") {
        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("failed to read {}: {}", path, e))?;
        let notebook: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("invalid notebook JSON: {}", e))?;
        let formatted = format_notebook(&notebook);
        Ok((formatted, false))
    } else {
        // Existing text file reading (unchanged)
    }
}
```

**Add notebook formatter:**

```rust
fn format_notebook(notebook: &serde_json::Value) -> String {
    let mut output = String::new();
    if let Some(cells) = notebook["cells"].as_array() {
        for (i, cell) in cells.iter().enumerate() {
            let cell_type = cell["cell_type"].as_str().unwrap_or("unknown");
            let source = cell["source"].as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<String>())
                .unwrap_or_default();
            output.push_str(&format!("--- Cell {} ({}) ---\n{}\n", i, cell_type, source));

            // Include outputs for code cells
            if let Some(outputs) = cell["outputs"].as_array() {
                for out in outputs {
                    if let Some(text) = out["text"].as_array() {
                        let text_str = text.iter().filter_map(|v| v.as_str()).collect::<String>();
                        output.push_str(&format!("[Output]: {}\n", text_str));
                    }
                }
            }
            output.push('\n');
        }
    }
    output
}
```

**Add notebook edit support to `tools/edit_file.rs`:**

```rust
"edit_file" if path.ends_with(".ipynb") => {
    if let Some(action) = input["notebook_action"].as_str() {
        let cell_num = input["cell_number"].as_u64()
            .ok_or("notebook edit requires cell_number")? as usize;
        let content = std::fs::read_to_string(&full_path)?;
        let mut notebook: Value = serde_json::from_str(&content)?;
        let cells = notebook["cells"].as_array_mut()
            .ok_or("invalid notebook: no cells array")?;

        match action {
            "replace_cell" => {
                let source = input["cell_source"].as_str().ok_or("requires cell_source")?;
                cells[cell_num]["source"] = json!([source]);
            }
            "insert_cell" => {
                let cell_type = input["cell_type"].as_str().unwrap_or("code");
                let source = input["cell_source"].as_str().ok_or("requires cell_source")?;
                let new_cell = json!({
                    "cell_type": cell_type,
                    "source": [source],
                    "metadata": {},
                    "outputs": []
                });
                cells.insert(cell_num, new_cell);
            }
            "delete_cell" => { cells.remove(cell_num); }
            _ => return Err("unknown notebook_action".to_string()),
        }

        std::fs::write(&full_path, serde_json::to_string_pretty(&notebook)?)?;
        Ok(("Notebook updated.".to_string(), false))
    }
}
```

## Frontend Changes

No new tool visuals needed — this enhances existing `read_file` and `edit_file`.

### `src/components/chat/ToolUseBlock.tsx`

Notebook-aware file reading/writing should build on the shared tool presentation foundation from plan [26-tool-use-ui](26-tool-use-ui.md). This plan should add the notebook-specific renderer for cell output within that shared tool-detail system.

## Permission Level

Same as existing `read_file` (**AutoAllow**) and `edit_file` (**Prompt**).

## Dependencies

- `serde_json` (already available) for notebook JSON parsing
- No new crates needed

## Verification

1. `read_file { path: "notebook.ipynb" }` -> formatted cell output with types and outputs
2. `edit_file { path: "notebook.ipynb", notebook_action: "replace_cell", cell_number: 0, cell_source: "print('hello')" }` -> cell updated
3. `edit_file { path: "notebook.ipynb", notebook_action: "insert_cell", cell_number: 1, cell_type: "markdown", cell_source: "# Analysis" }` -> cell inserted
4. `edit_file { path: "notebook.ipynb", notebook_action: "delete_cell", cell_number: 2 }` -> cell removed
5. Regular .py/.rs files -> unchanged behavior
