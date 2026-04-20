use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::{json, Map, Value};
use tauri::Emitter;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::warn;
use ulid::Ulid;

use crate::events::emitter::{LogLine, RunLogChunkPayload};
use crate::executor::workspace;
use crate::workflows::nodes::{NodeExecutionContext, NodeOutcome};
use crate::workflows::template::{render_template, OUTPUT_ALIASES_KEY};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;

struct ProcessCapture {
    exit_code: i32,
    stderr: String,
    stdout: String,
}

pub(super) async fn execute<R: tauri::Runtime>(
    ctx: &NodeExecutionContext<'_, R>,
) -> Result<NodeOutcome, String> {
    match ctx.node.node_type.as_str() {
        "code.bash.run" => execute_bash(ctx).await,
        "code.script.run" => execute_script(ctx).await,
        other => Err(format!("unsupported code node type `{}`", other)),
    }
}

async fn execute_bash<R: tauri::Runtime>(
    ctx: &NodeExecutionContext<'_, R>,
) -> Result<NodeOutcome, String> {
    let script_template = required_code_field(&ctx.node.data, "script", "code.bash.run")?;
    let cwd = resolve_working_directory(ctx.project_id, working_directory(&ctx.node.data))?;
    let timeout_secs = timeout_seconds(&ctx.node.data);
    let rendered = render_template(&script_template, ctx.outputs);
    let script_path = cwd.join(format!(".orbit-workflow-{}-{}.sh", ctx.run_id, ctx.node.id));
    let script_content = format!("set -euo pipefail\n{}", rendered);

    tokio::fs::write(&script_path, script_content)
        .await
        .map_err(|e| format!("code.bash.run failed to write temp script: {}", e))?;

    let capture = run_command_capture(
        "/bin/bash",
        &[script_path.to_string_lossy().to_string()],
        &cwd,
        timeout_secs,
    )
    .await;

    let _ = tokio::fs::remove_file(&script_path).await;

    let capture = capture?;
    emit_process_logs(ctx.app, ctx.run_id, &capture.stdout, &capture.stderr);

    if capture.exit_code != 0 {
        return Err(format_process_failure(
            "code.bash.run",
            capture.exit_code,
            &capture.stderr,
        ));
    }

    Ok(NodeOutcome {
        output: json!({
            "cwd": cwd.to_string_lossy(),
            "stdout": capture.stdout,
            "stderr": capture.stderr,
            "exitCode": capture.exit_code,
            "parsed": parse_json_or_null(&capture.stdout),
        }),
        next_handle: None,
    })
}

async fn execute_script<R: tauri::Runtime>(
    ctx: &NodeExecutionContext<'_, R>,
) -> Result<NodeOutcome, String> {
    let source = required_code_field(&ctx.node.data, "source", "code.script.run")?;
    let language = script_language(&ctx.node.data)?;
    let cwd = resolve_working_directory(ctx.project_id, working_directory(&ctx.node.data))?;
    let timeout_secs = timeout_seconds(&ctx.node.data);
    let node_binary = which::which("node")
        .map_err(|_| "code.script.run requires a local `node` binary in PATH".to_string())?;

    if language == "typescript" {
        ensure_typescript_node_support(&node_binary).await?;
    }

    let temp_id = Ulid::new().to_string();
    let extension = if language == "typescript" {
        "mts"
    } else {
        "mjs"
    };
    let module_path = cwd.join(format!(".orbit-workflow-{}.{}", temp_id, extension));
    let context_path = cwd.join(format!(".orbit-workflow-{}.context.json", temp_id));
    let context = json!({
        "trigger": ctx.outputs.get("trigger").cloned().unwrap_or(Value::Null),
        "outputs": ctx.outputs.clone(),
        "refs": build_reference_outputs(ctx.outputs),
        "projectDir": workspace::project_workspace_dir(ctx.project_id).to_string_lossy().to_string(),
        "cwd": cwd.to_string_lossy().to_string(),
    });

    tokio::fs::write(
        &context_path,
        serde_json::to_vec_pretty(&context)
            .map_err(|e| format!("code.script.run failed to serialize context: {}", e))?,
    )
    .await
    .map_err(|e| format!("code.script.run failed to write temp context: {}", e))?;

    tokio::fs::write(
        &module_path,
        build_script_module(
            source.as_str(),
            context_path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| "code.script.run failed to build context filename".to_string())?,
        ),
    )
    .await
    .map_err(|e| format!("code.script.run failed to write temp module: {}", e))?;

    let mut args = Vec::new();
    if language == "typescript" {
        args.push("--experimental-transform-types".to_string());
    }
    args.push(module_path.to_string_lossy().to_string());

    let capture = run_command_capture(&node_binary, &args, &cwd, timeout_secs).await;

    let _ = tokio::fs::remove_file(&module_path).await;
    let _ = tokio::fs::remove_file(&context_path).await;

    let capture = capture?;
    emit_process_logs(ctx.app, ctx.run_id, &capture.stdout, &capture.stderr);

    if capture.exit_code != 0 {
        return Err(format_process_failure(
            "code.script.run",
            capture.exit_code,
            &capture.stderr,
        ));
    }

    let payload: Value = serde_json::from_str(capture.stdout.trim()).map_err(|e| {
        format!(
            "code.script.run expected a structured result on stdout: {}",
            e
        )
    })?;
    let result = payload.get("result").cloned().unwrap_or(Value::Null);

    Ok(NodeOutcome {
        output: json!({
            "language": language,
            "cwd": cwd.to_string_lossy(),
            "result": result,
        }),
        next_handle: None,
    })
}

