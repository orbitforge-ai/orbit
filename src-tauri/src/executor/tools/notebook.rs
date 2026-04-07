use serde_json::{json, Value};

const NOTEBOOK_TRUNCATION_LIMIT: usize = 100_000;

pub fn is_notebook_path(path: &str) -> bool {
    path.ends_with(".ipynb")
}

pub fn parse_notebook(content: &str) -> Result<Value, String> {
    let notebook: Value =
        serde_json::from_str(content).map_err(|e| format!("invalid notebook JSON: {}", e))?;
    validate_notebook(&notebook)?;
    Ok(notebook)
}

pub fn validate_notebook(notebook: &Value) -> Result<(), String> {
    if !notebook.is_object() {
        return Err("notebook must be a JSON object".to_string());
    }
    if !notebook.get("cells").and_then(Value::as_array).is_some() {
        return Err("invalid notebook: missing cells array".to_string());
    }
    Ok(())
}

pub fn format_notebook(notebook: &Value) -> String {
    let mut output = String::new();
    let cells = notebook
        .get("cells")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for (index, cell) in cells.iter().enumerate() {
        let cell_type = cell
            .get("cell_type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        output.push_str(&format!("--- Cell {} ({}) ---\n", index, cell_type));
        output.push_str(&source_from_cell(cell));
        output.push('\n');

        if let Some(outputs) = cell.get("outputs").and_then(Value::as_array) {
            for (output_index, notebook_output) in outputs.iter().enumerate() {
                let rendered = format_output(notebook_output);
                if rendered.trim().is_empty() {
                    continue;
                }
                output.push_str(&format!("[Output {}]\n{}\n", output_index, rendered));
            }
        }

        output.push('\n');
    }

    truncate_for_display(output)
}

pub fn serialize_notebook_pretty(notebook: &Value) -> Result<String, String> {
    validate_notebook(notebook)?;
    serde_json::to_string_pretty(notebook)
        .map_err(|e| format!("failed to serialize notebook: {}", e))
}

pub fn notebook_from_input(content: &Value) -> Result<Value, String> {
    match content {
        Value::String(raw) => parse_notebook(raw),
        Value::Object(_) => {
            let notebook = content.clone();
            validate_notebook(&notebook)?;
            Ok(notebook)
        }
        _ => Err("notebook content must be a JSON string or object".to_string()),
    }
}

pub fn replace_cell_source(
    notebook: &mut Value,
    cell_index: usize,
    cell_type: Option<&str>,
    source: &str,
) -> Result<(), String> {
    let cells = cells_mut(notebook)?;
    if cell_index >= cells.len() {
        return Err(format!(
            "cell_number {} is out of range for notebook with {} cell(s)",
            cell_index,
            cells.len()
        ));
    }

    let target_cell_type = normalize_cell_type(cell_type.unwrap_or_else(|| {
        cells[cell_index]
            .get("cell_type")
            .and_then(Value::as_str)
            .unwrap_or("code")
    }))?;
    let new_cell = build_cell(&target_cell_type, source);
    cells[cell_index] = new_cell;
    Ok(())
}

pub fn insert_cell(
    notebook: &mut Value,
    cell_index: usize,
    cell_type: &str,
    source: &str,
) -> Result<(), String> {
    let cells = cells_mut(notebook)?;
    if cell_index > cells.len() {
        return Err(format!(
            "cell_number {} is out of range for insert into notebook with {} cell(s)",
            cell_index,
            cells.len()
        ));
    }

    let cell_type = normalize_cell_type(cell_type)?;
    cells.insert(cell_index, build_cell(&cell_type, source));
    Ok(())
}

pub fn delete_cell(notebook: &mut Value, cell_index: usize) -> Result<(), String> {
    let cells = cells_mut(notebook)?;
    if cell_index >= cells.len() {
        return Err(format!(
            "cell_number {} is out of range for notebook with {} cell(s)",
            cell_index,
            cells.len()
        ));
    }
    cells.remove(cell_index);
    Ok(())
}

fn cells_mut(notebook: &mut Value) -> Result<&mut Vec<Value>, String> {
    notebook
        .get_mut("cells")
        .and_then(Value::as_array_mut)
        .ok_or("invalid notebook: missing cells array".to_string())
}

fn normalize_cell_type(cell_type: &str) -> Result<String, String> {
    match cell_type {
        "code" | "markdown" => Ok(cell_type.to_string()),
        other => Err(format!(
            "invalid cell_type '{}'; expected 'code' or 'markdown'",
            other
        )),
    }
}

fn build_cell(cell_type: &str, source: &str) -> Value {
    match cell_type {
        "markdown" => json!({
            "cell_type": "markdown",
            "metadata": {},
            "source": source_lines(source),
        }),
        _ => json!({
            "cell_type": "code",
            "metadata": {},
            "execution_count": Value::Null,
            "outputs": [],
            "source": source_lines(source),
        }),
    }
}

fn source_from_cell(cell: &Value) -> String {
    cell.get("source")
        .and_then(Value::as_array)
        .map(|parts| parts.iter().filter_map(Value::as_str).collect::<String>())
        .or_else(|| {
            cell.get("source")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn source_lines(source: &str) -> Vec<String> {
    if source.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut remainder = source;
    while let Some(index) = remainder.find('\n') {
        let (line, rest) = remainder.split_at(index + 1);
        lines.push(line.to_string());
        remainder = rest;
    }
    if !remainder.is_empty() {
        lines.push(remainder.to_string());
    }
    lines
}

fn format_output(output: &Value) -> String {
    if let Some(text) = output.get("text") {
        return output_text(text);
    }
    if let Some(data) = output.get("data").and_then(Value::as_object) {
        if let Some(text_plain) = data.get("text/plain") {
            return output_text(text_plain);
        }
        if let Some(markdown) = data.get("text/markdown") {
            return output_text(markdown);
        }
    }
    if let Some(traceback) = output.get("traceback") {
        return output_text(traceback);
    }
    if let Some(ename) = output.get("ename").and_then(Value::as_str) {
        let evalue = output
            .get("evalue")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return if evalue.is_empty() {
            ename.to_string()
        } else {
            format!("{}: {}", ename, evalue)
        };
    }
    String::new()
}

fn output_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect::<String>(),
        _ => String::new(),
    }
}

fn truncate_for_display(mut content: String) -> String {
    if content.len() > NOTEBOOK_TRUNCATION_LIMIT {
        content.truncate(NOTEBOOK_TRUNCATION_LIMIT);
        content.push_str("\n[notebook truncated at 100KB]");
    }
    content
}

#[cfg(test)]
mod tests {
    use super::{
        delete_cell, format_notebook, insert_cell, parse_notebook, replace_cell_source,
        serialize_notebook_pretty,
    };
    use serde_json::json;

    #[test]
    fn formats_cells_and_outputs() {
        let notebook = json!({
            "cells": [
                {
                    "cell_type": "markdown",
                    "source": ["# Title\n", "More text"]
                },
                {
                    "cell_type": "code",
                    "source": ["print('hi')"],
                    "outputs": [{"text": ["hi\n"]}]
                }
            ]
        });

        let formatted = format_notebook(&notebook);
        assert!(formatted.contains("--- Cell 0 (markdown) ---"));
        assert!(formatted.contains("# Title"));
        assert!(formatted.contains("[Output 0]"));
        assert!(formatted.contains("hi"));
    }

    #[test]
    fn edits_cells() {
        let mut notebook = json!({
            "cells": [
                {"cell_type": "code", "source": ["print('a')"], "metadata": {}, "outputs": []}
            ]
        });

        replace_cell_source(&mut notebook, 0, Some("markdown"), "# Hello").unwrap();
        insert_cell(&mut notebook, 1, "code", "print('b')").unwrap();
        delete_cell(&mut notebook, 0).unwrap();

        let cells = notebook["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0]["cell_type"], "code");
    }

    #[test]
    fn parses_and_serializes_notebook_json() {
        let raw = r#"{"cells":[{"cell_type":"markdown","source":["hi"],"metadata":{}}]}"#;
        let notebook = parse_notebook(raw).unwrap();
        let serialized = serialize_notebook_pretty(&notebook).unwrap();
        assert!(serialized.contains("\"cells\""));
    }
}
