use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::debug;

use crate::events::emitter::{emit_log_chunk, emit_run_state_changed};
use crate::models::run::RunState;
use crate::models::task::ShellCommandConfig;

/// Result of running a process.
pub struct ProcessResult {
    pub exit_code: i32,
    pub duration_ms: i64,
}

/// Runs a shell command task, streaming log lines to the frontend.
/// Returns Ok(ProcessResult) or Err(reason string).
pub async fn run_shell(
    run_id: &str,
    cfg: &ShellCommandConfig,
    log_path: &PathBuf,
    timeout_secs: u64,
    app: &tauri::AppHandle,
) -> Result<ProcessResult, String> {
    let shell = cfg.shell.as_deref().unwrap_or("/bin/sh");
    let cwd = cfg
        .working_directory
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));

    // Ensure log directory exists
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut cmd = Command::new(shell);
    cmd.arg("-c")
        .arg(&cfg.command)
        .current_dir(&cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    // Inject environment variables
    if let Some(env) = &cfg.environment {
        for (k, v) in env {
            cmd.env(k, v);
        }
    }

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;

    let pid = child.id().unwrap_or(0);
    emit_run_state_changed(
        app,
        run_id,
        RunState::Pending.as_str(),
        RunState::Running.as_str(),
    );

    let stdout = child.stdout.take().expect("stdout should be piped");
    let stderr = child.stderr.take().expect("stderr should be piped");

    let run_id_clone = run_id.to_string();
    let log_path_clone = log_path.clone();
    let app_clone = app.clone();

    // Spawn a task to read stdout + stderr and batch-emit log lines
    let log_task = tokio::spawn(async move {
        let mut stdout_lines = BufReader::new(stdout).lines();
        let mut stderr_lines = BufReader::new(stderr).lines();
        let mut batch: Vec<(String, String)> = Vec::new(); // (stream, line)
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        let mut log_file = tokio::fs::File::create(&log_path_clone)
            .await
            .expect("cannot create log file");

        use tokio::io::AsyncWriteExt;

        loop {
            tokio::select! {
                line = stdout_lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            let entry = format!("{}\n", l);
                            let _ = log_file.write_all(entry.as_bytes()).await;
                            batch.push(("stdout".to_string(), l));
                        }
                        _ => break,
                    }
                }
                line = stderr_lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            let entry = format!("[stderr] {}\n", l);
                            let _ = log_file.write_all(entry.as_bytes()).await;
                            batch.push(("stderr".to_string(), l));
                        }
                        _ => {}
                    }
                }
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        emit_log_chunk(&app_clone, &run_id_clone, std::mem::take(&mut batch));
                    }
                }
            }
        }

        // Flush remaining lines
        if !batch.is_empty() {
            emit_log_chunk(&app_clone, &run_id_clone, batch);
        }
    });

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);

    let exit_status = tokio::time::timeout(timeout, child.wait())
        .await
        .map_err(|_| {
            // Timeout — the child is killed via kill_on_drop
            "timed out".to_string()
        })?
        .map_err(|e| e.to_string())?;

    // Wait for the log task to finish flushing
    let _ = log_task.await;

    let duration_ms = start.elapsed().as_millis() as i64;
    let exit_code = exit_status.code().unwrap_or(-1);

    debug!(
        run_id = run_id,
        pid = pid,
        exit_code = exit_code,
        duration_ms = duration_ms,
        "process finished"
    );

    Ok(ProcessResult {
        exit_code,
        duration_ms,
    })
}