async fn run_command_capture(
    program: impl AsRef<Path>,
    args: &[String],
    cwd: &Path,
    timeout_secs: u64,
) -> Result<ProcessCapture, String> {
    let temp_dir = std::env::temp_dir();
    let temp_id = Ulid::new().to_string();
    let stdout_path = temp_dir.join(format!("orbit-workflow-{}-stdout.log", temp_id));
    let stderr_path = temp_dir.join(format!("orbit-workflow-{}-stderr.log", temp_id));
    let stdout_file = std::fs::File::create(&stdout_path)
        .map_err(|e| format!("failed to create stdout capture: {}", e))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| format!("failed to create stderr capture: {}", e))?;

    let mut child = Command::new(program.as_ref());
    child
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .kill_on_drop(true);

    let mut child = child
        .spawn()
        .map_err(|e| format!("failed to start process: {}", e))?;

    let status = match timeout(
        Duration::from_secs(timeout_secs.clamp(1, MAX_TIMEOUT_SECS)),
        child.wait(),
    )
    .await
    {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            let _ = cleanup_capture_files(&stdout_path, &stderr_path).await;
            return Err(format!("failed to run process: {}", e));
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let stdout = tokio::fs::read_to_string(&stdout_path)
                .await
                .unwrap_or_default();
            let stderr = tokio::fs::read_to_string(&stderr_path)
                .await
                .unwrap_or_default();
            let _ = cleanup_capture_files(&stdout_path, &stderr_path).await;
            let detail = if stdout.trim().is_empty() && stderr.trim().is_empty() {
                String::new()
            } else {
                let mut parts = Vec::new();
                if !stdout.trim().is_empty() {
                    parts.push(format!("stdout: {}", stdout.trim()));
                }
                if !stderr.trim().is_empty() {
                    parts.push(format!("stderr: {}", stderr.trim()));
                }
                format!(" ({})", parts.join(" | "))
            };
            return Err(format!(
                "process timed out after {}s{}",
                timeout_secs, detail
            ));
        }
    };

    let stdout = tokio::fs::read_to_string(&stdout_path)
        .await
        .unwrap_or_default();
    let stderr = tokio::fs::read_to_string(&stderr_path)
        .await
        .unwrap_or_default();
    let _ = cleanup_capture_files(&stdout_path, &stderr_path).await;

    Ok(ProcessCapture {
        exit_code: status.code().unwrap_or(-1),
        stderr,
        stdout,
    })
}

async fn cleanup_capture_files(stdout_path: &Path, stderr_path: &Path) -> Result<(), String> {
    if stdout_path.exists() {
        tokio::fs::remove_file(stdout_path)
            .await
            .map_err(|e| format!("failed to remove stdout capture: {}", e))?;
    }
    if stderr_path.exists() {
        tokio::fs::remove_file(stderr_path)
            .await
            .map_err(|e| format!("failed to remove stderr capture: {}", e))?;
    }
    Ok(())
}

