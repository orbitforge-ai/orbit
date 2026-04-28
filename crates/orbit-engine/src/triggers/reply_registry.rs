//! Ambient reply-target registry, keyed by run id.
//!
//! When a trigger-driven agent run is spawned, the dispatcher records where
//! the originating message came from — plugin + channel + optional thread —
//! so the `message` tool can reply to that exact location without the agent
//! having to name a channel explicitly.
//!
//! Stored as Tauri-managed state and shared by the trigger dispatcher (writer)
//! and the outbound `message` tool (reader). Entries are removed when the run
//! finishes; if missed (crashed run), they age out with the process.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Where the `message` tool should reply when the caller supplied no explicit
/// channel. Populated by the trigger dispatcher; read by `message.rs`.
#[derive(Debug, Clone)]
pub struct ReplyChannel {
    pub plugin_id: String,
    pub provider_channel_id: String,
    pub provider_thread_id: Option<String>,
}

/// Thread-safe `run_id → ReplyChannel` map.
#[derive(Clone, Default)]
pub struct ReplyRegistry {
    inner: Arc<RwLock<HashMap<String, ReplyChannel>>>,
}

impl ReplyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, run_id: impl Into<String>, reply: ReplyChannel) {
        let mut map = self.inner.write().expect("reply registry poisoned");
        map.insert(run_id.into(), reply);
    }

    pub fn get(&self, run_id: &str) -> Option<ReplyChannel> {
        let map = self.inner.read().expect("reply registry poisoned");
        map.get(run_id).cloned()
    }

    pub fn clear(&self, run_id: &str) {
        let mut map = self.inner.write().expect("reply registry poisoned");
        map.remove(run_id);
    }
}
