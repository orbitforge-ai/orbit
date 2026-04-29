use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::Emitter;
use tracing::warn;

/// Runtime services that differ between desktop Tauri and the headless server.
///
/// Engine code should prefer this boundary for event emission and only ask for
/// a Tauri handle when it is calling a path that has not been made host-neutral
/// yet.
pub trait RuntimeHost: Send + Sync {
    fn emit_json(&self, event: &'static str, payload: Value);

    fn app_handle(&self) -> Option<tauri::AppHandle> {
        None
    }
}

pub type RuntimeHostHandle = Arc<dyn RuntimeHost>;

#[derive(Clone)]
pub struct TauriRuntimeHost {
    app: tauri::AppHandle,
}

impl TauriRuntimeHost {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl RuntimeHost for TauriRuntimeHost {
    fn emit_json(&self, event: &'static str, payload: Value) {
        if let Err(e) = self.app.emit(event, &payload) {
            warn!("failed to emit {event}: {e}");
        }
        crate::shim::ws::broadcast(event, &payload);
    }

    fn app_handle(&self) -> Option<tauri::AppHandle> {
        Some(self.app.clone())
    }
}

#[derive(Clone, Default)]
pub struct HeadlessRuntimeHost;

impl RuntimeHost for HeadlessRuntimeHost {
    fn emit_json(&self, event: &'static str, payload: Value) {
        crate::shim::ws::broadcast(event, &payload);
    }
}

pub fn tauri_host(app: tauri::AppHandle) -> RuntimeHostHandle {
    Arc::new(TauriRuntimeHost::new(app))
}

pub fn headless_host() -> RuntimeHostHandle {
    Arc::new(HeadlessRuntimeHost)
}

pub fn emit_serialized<T: Serialize + ?Sized>(
    host: &dyn RuntimeHost,
    event: &'static str,
    payload: &T,
) {
    match serde_json::to_value(payload) {
        Ok(value) => host.emit_json(event, value),
        Err(e) => warn!("failed to serialize {event} payload: {e}"),
    }
}