fn emit_process_logs<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    run_id: &str,
    stdout: &str,
    stderr: &str,
) {
    let mut lines = Vec::new();
    lines.extend(
        stdout
            .lines()
            .map(|line| ("stdout".to_string(), line.to_string())),
    );
    lines.extend(
        stderr
            .lines()
            .map(|line| ("stderr".to_string(), line.to_string())),
    );
    if !lines.is_empty() {
        let payload = RunLogChunkPayload {
            run_id: run_id.to_string(),
            lines: lines
                .into_iter()
                .map(|(stream, line)| LogLine { stream, line })
                .collect(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        if let Err(error) = app.emit("run:log_chunk", &payload) {
            warn!("failed to emit run:log_chunk: {}", error);
        }
    }
}

fn build_reference_outputs(outputs: &Value) -> Value {
    let mut refs = Map::new();
    let Some(aliases) = outputs.get(OUTPUT_ALIASES_KEY).and_then(Value::as_object) else {
        return Value::Object(refs);
    };

    for (alias, node_id) in aliases {
        let Some(node_id) = node_id.as_str() else {
            continue;
        };
        refs.insert(
            alias.clone(),
            outputs
                .get(node_id)
                .and_then(|value| value.get("output"))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }

    Value::Object(refs)
}

fn build_script_module(source: &str, context_file_name: &str) -> String {
    format!(
        r#"import fs from 'node:fs/promises';
import {{ Console }} from 'node:console';

globalThis.console = new Console({{
  stdout: process.stderr,
  stderr: process.stderr,
  inspectOptions: {{ depth: null }},
}});

const contextUrl = new URL('./{context_file_name}', import.meta.url);
const context = JSON.parse(await fs.readFile(contextUrl, 'utf8'));

const userCode = async ({{ trigger, outputs, refs, projectDir, cwd }}) => {{
{source}
}};

try {{
  const rawResult = await userCode(context);
  const normalizedResult = rawResult === undefined ? null : rawResult;
  process.stdout.write(JSON.stringify({{ result: normalizedResult }}));
}} catch (error) {{
  const message =
    error instanceof Error
      ? `${{error.name}}: ${{error.stack ?? error.message}}`
      : String(error);
  process.stderr.write(`${{message}}\n`);
  process.exit(1);
}}
"#
    )
}

fn format_process_failure(node_type: &str, exit_code: i32, stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("{} failed with exit code {}", node_type, exit_code)
    } else {
        format!(
            "{} failed with exit code {}: {}",
            node_type, exit_code, stderr
        )
    }
}

fn parse_json_or_null(text: &str) -> Value {
    serde_json::from_str(text.trim()).unwrap_or(Value::Null)
}

fn required_code_field(data: &Value, field: &str, node_type: &str) -> Result<String, String> {
    data.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{} requires data.{}", node_type, field))
}

fn timeout_seconds(data: &Value) -> u64 {
    data.get("timeoutSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS)
}

fn working_directory(data: &Value) -> &str {
    data.get("workingDirectory")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(".")
}

fn script_language(data: &Value) -> Result<&str, String> {
    match data
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("typescript")
    {
        "javascript" => Ok("javascript"),
        "typescript" => Ok("typescript"),
        other => Err(format!(
            "code.script.run has unsupported language '{}'",
            other
        )),
    }
}

fn resolve_relative_working_directory(root: &Path, requested: &str) -> Result<PathBuf, String> {
    let requested = requested.trim();
    let requested = if requested.is_empty() { "." } else { requested };
    if Path::new(requested).is_absolute() {
        return Err(
            "workingDirectory must be a relative path inside the project workspace".to_string(),
        );
    }
    let resolved = root.join(requested);

    if resolved.exists() {
        let canonical = resolved
            .canonicalize()
            .map_err(|e| format!("failed to resolve workingDirectory: {}", e))?;
        let root_canonical = root
            .canonicalize()
            .map_err(|e| format!("failed to resolve workspace root: {}", e))?;
        if !canonical.starts_with(&root_canonical) {
            return Err(format!(
                "workingDirectory escapes project workspace: {}",
                requested
            ));
        }
        return Ok(canonical);
    }

    let parent = resolved
        .parent()
        .ok_or_else(|| "invalid workingDirectory: no parent".to_string())?;
    let mut ancestor = parent.to_path_buf();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "invalid workingDirectory: no existing ancestor".to_string())?
            .to_path_buf();
    }

    let ancestor_canonical = ancestor
        .canonicalize()
        .map_err(|e| format!("failed to resolve workingDirectory ancestor: {}", e))?;
    let root_canonical = root
        .canonicalize()
        .map_err(|e| format!("failed to resolve workspace root: {}", e))?;
    if !ancestor_canonical.starts_with(&root_canonical) {
        return Err(format!(
            "workingDirectory escapes project workspace: {}",
            requested
        ));
    }

    Ok(resolved)
}

fn resolve_working_directory(project_id: &str, requested: &str) -> Result<PathBuf, String> {
    let root = workspace::project_workspace_dir(project_id);
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("failed to create project workspace: {}", e))?;
    let resolved = resolve_relative_working_directory(&root, requested)?;
    std::fs::create_dir_all(&resolved)
        .map_err(|e| format!("failed to create workingDirectory: {}", e))?;
    Ok(resolved)
}

