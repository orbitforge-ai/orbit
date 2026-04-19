//! MCP-stdio subprocess lifecycle, per-plugin mutex, log ring buffer, manual
//! reload. This module is intentionally kept to the minimum shape the rest of
//! the plugin system needs to compile and function end-to-end; the full MCP
//! JSON-RPC client is added in a follow-up slice.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Ring-buffer capacity for per-plugin stderr tail (exposed to the Plugin
/// detail drawer's Live Log tab).
const LOG_RING_CAPACITY: usize = 1000;

/// Public runtime status reported to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeStatus {
    Idle,
    Running,
    Error,
}

/// Holds the per-plugin handle and log ring. V1's scope here is the log ring
/// and shutdown signal; the actual subprocess + MCP framing is plugged in by
/// the tools layer in a follow-up slice.
#[derive(Default)]
pub struct RuntimeRegistry {
    inner: Arc<Mutex<std::collections::HashMap<String, RuntimeSlot>>>,
}

pub struct RuntimeSlot {
    pub status: RuntimeStatus,
    pub log_ring: VecDeque<String>,
}

impl Default for RuntimeSlot {
    fn default() -> Self {
        Self {
            status: RuntimeStatus::Idle,
            log_ring: VecDeque::with_capacity(LOG_RING_CAPACITY),
        }
    }
}

impl RuntimeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a subprocess is marked running. When the full MCP client lands,
    /// this becomes "the child process handle is still alive".
    pub fn is_running(&self, plugin_id: &str) -> bool {
        let map = self.inner.lock().expect("runtime registry poisoned");
        map.get(plugin_id)
            .map(|s| s.status == RuntimeStatus::Running)
            .unwrap_or(false)
    }

    pub fn status(&self, plugin_id: &str) -> RuntimeStatus {
        let map = self.inner.lock().expect("runtime registry poisoned");
        map.get(plugin_id)
            .map(|s| s.status)
            .unwrap_or(RuntimeStatus::Idle)
    }

    /// Append a stderr line to the per-plugin ring buffer. Called from the
    /// subprocess reader once it lands; safe to call now for test purposes.
    pub fn push_log(&self, plugin_id: &str, line: String) {
        let mut map = self.inner.lock().expect("runtime registry poisoned");
        let slot = map.entry(plugin_id.to_string()).or_default();
        if slot.log_ring.len() == LOG_RING_CAPACITY {
            slot.log_ring.pop_front();
        }
        slot.log_ring.push_back(line);
    }

    /// Return the last `tail_lines` lines of log for a plugin.
    pub fn log_tail(&self, plugin_id: &str, tail_lines: usize) -> String {
        let map = self.inner.lock().expect("runtime registry poisoned");
        let Some(slot) = map.get(plugin_id) else {
            return String::new();
        };
        let take = slot.log_ring.len().min(tail_lines);
        let start = slot.log_ring.len() - take;
        slot.log_ring
            .iter()
            .skip(start)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Mark the plugin as idle and drop its slot. The real subprocess kill
    /// path is added in the follow-up slice; today this clears state so
    /// reload / disable / uninstall are observable.
    pub fn shutdown(&self, plugin_id: &str) {
        let mut map = self.inner.lock().expect("runtime registry poisoned");
        map.remove(plugin_id);
    }

    /// Mark as running (used by tests + forthcoming subprocess supervisor).
    #[allow(dead_code)]
    pub fn mark_running(&self, plugin_id: &str) {
        let mut map = self.inner.lock().expect("runtime registry poisoned");
        let slot = map.entry(plugin_id.to_string()).or_default();
        slot.status = RuntimeStatus::Running;
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
        assert_eq!(lines.last().unwrap(), &format!("line {}", LOG_RING_CAPACITY + 49));
    }

    #[test]
    fn running_flag_flips_on_shutdown() {
        let reg = RuntimeRegistry::new();
        reg.mark_running("com.orbit.x");
        assert!(reg.is_running("com.orbit.x"));
        reg.shutdown("com.orbit.x");
        assert!(!reg.is_running("com.orbit.x"));
    }
}
