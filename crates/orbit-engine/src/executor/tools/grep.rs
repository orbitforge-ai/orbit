use regex::{Regex, RegexBuilder};
use serde_json::json;
use walkdir::WalkDir;

use crate::executor::llm_provider::ToolDefinition;

use super::{
    context::ToolExecutionContext,
    helpers::{compile_globs, matches_globs, validate_path, CompiledGlob},
    ToolHandler,
};

pub struct GrepTool;

const DEFAULT_MAX_RESULTS: usize = 100;
const MAX_RESULTS_LIMIT: usize = 500;

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Content,
    Files,
    Count,
}

#[async_trait::async_trait]
impl ToolHandler for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Search file contents using regex patterns. Returns matching lines with file paths and line numbers, or file/count summaries.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory or file to search within. Relative to the workspace root. Defaults to '.'."
                    },
                    "glob": {
                        "type": "string",
                        "description": "Optional glob filter such as '*.rs' or '*.{ts,tsx}'"
                    },
                    "case_insensitive": {
                        "type": "boolean",
                        "description": "If true, performs a case-insensitive search."
                    },
                    "context_lines": {
                        "type": "integer",
                        "description": "Number of lines of context to include before and after each match. Defaults to 0."
                    },
                    "output_mode": {
                        "type": "string",
                        "enum": ["content", "files", "count"],
                        "description": "content = matching lines, files = matching file paths only, count = counts per file"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum results to return. Defaults to 100 and is capped at 500."
                    }
                },
                "required": ["pattern"]
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
        let pattern = input["pattern"]
            .as_str()
            .ok_or("grep: missing 'pattern' field")?;
        let search_path = input["path"].as_str().unwrap_or(".");
        let workspace_root = ctx.workspace_root();
        let full_path = validate_path(&workspace_root, search_path)?;
        let case_insensitive = input["case_insensitive"].as_bool().unwrap_or(false);
        let context_lines = input["context_lines"].as_u64().unwrap_or(0) as usize;
        let max_results = input["max_results"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_RESULTS as u64)
            .min(MAX_RESULTS_LIMIT as u64) as usize;
        let output_mode = parse_output_mode(input["output_mode"].as_str().unwrap_or("content"))?;
        let glob_filter = input["glob"]
            .as_str()
            .map(compile_globs)
            .transpose()?
            .unwrap_or_default();

        let regex = RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|e| format!("invalid regex '{}': {}", pattern, e))?;

        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        let match_root = if full_path.is_dir() {
            full_path.clone()
        } else {
            full_path.parent().unwrap_or(&workspace_root).to_path_buf()
        };

        let mut results = Vec::new();
        let mut truncated = false;

        if full_path.is_file() {
            truncated = search_file(
                &full_path,
                &workspace_root,
                &match_root,
                &regex,
                &glob_filter,
                context_lines,
                output_mode,
                max_results,
                &mut results,
            )?;
        } else if full_path.is_dir() {
            for entry in WalkDir::new(&full_path)
                .into_iter()
                .filter_map(|entry| entry.ok())
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                if search_file(
                    entry.path(),
                    &workspace_root,
                    &full_path,
                    &regex,
                    &glob_filter,
                    context_lines,
                    output_mode,
                    max_results,
                    &mut results,
                )? {
                    truncated = true;
                    break;
                }
            }
        } else {
            return Err(format!(
                "grep: '{}' is not a file or directory",
                search_path
            ));
        }

        if results.is_empty() {
            return Ok(("No matches found.".to_string(), false));
        }

        if truncated {
            results.push(format!(
                "[truncated to {} result(s); narrow the pattern or glob for more precision]",
                max_results
            ));
        }

        let output = if output_mode == OutputMode::Content {
            results.join("\n\n")
        } else {
            results.join("\n")
        };

        Ok((output, false))
    }
}

fn parse_output_mode(value: &str) -> Result<OutputMode, String> {
    match value {
        "content" => Ok(OutputMode::Content),
        "files" => Ok(OutputMode::Files),
        "count" => Ok(OutputMode::Count),
        other => Err(format!(
            "grep: invalid output_mode '{}'; expected one of: content, files, count",
            other
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn search_file(
    path: &std::path::Path,
    workspace_root: &std::path::Path,
    glob_root: &std::path::Path,
    regex: &Regex,
    glob_filter: &[CompiledGlob],
    context_lines: usize,
    output_mode: OutputMode,
    max_results: usize,
    results: &mut Vec<String>,
) -> Result<bool, String> {
    if !glob_filter.is_empty() && !matches_globs(path, glob_root, glob_filter) {
        return Ok(false);
    }

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return Ok(false),
    };

    let relative = path
        .strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();
    let lines: Vec<&str> = content.lines().collect();

    match output_mode {
        OutputMode::Files => {
            if regex.is_match(&content) {
                results.push(relative);
            }
        }
        OutputMode::Count => {
            let count: usize = lines.iter().map(|line| regex.find_iter(line).count()).sum();
            if count > 0 {
                results.push(format!("{}: {} match(es)", relative, count));
            }
        }
        OutputMode::Content => {
            for (index, line) in lines.iter().enumerate() {
                if !regex.is_match(line) {
                    continue;
                }
                results.push(format_match(&relative, &lines, index, context_lines));
                if results.len() >= max_results {
                    return Ok(true);
                }
            }
        }
    }

    Ok(results.len() >= max_results)
}

fn format_match(path: &str, lines: &[&str], match_index: usize, context_lines: usize) -> String {
    if context_lines == 0 {
        return format!("{}:{}: {}", path, match_index + 1, lines[match_index]);
    }

    let start = match_index.saturating_sub(context_lines);
    let end = (match_index + context_lines + 1).min(lines.len());
    let mut block = vec![format!("{}:{}", path, match_index + 1)];

    for (index, line) in lines[start..end].iter().enumerate() {
        let line_number = start + index + 1;
        let marker = if line_number == match_index + 1 {
            ">"
        } else {
            " "
        };
        block.push(format!("{} {:>4} | {}", marker, line_number, line));
    }

    block.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{format_match, parse_output_mode, OutputMode};

    #[test]
    fn parses_output_mode() {
        assert!(matches!(
            parse_output_mode("content"),
            Ok(OutputMode::Content)
        ));
        assert!(parse_output_mode("nope").is_err());
    }

    #[test]
    fn formats_context_blocks() {
        let lines = vec!["alpha", "beta", "gamma"];
        let formatted = format_match("src/main.rs", &lines, 1, 1);
        assert!(formatted.contains("src/main.rs:2"));
        assert!(formatted.contains(">    2 | beta"));
    }
}
