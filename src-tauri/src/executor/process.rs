use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::debug;

use crate::events::emitter::emit_log_chunk;
use crate::models::task::{ScriptFileConfig, ShellCommandConfig};

/// Result of running a process.
pub struct ProcessResult {
    pub exit_code: i32,
    pub duration_ms: i64,
}

/// Runs a shell command task, streaming log lines to the frontend.
/// The `cancel` receiver fires if the run is cancelled externally.
pub async fn run_shell(
    run_id: &str,
    cfg: &ShellCommandConfig,
    log_path: &PathBuf,
    timeout_secs: u64,
    app: &tauri::AppHandle,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<ProcessResult, String> {
    let shell = cfg.shell.as_deref().unwrap_or("/bin/sh");
    let cwd = cfg
        .working_directory
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));

    run_command(
        run_id,
        shell,
        &["-c", &cfg.command],
        &cwd,
        cfg.environment.as_ref(),
        log_path,
        timeout_secs,
        app,
        cancel,
    )
    .await
}

/// Runs a script file task.
pub async fn run_script(
    run_id: &str,
    cfg: &ScriptFileConfig,
    log_path: &PathBuf,
    timeout_secs: u64,
    app: &tauri::AppHandle,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<ProcessResult, String> {
    let interpreter = cfg.interpreter.as_deref().unwrap_or("/bin/sh");
    let cwd = cfg
        .working_directory
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));

    run_command(
        run_id,
        interpreter,
        &[&cfg.script_path],
        &cwd,
        cfg.environment.as_ref(),
        log_path,
        timeout_secs,
        app,
        cancel,
    )
    .await
}

async fn run_command(
    run_id: &str,
    program: &str,
    args: &[&str],
    cwd: &PathBuf,
    environment: Option<&std::collections::HashMap<String, String>>,
    log_path: &PathBuf,
    timeout_secs: u64,
    app: &tauri::AppHandle,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<ProcessResult, String> {
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    if let Some(env) = environment {
        for (k, v) in env {
            cmd.env(k, v);
        }
    }

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;

    let pid = child.id().unwrap_or(0);
    let stdout = child.stdout.take().expect("stdout should be piped");
    let stderr = child.stderr.take().expect("stderr should be piped");

    let run_id_clone = run_id.to_string();
    let log_path_clone = log_path.clone();
    let app_clone = app.clone();

    // Spawn a task to read stdout + stderr and batch-emit log lines
    let log_task = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;

        let mut stdout_lines = BufReader::new(stdout).lines();
        let mut stderr_lines = BufReader::new(stderr).lines();
        let mut batch: Vec<(String, String)> = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        let mut log_file = tokio::fs::File::create(&log_path_clone)
            .await
            .expect("cannot create log file");
        let mut bytes_written: u64 = 0;
        let mut rotation_index: u8 = 0;

        async fn rotate_log(
            log_path: &PathBuf,
            current_file: tokio::fs::File,
            rotation_index: &mut u8,
        ) -> tokio::fs::File {
            drop(current_file);
            for i in (1..=(*rotation_index).min(2)).rev() {
                let from = log_path.with_extension(format!("log.{}", i));
                let to = log_path.with_extension(format!("log.{}", i + 1));
                let _ = tokio::fs::rename(&from, &to).await;
            }
            let rotated = log_path.with_extension("log.1");
            let _ = tokio::fs::rename(log_path, &rotated).await;
            *rotation_index = (*rotation_index + 1).min(3);
            tokio::fs::File::create(log_path)
                .await
                .expect("cannot create rotated log file")
        }

        loop {
            tokio::select! {
                line = stdout_lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            let entry = format!("{}\n", l);
                            let entry_bytes = entry.as_bytes();
                            let _ = log_file.write_all(entry_bytes).await;
                            bytes_written += entry_bytes.len() as u64;
                            if bytes_written >= 50 * 1024 * 1024 {
                                log_file = rotate_log(&log_path_clone, log_file, &mut rotation_index).await;
                                bytes_written = 0;
                            }
                            batch.push(("stdout".to_string(), l));
                        }
                        _ => break,
                    }
                }
                line = stderr_lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            let entry = format!("[stderr] {}\n", l);
                            let entry_bytes = entry.as_bytes();
                            let _ = log_file.write_all(entry_bytes).await;
                            bytes_written += entry_bytes.len() as u64;
                            if bytes_written >= 50 * 1024 * 1024 {
                                log_file = rotate_log(&log_path_clone, log_file, &mut rotation_index).await;
                                bytes_written = 0;
                            }
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

        if !batch.is_empty() {
            emit_log_chunk(&app_clone, &run_id_clone, batch);
        }
    });

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(timeout_secs);

    let exit_status = tokio::select! {
        result = tokio::time::timeout(timeout, child.wait()) => {
            match result {
                Ok(Ok(status)) => status,
                Ok(Err(e)) => return Err(e.to_string()),
                Err(_) => {
                    // Timeout — child is killed via kill_on_drop
                    let _ = log_task.await;
                    return Err("timed out".to_string());
                }
            }
        }
        _ = cancel => {
            // Cancellation requested — kill child
            let _ = child.kill().await;
            let _ = log_task.await;
            return Err("cancelled".to_string());
        }
    };

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
