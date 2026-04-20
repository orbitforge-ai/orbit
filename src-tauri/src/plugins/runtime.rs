//! Subprocess lifecycle + log ring + per-plugin mutex. Wraps the low-level
//! `mcp_client::McpClient` with the caching, locking, and per-plugin state
//! the rest of the plugin system needs.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex as SyncMutex};

use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{info, warn};

use super::install;
use super::manifest::PluginManifest;
use super::mcp_client::{LaunchSpec, McpClient};

const LOG_RING_CAPACITY: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeStatus {
    Idle,
    Running,
    Error,
}

pub struct RuntimeRegistry {
    logs: Arc<SyncMutex<HashMap<String, VecDeque<String>>>>,
    /// Sync mutex: the map itself is quick to read/write; per-entry async
    /// mutexes live inside `PluginClient` for the actual subprocess I/O.
    clients: Arc<SyncMutex<HashMap<String, Arc<PluginClient>>>>,
    event_sender: Arc<SyncMutex<Option<Box<dyn Fn(String, String) + Send + Sync>>>>,
}

struct PluginClient {
    /// Serialize concurrent tool calls for a single plugin so the subprocess
    /// never sees interleaved writes.
    lock: AsyncMutex<()>,
    client: AsyncMutex<Option<McpClient>>,
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeRegistry {
    pub fn new() -> Self {
        Self {
            logs: Arc::new(SyncMutex::new(HashMap::new())),
            clients: Arc::new(SyncMutex::new(HashMap::new())),
            event_sender: Arc::new(SyncMutex::new(None)),
        }
    }

    /// Register an emitter that forwards each stderr line to a Tauri event
    /// (`plugin:log:<id>`). Called once during `PluginManager::init`.
    pub fn set_log_event_sender<F>(&self, sender: F)
    where
        F: Fn(String, String) + Send + Sync + 'static,
    {
        let mut slot = self.event_sender.lock().expect("log sender poisoned");
        *slot = Some(Box::new(sender));
    }

    pub fn is_running(&self, plugin_id: &str) -> bool {
        let clients = self.clients.lock().expect("runtime clients poisoned");
        clients.contains_key(plugin_id)
    }

    pub fn status(&self, plugin_id: &str) -> RuntimeStatus {
        if self.is_running(plugin_id) {
            RuntimeStatus::Running
        } else {
            RuntimeStatus::Idle
        }
    }

    pub fn push_log(&self, plugin_id: &str, line: String) {
        {
            let mut logs = self.logs.lock().expect("runtime logs poisoned");
            let ring = logs.entry(plugin_id.to_string()).or_default();
            if ring.len() == LOG_RING_CAPACITY {
                ring.pop_front();
            }
            ring.push_back(line.clone());
        }
        let sender = self.event_sender.lock().expect("log sender poisoned");
        if let Some(send) = sender.as_ref() {
            send(plugin_id.to_string(), line);
        }
    }

    pub fn log_tail(&self, plugin_id: &str, tail_lines: usize) -> String {
        let logs = self.logs.lock().expect("runtime logs poisoned");
        let Some(ring) = logs.get(plugin_id) else {
            return String::new();
        };
        let take = ring.len().min(tail_lines);
        let start = ring.len() - take;
        ring.iter()
            .skip(start)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Synchronous shutdown — marks state idle and queues the kill. Used by
    /// disable / reload / uninstall paths where we cannot block on async.
    pub fn shutdown(&self, plugin_id: &str) {
        let client_slot = {
            let mut clients = self.clients.lock().expect("runtime clients poisoned");
            clients.remove(plugin_id)
        };
        if let Some(slot) = client_slot {
            tauri::async_runtime::spawn(async move {
                let inner = {
                    let mut guard = slot.client.lock().await;
                    guard.take()
                };
                if let Some(client) = inner {
                    client.shutdown().await;
                }
            });
        }
    }

    /// Call a plugin tool. Lazy-spawns the subprocess on first call.
    pub async fn call_tool(
        &self,
        manifest: &PluginManifest,
        tool_name: &str,
        arguments: &Value,
        extra_env: &std::collections::BTreeMap<String, String>,
    ) -> Result<Value, String> {
        let plugin_id = manifest.id.as_str();
        let slot = self.get_or_create_slot(plugin_id).await;
        let _call_guard = slot.lock.lock().await;

        {
            let guard = slot.client.lock().await;
            if guard.is_none() {
                drop(guard);
                self.ensure_spawned(&slot, manifest, extra_env).await?;
            }
        }

        let result = {
            let guard = slot.client.lock().await;
            let Some(client) = guard.as_ref() else {
                return Err("plugin subprocess not initialised".to_string());
            };
            client.call_tool(tool_name, arguments).await
        };

        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                // Kill the subprocess on error so the next call respawns.
                warn!(plugin_id, tool_name, "plugin tool error: {}", e);
                let mut guard = slot.client.lock().await;
                if let Some(client) = guard.take() {
                    client.shutdown().await;
                }
                Err(e)
            }
        }
    }

