//! Plugin system V1 — local-install integration plugins.
//!
//! Architecture:
//! - MCP stdio subprocess per enabled plugin (`runtime`)
//! - Manifest-declared tools, entity types, OAuth providers, UI, hooks
//! - Generic JSON-blob entity storage (`entities`) gated by manifest schemas
//! - Tauri-state-managed `PluginManager` owns the lifecycle
//!
//! See `docs/plugins/INTERNAL_ARCHITECTURE.md` (added alongside V1) for the
//! extension-recipe reference.

pub mod core_api;
pub mod entities;
pub mod hooks;
pub mod install;
pub mod manifest;
pub mod mcp_client;
pub mod oauth;
pub mod registry;
pub mod runtime;
pub mod tools;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::db::DbPool;
use core_api::CoreApiServer;
use manifest::PluginManifest;
use registry::{PluginRegistry, RegistryEntry};
use runtime::RuntimeRegistry;

/// Current host-API version. Plugins declare a semver range in
/// `plugin.json.hostApiVersion`; manifests whose range does not match this
/// value are rejected at install time.
pub const PLUGIN_HOST_API_VERSION: &str = "1.0.0";

/// Directory where installed plugins live (`~/.orbit/plugins/`).
pub fn plugins_dir() -> PathBuf {
    crate::data_dir().join("plugins")
}

/// Staging area for in-progress installs (`~/.orbit/plugins/.staging/`).
pub fn staging_dir() -> PathBuf {
    plugins_dir().join(".staging")
}

/// Path where a plugin's core-API unix socket lives. The subprocess reads
/// this path from `ORBIT_CORE_API_SOCKET` and dials in via JSON-RPC.
pub fn core_api_socket_path(plugin_id: &str) -> PathBuf {
    plugins_dir()
        .join(plugin_id)
        .join(".orbit")
        .join("core.sock")
}

/// Top-level plugin subsystem. Lives in Tauri managed state and is the only
/// entry point for install/uninstall/enable/disable/reload and tool dispatch.
pub struct PluginManager {
    inner: Arc<RwLock<PluginManagerInner>>,
    pub(crate) runtime: Arc<RuntimeRegistry>,
    pub(crate) oauth_state: Arc<oauth::OAuthState>,
    pub(crate) core_api: Arc<CoreApiServer>,
}

struct PluginManagerInner {
    registry: PluginRegistry,
    manifests: Vec<PluginManifest>,
}

impl PluginManager {
    /// Create a new PluginManager. Loads `registry.json`, parses the manifest
    /// of every installed plugin (dropping any whose manifest is missing or
    /// unparseable), and prepares the runtime registry. Does **not** spawn any
    /// plugin subprocesses — spawning is lazy.
    pub fn init(_db: DbPool) -> Self {
        let plugins_dir = plugins_dir();
        if let Err(e) = std::fs::create_dir_all(&plugins_dir) {
            tracing::warn!("failed to create plugins dir: {}", e);
        }
        if let Err(e) = std::fs::create_dir_all(staging_dir()) {
            tracing::warn!("failed to create plugin staging dir: {}", e);
        }

        // First-launch bootstrap: copy every bundled plugin not yet installed
        // into the user's plugins dir. Bundled plugins are trusted so we skip
        // the staging/review step.
        let mut registry = PluginRegistry::load(&plugins_dir).unwrap_or_else(|e| {
            tracing::warn!(
                "failed to load plugin registry.json ({}); starting empty",
                e
            );
            PluginRegistry::default()
        });
        install::bootstrap_bundled_plugins(&mut registry);
        let _ = registry.save(&plugins_dir);

        let mut manifests = Vec::new();
        let registry = registry;
        for entry in registry.entries() {
            let manifest_path = plugins_dir.join(&entry.id).join("plugin.json");
            match manifest::load_from_path(&manifest_path) {
                Ok(m) => manifests.push(m),
                Err(e) => tracing::warn!(
                    plugin_id = entry.id.as_str(),
                    "failed to parse plugin manifest: {}",
                    e
                ),
            }
        }

        tracing::info!("plugin manager initialised: {} installed", manifests.len());

        Self {
            inner: Arc::new(RwLock::new(PluginManagerInner {
                registry,
                manifests,
            })),
            runtime: Arc::new(RuntimeRegistry::new()),
            oauth_state: Arc::new(oauth::OAuthState::new()),
            core_api: Arc::new(CoreApiServer::new()),
        }
    }

