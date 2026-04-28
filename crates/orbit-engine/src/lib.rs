//! Orbit core engine.
//!
//! This crate is the host-agnostic core of Orbit: executor, scheduler, plugin
//! manager, database layer, transport shim, command handlers, and supporting
//! modules. It is consumed by both the Tauri desktop app (`orbit` crate at
//! `src-tauri/`) and the future standalone cloud/self-host server
//! (`orbit-server` binary).
//!
//! Tauri-specific bits (the `tauri::Builder`, tray menu, window events,
//! `tauri::generate_handler!`, and `tauri-plugin-*` crates) live in
//! `src-tauri/`. This crate still depends on the `tauri` core crate because
//! `AppContext::tauri` is `Option<tauri::AppHandle>`, `events/emitter.rs`
//! uses the `tauri::Emitter` trait, and `commands/*` carry `#[tauri::command]`
//! annotations — that coupling will be lifted behind a trait in a later phase.

pub mod app_context;
pub mod auth;
pub mod commands;
pub mod db;
pub mod error;
pub mod events;
pub mod executor;
pub mod memory_service;
pub mod models;
pub mod plugins;
pub mod scheduler;
pub mod shim;
pub mod triggers;
pub mod workflows;

use std::path::PathBuf;

/// Wrapper around the live `tauri::AppHandle` registered as managed state in
/// desktop builds. Engine code that needs to walk into `tauri::State<T>`
/// extractors (e.g. trigger spawn, workflow agent nodes) reads this from
/// `tauri::Manager::state` and clones the handle.
///
/// The standalone server never registers this — engine code paths that read
/// it must be guarded by `AppContext::app()` or equivalent.
#[derive(Clone)]
pub struct RuntimeAppHandleState(pub tauri::AppHandle);

/// `~/.orbit` — the on-device data root. Local mode and the standalone server
/// both write under this path; the cloud-deployed engine binds it to a Fly
/// volume at `/data` via the `HOME` env var.
pub fn data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".orbit")
}

/// `~/.orbit/plugins` — installed plugin tree.
pub fn plugins_dir() -> PathBuf {
    plugins::plugins_dir()
}
