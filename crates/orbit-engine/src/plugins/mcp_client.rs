//! Minimal MCP stdio client.
//!
//! The Model Context Protocol uses JSON-RPC 2.0 framed over line-delimited
//! stdio (`Content-Length` framing is optional in the spec; the common SDK
//! uses newline-delimited JSON, which is what we implement). This client
//! covers the subset V1 needs:
//!   - `initialize`
//!   - `tools/list`
//!   - `tools/call` (with streaming `$/progress` notifications)
//!   - graceful shutdown
//!
//! Each enabled plugin owns one `McpClient` wrapping one subprocess. The
//! subprocess is lazy-spawned on first call and torn down on disable /
//! reload / uninstall via `shutdown()`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{oneshot, Mutex};
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

const JSONRPC_VERSION: &str = "2.0";
const PROTOCOL_VERSION: &str = "2024-11-05";
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Handle to a running plugin MCP subprocess.
pub struct McpClient {
    plugin_id: String,
    state: Arc<McpState>,
}

struct McpState {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    pending: Mutex<HashMap<i64, oneshot::Sender<JsonRpcResponse>>>,
    next_id: AtomicI64,
}

#[derive(Debug, Clone, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'a str,
    id: i64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

/// Subprocess launch spec — pulled from the manifest's `runtime` block plus
/// env vars the caller wants to inject (OAuth tokens, core-api socket path).
pub struct LaunchSpec {
    pub plugin_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: std::path::PathBuf,
    pub env: std::collections::BTreeMap<String, String>,
}