    /// Return `tools/list` for a plugin (spawning if necessary).
    pub async fn list_tools(
        &self,
        manifest: &PluginManifest,
        extra_env: &std::collections::BTreeMap<String, String>,
    ) -> Result<Value, String> {
        let slot = self.get_or_create_slot(&manifest.id).await;
        let _call_guard = slot.lock.lock().await;
        {
            let guard = slot.client.lock().await;
            if guard.is_none() {
                drop(guard);
                self.ensure_spawned(&slot, manifest, extra_env).await?;
            }
        }
        let guard = slot.client.lock().await;
        let Some(client) = guard.as_ref() else {
            return Err("plugin subprocess not initialised".to_string());
        };
        client.list_tools().await
    }

    async fn get_or_create_slot(&self, plugin_id: &str) -> Arc<PluginClient> {
        let mut clients = self.clients.lock().expect("runtime clients poisoned");
        clients
            .entry(plugin_id.to_string())
            .or_insert_with(|| {
                Arc::new(PluginClient {
                    lock: AsyncMutex::new(()),
                    client: AsyncMutex::new(None),
                })
            })
            .clone()
    }

    async fn ensure_spawned(
        &self,
        slot: &Arc<PluginClient>,
        manifest: &PluginManifest,
        extra_env: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), String> {
        let source_dir = install::resolve_source_dir(&manifest.id);
        let working_dir = manifest
            .runtime
            .working_dir
            .as_ref()
            .map(|w| source_dir.join(w))
            .unwrap_or(source_dir.clone());

        let mut env = manifest.runtime.env.clone();
        // Baseline env every plugin gets.
        env.insert("ORBIT_PLUGIN_ID".into(), manifest.id.clone());
        env.insert(
            "ORBIT_PLUGIN_DATA_DIR".into(),
            source_dir.to_string_lossy().to_string(),
        );
        // Passthrough env injected by the caller (OAuth tokens, core-api
        // socket path) overrides anything the manifest set — tokens win.
        for (k, v) in extra_env {
            env.insert(k.clone(), v.clone());
        }
        // Keep PATH so common interpreters (`node`, `python`) resolve.
        if let Ok(path) = std::env::var("PATH") {
            env.insert("PATH".into(), path);
        }
        // Home is required by many runtimes.
        if let Ok(home) = std::env::var("HOME") {
            env.insert("HOME".into(), home);
        }

        let spec = LaunchSpec {
            plugin_id: manifest.id.clone(),
            command: manifest.runtime.command.clone(),
            args: manifest.runtime.args.clone(),
            working_dir,
            env,
        };

        let logs = self.logs.clone();
        let sender = self.event_sender.clone();
        let log_sink: std::sync::Arc<dyn Fn(&str, String) + Send + Sync> =
            std::sync::Arc::new(move |plugin_id: &str, line: String| {
                {
                    let mut map = logs.lock().expect("logs poisoned");
                    let ring = map.entry(plugin_id.to_string()).or_default();
                    if ring.len() == LOG_RING_CAPACITY {
                        ring.pop_front();
                    }
                    ring.push_back(line.clone());
                }
                if let Ok(guard) = sender.lock() {
                    if let Some(send) = guard.as_ref() {
                        send(plugin_id.to_string(), line);
                    }
                }
            });

        info!(
            plugin_id = manifest.id.as_str(),
            "spawning plugin subprocess"
        );
        let client = McpClient::spawn(spec, log_sink).await?;

        let mut guard = slot.client.lock().await;
        *guard = Some(client);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_ring_bounds_size() {
        let reg = RuntimeRegistry::new();
        for i in 0..(LOG_RING_CAPACITY + 50) {
            reg.push_log("com.orbit.x", format!("line {}", i));
        }
        let tail = reg.log_tail("com.orbit.x", LOG_RING_CAPACITY + 100);
        let lines: Vec<&str> = tail.split('\n').collect();
        assert_eq!(lines.len(), LOG_RING_CAPACITY);
        assert_eq!(
            lines.last().unwrap(),
            &format!("line {}", LOG_RING_CAPACITY + 49)
        );
    }
}