    /// Spawn the unix-socket core-API listener for every enabled plugin.
    /// Called once at startup; reload paths call [`respawn_core_api`] to
    /// pick up manifest changes.
    pub fn start_core_api_servers(&self, db: DbPool) {
        let manifests = self.manifests();
        let core_api = self.core_api.clone();
        tauri::async_runtime::spawn(async move {
            for manifest in manifests {
                if let Err(e) = core_api.start(manifest.clone(), db.clone()).await {
                    tracing::warn!(
                        plugin_id = manifest.id.as_str(),
                        "failed to start core-api socket: {}",
                        e
                    );
                }
            }
        });
    }

    /// Start a single plugin's core-API socket. Used after install/enable so
    /// the subprocess can dial in on first tool call.
    pub fn respawn_core_api(&self, plugin_id: &str, db: DbPool) {
        let Some(manifest) = self.manifest(plugin_id) else {
            return;
        };
        let core_api = self.core_api.clone();
        let plugin_id = plugin_id.to_string();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = core_api.start(manifest, db).await {
                tracing::warn!(plugin_id = %plugin_id, "respawn core-api failed: {}", e);
            }
        });
    }

    /// Hook the runtime log ring to Tauri events so the Plugin detail drawer's
    /// Live Log tab can stream stderr in real time.
    pub fn attach_log_emitter<R: Runtime>(&self, app: &AppHandle<R>) {
        let app = app.clone();
        self.runtime.set_log_event_sender(move |plugin_id, line| {
            let event = format!("plugin:log:{}", plugin_id);
            let _ = app.emit(&event, line);
        });
    }

    /// List a summary of every installed plugin (both enabled and disabled).
    pub fn list(&self) -> Vec<PluginSummary> {
        let inner = self.inner.read().expect("plugin manager lock poisoned");
        inner
            .registry
            .entries()
            .iter()
            .map(|entry| {
                let manifest = inner.manifests.iter().find(|m| m.id == entry.id);
                PluginSummary {
                    id: entry.id.clone(),
                    name: manifest
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| entry.id.clone()),
                    version: manifest.map(|m| m.version.clone()).unwrap_or_default(),
                    description: manifest.and_then(|m| m.description.clone()),
                    enabled: entry.enabled,
                    bundled: entry.bundled,
                    dev: entry.dev,
                    running: self.runtime.is_running(&entry.id),
                }
            })
            .collect()
    }

    /// Parsed manifest for a plugin, if installed.
    pub fn manifest(&self, plugin_id: &str) -> Option<PluginManifest> {
        let inner = self.inner.read().expect("plugin manager lock poisoned");
        inner.manifests.iter().find(|m| m.id == plugin_id).cloned()
    }

    /// Every manifest currently loaded. Used by the tools layer to build the
    /// agent-facing tool catalog.
    pub fn manifests(&self) -> Vec<PluginManifest> {
        let inner = self.inner.read().expect("plugin manager lock poisoned");
        inner.manifests.clone()
    }

    /// Whether a plugin is enabled.
    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        let inner = self.inner.read().expect("plugin manager lock poisoned");
        inner
            .registry
            .entries()
            .iter()
            .any(|e| e.id == plugin_id && e.enabled)
    }

    /// Accept a confirmed install — moves the staging directory into place
    /// and registers the plugin (disabled by default).
    pub fn confirm_install<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        staging_id: &str,
    ) -> Result<PluginManifest, String> {
        let manifest = install::commit_from_staging(staging_id)?;
        self.register_new(app, &manifest, RegistryEntry::new(manifest.id.clone()))?;
        Ok(manifest)
    }

    /// Install a plugin from a directory without copying (dev mode).
    pub fn install_from_directory<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        path: &std::path::Path,
    ) -> Result<PluginManifest, String> {
        let manifest = install::install_from_directory(path)?;
        let mut entry = RegistryEntry::new(manifest.id.clone());
        entry.dev = true;
        self.register_new(app, &manifest, entry)?;
        Ok(manifest)
    }

    fn register_new<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        manifest: &PluginManifest,
        entry: RegistryEntry,
    ) -> Result<(), String> {
        let plugins_dir = plugins_dir();
        let mut inner = self.inner.write().expect("plugin manager lock poisoned");
        inner
            .registry
            .upsert(entry)
            .map_err(|e| format!("registry upsert failed: {}", e))?;
        // Replace any prior manifest for this id.
        inner.manifests.retain(|m| m.id != manifest.id);
        inner.manifests.push(manifest.clone());
        inner
            .registry
            .save(&plugins_dir)
            .map_err(|e| format!("failed to save registry.json: {}", e))?;
        drop(inner);
        let _ = app.emit("plugins:changed", ());
        Ok(())
    }

    /// Toggle a plugin's enabled state. Killing the subprocess on disable.
    pub fn set_enabled<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let plugins_dir = plugins_dir();
        let mut inner = self.inner.write().expect("plugin manager lock poisoned");
        inner
            .registry
            .set_enabled(plugin_id, enabled)
            .map_err(|e| format!("set_enabled failed: {}", e))?;
        inner
            .registry
            .save(&plugins_dir)
            .map_err(|e| format!("failed to save registry.json: {}", e))?;
        drop(inner);

        if !enabled {
            self.runtime.shutdown(plugin_id);
        }
        let _ = app.emit("plugins:changed", ());
        Ok(())
    }

    /// Manual reload — kill subprocess and re-parse manifest. For dev-installed
    /// plugins (`.dev-source` pointer present), re-copy `plugin.json` from the
    /// original source directory first so edits to the working copy are picked
    /// up without a full reinstall.
    pub fn reload<R: Runtime>(&self, app: &AppHandle<R>, plugin_id: &str) -> Result<(), String> {
        self.runtime.shutdown(plugin_id);
        let plugin_dir = plugins_dir().join(plugin_id);
        let pointer = plugin_dir.join(".dev-source");
        if let Ok(source_str) = std::fs::read_to_string(&pointer) {
            let source = std::path::PathBuf::from(source_str.trim());
            let source_manifest = source.join("plugin.json");
            if source_manifest.is_file() {
                std::fs::copy(&source_manifest, plugin_dir.join("plugin.json"))
                    .map_err(|e| format!("reload: copy dev manifest failed: {}", e))?;
            }
        }
        let manifest = manifest::load_from_path(&plugin_dir.join("plugin.json"))
            .map_err(|e| format!("reload failed: {}", e))?;
        let mut inner = self.inner.write().expect("plugin manager lock poisoned");
        inner.manifests.retain(|m| m.id != manifest.id);
        inner.manifests.push(manifest);
        drop(inner);
        let _ = app.emit("plugins:changed", ());
        Ok(())
    }

    /// Reload every enabled plugin.
    pub fn reload_all<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), String> {
        let ids: Vec<String> = {
            let inner = self.inner.read().expect("plugin manager lock poisoned");
            inner
                .registry
                .entries()
                .iter()
                .filter(|e| e.enabled)
                .map(|e| e.id.clone())
                .collect()
        };
        for id in ids {
            if let Err(e) = self.reload(app, &id) {
                tracing::warn!(plugin_id = id, "reload failed: {}", e);
            }
        }
        Ok(())
    }

    /// Uninstall — kill subprocess, delete files and registry entry, wipe
    /// Keychain secrets. **Retains** `plugin_entities` rows per V1 policy.
    pub fn uninstall<R: Runtime>(&self, app: &AppHandle<R>, plugin_id: &str) -> Result<(), String> {
        let plugins_dir = plugins_dir();

        self.runtime.shutdown(plugin_id);

        let mut inner = self.inner.write().expect("plugin manager lock poisoned");
        let was_dev = inner
            .registry
            .entries()
            .iter()
            .find(|e| e.id == plugin_id)
            .map(|e| e.dev)
            .unwrap_or(false);
        inner.registry.remove(plugin_id);
        inner.manifests.retain(|m| m.id != plugin_id);
        inner
            .registry
            .save(&plugins_dir)
            .map_err(|e| format!("failed to save registry.json: {}", e))?;
        drop(inner);

        let dir = plugins_dir.join(plugin_id);
        if dir.exists() {
            if was_dev {
                // Dev installs are typically symlinks or pointer files. Remove
                // the pointer only; never touch the source directory.
                if dir.is_symlink() {
                    let _ = std::fs::remove_file(&dir);
                } else {
                    let _ = std::fs::remove_dir_all(&dir);
                }
            } else {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }

        oauth::wipe_plugin_secrets(plugin_id);
        let _ = app.emit("plugins:changed", ());
        let _ = app.emit("plugin:uninstalled", plugin_id.to_string());
        Ok(())
    }
}

/// Public summary used by the Tauri command layer + frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub bundled: bool,
    pub dev: bool,
    pub running: bool,
}

/// Access the `PluginManager` from Tauri managed state.
pub fn from_state<R: Runtime>(app: &AppHandle<R>) -> Arc<PluginManager> {
    app.state::<Arc<PluginManager>>().inner().clone()
}
