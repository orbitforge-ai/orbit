//! HTTP+WebSocket shim exposing Tauri commands and events to remote clients.
//!
//! In desktop builds the shim runs alongside the Tauri window so a browser
//! tab can talk to the same backend for development. The architecture is
//! identical for a future standalone cloud server: construct an
//! `AppContext` without `tauri::AppHandle`, call `shim::start`.

pub mod auth;
pub mod registry;
pub mod router;
pub mod static_files;
pub mod ws;

pub use router::start;
