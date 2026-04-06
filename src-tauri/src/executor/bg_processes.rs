use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

const MAX_BG_LOG_BYTES: usize = 1_000_000;
const MAX_POLL_OUTPUT_BYTES: usize = 50_000;
const BG_PROCESS_CLEANUP_SECS: u64 = 300;

#[derive(Clone)]
pub struct BgProcessRegistry {
    entries: Arc<Mutex<HashMap<String, Arc<BgProcessEntry>>>>,
}

struct BgProcessEntry {
    process_id: String,
    agent_id: String,
    command: String,
    pid: u32,
    output_path: PathBuf,
    started_at: String,
    status: Mutex<BgProcessStatus>,
}

struct BgProcessStatus {
    running: bool,
    exit_code: Option<i32>,
    finished_at: Option<String>,
    cleanup_at: Option<tokio::time::Instant>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgProcessSummary {
    pub process_id: String,
    pub command: String,
    pub pid: u32,
    pub output_path: String,
    pub running: bool,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgProcessPollResult {
    pub process_id: String,
    pub command: String,
    pub pid: u32,
    pub output_path: String,
    pub running: bool,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgProcessKillResult {
    pub process_id: String,
    pub command: String,
    pub pid: u32,
    pub running: bool,
    pub message: String,
}

impl BgProcessRegistry {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn spawn(
        &self,
        agent_id: &str,
        command: &str,
        workspace_root: &Path,
        bg_root: &Path,
    ) -> Result<BgProcessSummary, String> {
        tokio::fs::create_dir_all(bg_root)
            .await
            .map_err(|e| format!("failed to create bg process directory: {}", e))?;

        self.cleanup_finished().await;

        let process_id = ulid::Ulid::new().to_string();
        let output_path = bg_root.join(format!("{}.log", process_id));
        let mut child = tokio::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .current_dir(workspace_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false)
            .spawn()
            .map_err(|e| format!("failed to spawn background command: {}", e))?;

        let pid = child
            .id()
            .ok_or("background process did not provide a pid")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("background process stdout was not captured")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("background process stderr was not captured")?;
        let started_at = chrono::Utc::now().to_rfc3339();

        let entry = Arc::new(BgProcessEntry {
            process_id: process_id.clone(),
            agent_id: agent_id.to_string(),
            command: command.to_string(),
            pid,
            output_path: output_path.clone(),
            started_at: started_at.clone(),
            status: Mutex::new(BgProcessStatus {
                running: true,
                exit_code: None,
                finished_at: None,
                cleanup_at: None,
            }),
        });

        let entry_for_task = Arc::clone(&entry);
        tokio::spawn(async move {
            let output_task = tokio::spawn(capture_background_output(stdout, stderr, output_path));
            let wait_result = child.wait().await;
            let _ = output_task.await;

            let mut status = entry_for_task.status.lock().await;
            status.running = false;
            status.exit_code = wait_result
                .ok()
                .and_then(|result| result.code())
                .or(Some(-1));
            status.finished_at = Some(chrono::Utc::now().to_rfc3339());
            status.cleanup_at =
                Some(tokio::time::Instant::now() + Duration::from_secs(BG_PROCESS_CLEANUP_SECS));
        });

        self.entries
            .lock()
            .await
            .insert(process_id, Arc::clone(&entry));
        Ok(entry.summary().await)
    }

    pub async fn list(&self, agent_id: &str) -> Vec<BgProcessSummary> {
        self.cleanup_finished().await;

        let entries = {
            let entries = self.entries.lock().await;
            entries.values().cloned().collect::<Vec<_>>()
        };

        let mut summaries = Vec::new();
        for entry in entries {
            if entry.agent_id != agent_id {
                continue;
            }
            summaries.push(entry.summary().await);
        }
        summaries.sort_by(|left, right| right.started_at.cmp(&left.started_at));
        summaries
    }

    pub async fn poll(
        &self,
        agent_id: &str,
        process_id: &str,
    ) -> Result<BgProcessPollResult, String> {
        self.cleanup_finished().await;
        let entry = self
            .get_owned_entry(agent_id, process_id)
            .await?
            .ok_or_else(|| format!("background process '{}' not found", process_id))?;
        let status = entry.status.lock().await;
        let running = status.running;
        let finished_at = status.finished_at.clone();
        let exit_code = status.exit_code;
        drop(status);

        Ok(BgProcessPollResult {
            process_id: entry.process_id.clone(),
            command: entry.command.clone(),
            pid: entry.pid,
            output_path: entry.output_path.display().to_string(),
            running,
            started_at: entry.started_at.clone(),
            finished_at,
            exit_code,
            output: tail_text_file(&entry.output_path, MAX_POLL_OUTPUT_BYTES)
                .await
                .unwrap_or_default(),
        })
    }

    pub async fn kill(
        &self,
        agent_id: &str,
        process_id: &str,
    ) -> Result<BgProcessKillResult, String> {
        self.cleanup_finished().await;
        let entry = self
            .get_owned_entry(agent_id, process_id)
            .await?
            .ok_or_else(|| format!("background process '{}' not found", process_id))?;

        let status_before = entry.status.lock().await.running;
        if !status_before {
            return Ok(BgProcessKillResult {
                process_id: entry.process_id.clone(),
                command: entry.command.clone(),
                pid: entry.pid,
                running: false,
                message: "Process already exited.".to_string(),
            });
        }

        let term = tokio::process::Command::new("/bin/kill")
            .arg(entry.pid.to_string())
            .status()
            .await
            .map_err(|e| format!("failed to send termination signal: {}", e))?;

        if !term.success() {
            return Err(format!(
                "failed to terminate background process '{}'",
                process_id
            ));
        }

        Ok(BgProcessKillResult {
            process_id: entry.process_id.clone(),
            command: entry.command.clone(),
            pid: entry.pid,
            running: true,
            message: "Termination signal sent.".to_string(),
        })
    }

    async fn cleanup_finished(&self) {
        let entries = {
            let entries = self.entries.lock().await;
            entries
                .iter()
                .map(|(process_id, entry)| (process_id.clone(), Arc::clone(entry)))
                .collect::<Vec<_>>()
        };

        let mut stale_ids = Vec::new();
        for (process_id, entry) in entries {
            let status = entry.status.lock().await;
            if status.running {
                continue;
            }
            if let Some(cleanup_at) = status.cleanup_at {
                if tokio::time::Instant::now() >= cleanup_at {
                    stale_ids.push(process_id);
                }
            }
        }

        if stale_ids.is_empty() {
            return;
        }

        let mut entries = self.entries.lock().await;
        for process_id in stale_ids {
            if let Some(entry) = entries.remove(&process_id) {
                let _ = tokio::fs::remove_file(&entry.output_path).await;
            }
        }
    }

    async fn get_owned_entry(
        &self,
        agent_id: &str,
        process_id: &str,
    ) -> Result<Option<Arc<BgProcessEntry>>, String> {
        let entry = {
            let entries = self.entries.lock().await;
            entries.get(process_id).cloned()
        };

        match entry {
            Some(entry) if entry.agent_id == agent_id => Ok(Some(entry)),
            Some(_) => Err("background process belongs to a different agent".to_string()),
            None => Ok(None),
        }
    }
}

impl BgProcessEntry {
    async fn summary(&self) -> BgProcessSummary {
        let status = self.status.lock().await;
        BgProcessSummary {
            process_id: self.process_id.clone(),
            command: self.command.clone(),
            pid: self.pid,
            output_path: self.output_path.display().to_string(),
            running: status.running,
            started_at: self.started_at.clone(),
            finished_at: status.finished_at.clone(),
            exit_code: status.exit_code,
        }
    }
}

async fn capture_background_output(
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    output_path: PathBuf,
) {
    if let Some(parent) = output_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stderr_lines = BufReader::new(stderr).lines();
    let mut log_file = match tokio::fs::File::create(&output_path).await {
        Ok(file) => file,
        Err(_) => return,
    };
    let mut bytes_written = 0usize;
    let mut stdout_done = false;
    let mut stderr_done = false;

    while !stdout_done || !stderr_done {
        tokio::select! {
            line = stdout_lines.next_line(), if !stdout_done => {
                match line {
                    Ok(Some(line)) => {
                        if write_log_line(&mut log_file, &output_path, &line, &mut bytes_written).await.is_err() {
                            break;
                        }
                    }
                    _ => stdout_done = true,
                }
            }
            line = stderr_lines.next_line(), if !stderr_done => {
                match line {
                    Ok(Some(line)) => {
                        let line = format!("[stderr] {}", line);
                        if write_log_line(&mut log_file, &output_path, &line, &mut bytes_written).await.is_err() {
                            break;
                        }
                    }
                    _ => stderr_done = true,
                }
            }
        }
    }
}

async fn write_log_line(
    log_file: &mut tokio::fs::File,
    output_path: &Path,
    line: &str,
    bytes_written: &mut usize,
) -> Result<(), String> {
    if *bytes_written >= MAX_BG_LOG_BYTES {
        *log_file = tokio::fs::File::create(output_path)
            .await
            .map_err(|e| e.to_string())?;
        *bytes_written = 0;
        let notice = "[previous background output truncated]\n";
        log_file
            .write_all(notice.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        *bytes_written += notice.len();
    }

    let entry = format!("{}\n", line);
    log_file
        .write_all(entry.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    *bytes_written += entry.len();
    Ok(())
}

async fn tail_text_file(path: &Path, max_bytes: usize) -> Result<String, String> {
    let content = tokio::fs::read(path)
        .await
        .map_err(|e| format!("failed to read process output: {}", e))?;

    if content.len() <= max_bytes {
        return Ok(String::from_utf8_lossy(&content).into_owned());
    }

    let start = content.len() - max_bytes;
    Ok(format!(
        "[output truncated to last {} bytes]\n{}",
        max_bytes,
        String::from_utf8_lossy(&content[start..])
    ))
}
