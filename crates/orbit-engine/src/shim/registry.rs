//! In-memory dispatch table mapping command names to async adapter closures.
//!
//! Each entry wraps a Tauri command implementation so the HTTP router can
//! invoke it without going through the Tauri IPC runtime. Commands are
//! registered by per-area `register_http` functions in
//! `crate::commands::*`; see `Registry::build` for the full list.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::app_context::AppContext;

pub type AdapterFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;

pub type Adapter =
    Arc<dyn Fn(Arc<AppContext>, Value) -> AdapterFuture + Send + Sync + 'static>;

#[derive(Default, Clone)]
pub struct Registry {
    handlers: HashMap<&'static str, Adapter>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command adapter. Panics on duplicate names to surface
    /// accidental double-registration at startup rather than at request time.
    pub fn register<F, Fut>(&mut self, name: &'static str, handler: F)
    where
        F: Fn(Arc<AppContext>, Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        let adapter: Adapter = Arc::new(move |ctx, args| Box::pin(handler(ctx, args)));
        if self.handlers.insert(name, adapter).is_some() {
            panic!("shim::registry: duplicate command registration: {}", name);
        }
    }

    pub fn get(&self, name: &str) -> Option<&Adapter> {
        self.handlers.get(name)
    }

    pub fn command_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.handlers.keys().copied().collect();
        names.sort();
        names
    }
}

/// Build the registry by calling every per-module `register_http`. New
/// command modules add themselves here.
pub fn build() -> Registry {
    let mut reg = Registry::new();
    crate::commands::auth::register_http(&mut reg);
    crate::commands::projects::register_http(&mut reg);
    crate::commands::tasks::register_http(&mut reg);
    crate::commands::agents::register_http(&mut reg);
    crate::commands::runs::register_http(&mut reg);
    crate::commands::chat::register_http(&mut reg);
    crate::commands::project_workflows::register_http(&mut reg);
    crate::commands::workflow_runs::register_http(&mut reg);
    crate::commands::project_boards::register_http(&mut reg);
    crate::commands::project_board_columns::register_http(&mut reg);
    crate::commands::work_items::register_http(&mut reg);
    crate::commands::work_item_events::register_http(&mut reg);
    crate::commands::schedules::register_http(&mut reg);
    crate::commands::pulse::register_http(&mut reg);
    crate::commands::workspace::register_http(&mut reg);
    crate::commands::llm::register_http(&mut reg);
    crate::commands::bus::register_http(&mut reg);
    crate::commands::skills::register_http(&mut reg);
    crate::commands::permissions::register_http(&mut reg);
    crate::commands::global_settings::register_http(&mut reg);
    crate::commands::users::register_http(&mut reg);
    crate::commands::memory::register_http(&mut reg);
    crate::commands::plugins::register_http(&mut reg);
    crate::commands::triggers::register_http(&mut reg);
    #[cfg(feature = "desktop")]
    crate::commands::terminals::register_http(&mut reg);
    reg
}