impl McpClient {
    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// Spawn the subprocess, run the `initialize` handshake, return a ready
    /// client. Stderr is streamed into `log_sink` so the runtime log buffer
    /// can capture it.
    pub async fn spawn(
        spec: LaunchSpec,
        log_sink: Arc<dyn Fn(&str, String) + Send + Sync>,
    ) -> Result<Self, String> {
        let mut cmd = tokio::process::Command::new(&spec.command);
        cmd.args(&spec.args)
            .current_dir(&spec.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Explicit env: never inherit the user's shell env. The caller is
        // responsible for passing every var the subprocess needs.
        cmd.env_clear();
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn {:?} failed: {}", spec.command, e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "subprocess stdin unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "subprocess stdout unavailable".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "subprocess stderr unavailable".to_string())?;

        let state = Arc::new(McpState {
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(Some(stdin)),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
        });

        // Reader task: parses stdout, matches responses to pending waiters,
        // drops notifications (progress + unsolicited) for now.
        {
            let state = state.clone();
            let plugin_id = spec.plugin_id.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            if line.trim().is_empty() {
                                continue;
                            }
                            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                                debug!(plugin_id = plugin_id.as_str(), "non-JSON stdout: {}", line);
                                continue;
                            };
                            let Some(id) = value.get("id").and_then(Value::as_i64) else {
                                // Notification (no id) — progress or other.
                                continue;
                            };
                            let response: JsonRpcResponse =
                                match serde_json::from_value(value.clone()) {
                                    Ok(r) => r,
                                    Err(_) => continue,
                                };
                            let mut pending = state.pending.lock().await;
                            if let Some(sender) = pending.remove(&id) {
                                let _ = sender.send(response);
                            }
                        }
                        Ok(None) => {
                            // stdout closed — subprocess exiting.
                            break;
                        }
                        Err(e) => {
                            warn!(plugin_id = plugin_id.as_str(), "stdout read error: {}", e);
                            break;
                        }
                    }
                }
            });
        }

        // stderr -> log sink (line-by-line into the per-plugin ring buffer).
        {
            let plugin_id = spec.plugin_id.clone();
            let sink = log_sink.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    sink(&plugin_id, line);
                }
            });
        }

        let client = Self {
            plugin_id: spec.plugin_id,
            state,
        };

        client.initialize().await?;

        Ok(client)
    }

    async fn initialize(&self) -> Result<(), String> {
        let params = serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "clientInfo": {
                "name": "orbit",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });
        let fut = self.request("initialize", params);
        let response = timeout(INITIALIZE_TIMEOUT, fut)
            .await
            .map_err(|_| "MCP initialize timed out".to_string())??;
        if response.error.is_some() {
            return Err(format!(
                "MCP initialize failed: {:?}",
                response.error.unwrap().message
            ));
        }
        // MCP requires a `notifications/initialized` after the response.
        self.notify("notifications/initialized", Value::Null)
            .await?;
        Ok(())
    }

    /// Call `tools/list`. Returns the raw `result` payload (an object with a
    /// `tools` array) so callers can introspect the full shape.
    pub async fn list_tools(&self) -> Result<Value, String> {
        let response = self.request("tools/list", Value::Null).await?;
        if let Some(err) = response.error {
            return Err(format!("tools/list failed: {}", err.message));
        }
        Ok(response.result.unwrap_or(Value::Null))
    }

    /// Call `tools/call` with a given tool name and arguments. Returns the
    /// raw `result` payload (`content` array, `isError` flag).
    pub async fn call_tool(&self, name: &str, arguments: &Value) -> Result<Value, String> {
        let params = serde_json::json!({ "name": name, "arguments": arguments });
        let fut = self.request("tools/call", params);
        let response = timeout(DEFAULT_CALL_TIMEOUT, fut)
            .await
            .map_err(|_| format!("tools/call {:?} timed out", name))??;
        if let Some(err) = response.error {
            return Err(format!("tools/call {:?} failed: {}", name, err.message));
        }
        Ok(response.result.unwrap_or(Value::Null))
    }

    async fn request(&self, method: &str, params: Value) -> Result<JsonRpcResponse, String> {
        let id = self.state.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.state.pending.lock().await;
            pending.insert(id, tx);
        }
        let req = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION,
            id,
            method,
            params,
        };
        let mut frame =
            serde_json::to_string(&req).map_err(|e| format!("failed to encode request: {}", e))?;
        frame.push('\n');
        {
            let mut stdin = self.state.stdin.lock().await;
            let Some(pipe) = stdin.as_mut() else {
                return Err("subprocess stdin closed".to_string());
            };
            pipe.write_all(frame.as_bytes())
                .await
                .map_err(|e| format!("write to subprocess: {}", e))?;
            pipe.flush()
                .await
                .map_err(|e| format!("flush subprocess: {}", e))?;
        }
        rx.await
            .map_err(|_| "subprocess response channel dropped".to_string())
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let frame_value = serde_json::json!({
            "jsonrpc": JSONRPC_VERSION,
            "method": method,
            "params": params,
        });
        let mut frame = frame_value.to_string();
        frame.push('\n');
        let mut stdin = self.state.stdin.lock().await;
        let Some(pipe) = stdin.as_mut() else {
            return Err("subprocess stdin closed".to_string());
        };
        pipe.write_all(frame.as_bytes())
            .await
            .map_err(|e| format!("write notify: {}", e))?;
        pipe.flush()
            .await
            .map_err(|e| format!("flush notify: {}", e))?;
        Ok(())
    }

    /// Kill the subprocess. Idempotent.
    pub async fn shutdown(self) {
        if let Some(mut child) = self.state.child.lock().await.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        let mut stdin = self.state.stdin.lock().await;
        *stdin = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_missing_binary_returns_err() {
        let sink: Arc<dyn Fn(&str, String) + Send + Sync> = Arc::new(|_: &str, _: String| {});
        let spec = LaunchSpec {
            plugin_id: "com.orbit.test".into(),
            command: "/nonexistent-binary-path-12345".into(),
            args: vec![],
            working_dir: std::env::temp_dir(),
            env: Default::default(),
        };
        let result = McpClient::spawn(spec, sink).await;
        assert!(result.is_err());
    }
}