async fn ensure_typescript_node_support(node_binary: &Path) -> Result<(), String> {
    let version_output = Command::new(node_binary)
        .arg("--version")
        .output()
        .await
        .map_err(|e| format!("code.script.run failed to inspect node version: {}", e))?;
    if !version_output.status.success() {
        return Err("code.script.run could not determine the local node version".to_string());
    }

    let version = String::from_utf8_lossy(&version_output.stdout);
    let major = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    if major < 22 {
        return Err("Node 22+ required for TypeScript workflow nodes".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_reference_outputs, build_script_module, resolve_relative_working_directory,
        run_command_capture,
    };
    use serde_json::json;

    fn unique_temp_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("orbit-code-node-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn resolve_relative_working_directory_rejects_escapes() {
        let root = unique_temp_dir();
        let err = resolve_relative_working_directory(&root, "../escape")
            .expect_err("parent traversal should be rejected");
        assert!(err.contains("escapes project workspace"));
        let err = resolve_relative_working_directory(&root, root.to_string_lossy().as_ref())
            .expect_err("absolute paths should be rejected");
        assert!(err.contains("relative path"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_relative_working_directory_allows_nested_paths() {
        let root = unique_temp_dir();
        let resolved =
            resolve_relative_working_directory(&root, "nested/scripts").expect("nested path works");
        assert!(resolved.ends_with("nested/scripts"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn build_reference_outputs_uses_aliases() {
        let refs = build_reference_outputs(&json!({
            "node-1": { "output": { "ok": true } },
            "node-2": { "output": { "value": 3 } },
            "__aliases": {
                "bash-node": "node-1",
                "script-node": "node-2"
            }
        }));

        assert_eq!(
            refs,
            json!({
                "bash-node": { "ok": true },
                "script-node": { "value": 3 }
            })
        );
    }

    #[tokio::test]
    async fn run_command_capture_collects_stdout_and_stderr() {
        let cwd = unique_temp_dir();
        let capture = run_command_capture(
            "/bin/bash",
            &[
                "-lc".to_string(),
                "printf '{\"ok\":true}'; printf 'warn\\n' >&2".to_string(),
            ],
            &cwd,
            30,
        )
        .await
        .expect("command capture succeeds");

        assert_eq!(capture.exit_code, 0);
        assert_eq!(capture.stdout, "{\"ok\":true}");
        assert_eq!(capture.stderr, "warn\n");
        let _ = std::fs::remove_dir_all(cwd);
    }

    #[tokio::test]
    async fn node_runtime_can_execute_javascript_module_wrapper() {
        let cwd = unique_temp_dir();
        let context_path = cwd.join(".context.json");
        let module_path = cwd.join(".script.mjs");
        std::fs::write(
            &context_path,
            serde_json::to_string(&json!({
                "trigger": { "data": { "count": 2 }, "kind": "manual" },
                "outputs": {},
                "refs": {},
                "projectDir": cwd.to_string_lossy(),
                "cwd": cwd.to_string_lossy(),
            }))
            .expect("serialize context"),
        )
        .expect("write context");
        std::fs::write(
            &module_path,
            build_script_module(
                "return { total: trigger.data.count + 1, cwd };",
                context_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .expect("context file name"),
            ),
        )
        .expect("write module");

        let capture = run_command_capture(
            which::which("node").expect("node installed"),
            &[module_path.to_string_lossy().to_string()],
            &cwd,
            30,
        )
        .await
        .expect("node wrapper succeeds");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(capture.stdout.trim()).expect("json output"),
            json!({
                "result": {
                    "total": 3,
                    "cwd": cwd.to_string_lossy()
                }
            })
        );

        let _ = std::fs::remove_dir_all(cwd);
    }

    #[tokio::test]
    async fn node_runtime_can_execute_typescript_with_transform_types() {
        let cwd = unique_temp_dir();
        let context_path = cwd.join(".context.json");
        let module_path = cwd.join(".script.mts");
        std::fs::write(
            &context_path,
            serde_json::to_string(&json!({
                "trigger": { "data": { "count": 2 }, "kind": "manual" },
                "outputs": {},
                "refs": {},
                "projectDir": cwd.to_string_lossy(),
                "cwd": cwd.to_string_lossy(),
            }))
            .expect("serialize context"),
        )
        .expect("write context");
        std::fs::write(
            &module_path,
            build_script_module(
                "type Payload = { total: number };\nconst payload: Payload = { total: trigger.data.count + 2 };\nreturn payload;",
                context_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .expect("context file name"),
            ),
        )
        .expect("write module");

        let capture = run_command_capture(
            which::which("node").expect("node installed"),
            &[
                "--experimental-transform-types".to_string(),
                module_path.to_string_lossy().to_string(),
            ],
            &cwd,
            30,
        )
        .await
        .expect("typescript wrapper succeeds");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(capture.stdout.trim()).expect("json output"),
            json!({
                "result": {
                    "total": 4
                }
            })
        );

        let _ = std::fs::remove_dir_all(cwd);
    }

    #[tokio::test]
    async fn node_runtime_supports_relative_imports_from_cwd() {
        let cwd = unique_temp_dir();
        let helper_path = cwd.join("helper.mjs");
        let context_path = cwd.join(".context.json");
        let module_path = cwd.join(".script.mjs");
        std::fs::write(
            &helper_path,
            "export function bump(value) { return value + 5; }\n",
        )
        .expect("write helper");
        std::fs::write(
            &context_path,
            serde_json::to_string(&json!({
                "trigger": { "data": { "count": 2 }, "kind": "manual" },
                "outputs": {},
                "refs": {},
                "projectDir": cwd.to_string_lossy(),
                "cwd": cwd.to_string_lossy(),
            }))
            .expect("serialize context"),
        )
        .expect("write context");
        std::fs::write(
            &module_path,
            build_script_module(
                "const helper = await import('./helper.mjs');\nreturn { total: helper.bump(trigger.data.count) };",
                context_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .expect("context file name"),
            ),
        )
        .expect("write module");

        let capture = run_command_capture(
            which::which("node").expect("node installed"),
            &[module_path.to_string_lossy().to_string()],
            &cwd,
            30,
        )
        .await
        .expect("dynamic import succeeds");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(capture.stdout.trim()).expect("json output"),
            json!({
                "result": {
                    "total": 7
                }
            })
        );

        let _ = std::fs::remove_dir_all(cwd);
    }

    #[tokio::test]
    async fn node_runtime_surfaces_non_serializable_results() {
        let cwd = unique_temp_dir();
        let context_path = cwd.join(".context.json");
        let module_path = cwd.join(".script.mjs");
        std::fs::write(
            &context_path,
            serde_json::to_string(&json!({
                "trigger": { "data": {}, "kind": "manual" },
                "outputs": {},
                "refs": {},
                "projectDir": cwd.to_string_lossy(),
                "cwd": cwd.to_string_lossy(),
            }))
            .expect("serialize context"),
        )
        .expect("write context");
        std::fs::write(
            &module_path,
            build_script_module(
                "const value = {};\nvalue.self = value;\nreturn value;",
                context_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .expect("context file name"),
            ),
        )
        .expect("write module");

        let capture = run_command_capture(
            which::which("node").expect("node installed"),
            &[module_path.to_string_lossy().to_string()],
            &cwd,
            30,
        )
        .await
        .expect("node wrapper runs");

        assert_ne!(capture.exit_code, 0);
        assert!(
            capture.stderr.contains("TypeError"),
            "stderr was: {}",
            capture.stderr
        );

        let _ = std::fs::remove_dir_all(cwd);
    }
}
