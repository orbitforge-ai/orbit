//! `~/.orbit/plugins/registry.json` — the source of truth for which plugins
//! are installed on this device and whether they are enabled.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

const REGISTRY_FILENAME: &str = "registry.json";
const REGISTRY_SCHEMA_VERSION: u32 = 1;

/// One entry per installed plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    /// Shipped with the app bundle (e.g. `com.orbit.github`). Affects the
    /// uninstall flow and is displayed as a badge in the UI.
    #[serde(default)]
    pub bundled: bool,
    /// Installed via `install_from_directory` — symlink/pointer rather than
    /// a file copy. Uninstall never touches the source tree.
    #[serde(default)]
    pub dev: bool,
    #[serde(default)]
    pub installed_at: String,
}

impl RegistryEntry {
    pub fn new(id: String) -> Self {
        Self {
            id,
            enabled: false,
            bundled: false,
            dev: false,
            installed_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegistry {
    #[serde(default = "default_version")]
    pub schema_version: u32,
    #[serde(default)]
    entries: Vec<RegistryEntry>,
}

fn default_version() -> u32 {
    REGISTRY_SCHEMA_VERSION
}

impl PluginRegistry {
    pub fn entries(&self) -> &[RegistryEntry] {
        &self.entries
    }

    /// Load from `<plugins_dir>/registry.json`, or return an empty registry
    /// when the file does not yet exist.
    pub fn load(plugins_dir: &Path) -> Result<Self, String> {
        let path = plugins_dir.join(REGISTRY_FILENAME);
        if !path.exists() {
            return Ok(Self {
                schema_version: REGISTRY_SCHEMA_VERSION,
                entries: Vec::new(),
            });
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        let mut parsed: Self = serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse {}: {}", path.display(), e))?;
        if parsed.schema_version == 0 {
            parsed.schema_version = REGISTRY_SCHEMA_VERSION;
        }
        Ok(parsed)
    }

    /// Atomically save to `<plugins_dir>/registry.json`.
    pub fn save(&self, plugins_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(plugins_dir)
            .map_err(|e| format!("failed to create {}: {}", plugins_dir.display(), e))?;
        let path = plugins_dir.join(REGISTRY_FILENAME);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize registry: {}", e))?;
        fs::write(&tmp, &json).map_err(|e| format!("failed to write registry tmp: {}", e))?;
        fs::rename(&tmp, &path).map_err(|e| format!("failed to finalise registry: {}", e))?;
        Ok(())
    }

    /// Insert or replace an entry.
    pub fn upsert(&mut self, entry: RegistryEntry) -> Result<(), String> {
        if entry.id.is_empty() {
            return Err("registry entry id must not be empty".into());
        }
        if let Some(existing) = self.entries.iter_mut().find(|e| e.id == entry.id) {
            let installed_at = existing.installed_at.clone();
            *existing = entry;
            if existing.installed_at.is_empty() {
                existing.installed_at = installed_at;
            }
        } else {
            self.entries.push(entry);
        }
        Ok(())
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("plugin {:?} not installed", id))?;
        entry.enabled = enabled;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir();
        let mut reg = PluginRegistry::default();
        reg.upsert(RegistryEntry::new("com.orbit.a".into()))
            .unwrap();
        reg.upsert(RegistryEntry::new("com.orbit.b".into()))
            .unwrap();
        reg.save(dir.path()).unwrap();
        let loaded = PluginRegistry::load(dir.path()).unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert!(loaded.entries.iter().any(|e| e.id == "com.orbit.a"));
    }

    #[test]
    fn set_enabled_persists() {
        let dir = tempdir();
        let mut reg = PluginRegistry::default();
        reg.upsert(RegistryEntry::new("com.orbit.a".into()))
            .unwrap();
        reg.save(dir.path()).unwrap();
        let mut reg = PluginRegistry::load(dir.path()).unwrap();
        reg.set_enabled("com.orbit.a", true).unwrap();
        reg.save(dir.path()).unwrap();
        let reg = PluginRegistry::load(dir.path()).unwrap();
        assert!(reg.entries[0].enabled);
    }

    #[test]
    fn upsert_replaces_existing() {
        let mut reg = PluginRegistry::default();
        reg.upsert(RegistryEntry::new("com.orbit.a".into()))
            .unwrap();
        let mut entry2 = RegistryEntry::new("com.orbit.a".into());
        entry2.enabled = true;
        reg.upsert(entry2).unwrap();
        assert_eq!(reg.entries.len(), 1);
        assert!(reg.entries[0].enabled);
    }

    /// Tiny in-test temp dir. Keeps us off the `tempfile` dep.
    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let base = std::env::temp_dir().join(format!(
            "orbit-plugin-registry-test-{}",
            ulid::Ulid::new().to_string()
        ));
        std::fs::create_dir_all(&base).unwrap();
        TempDir(base)
    }
}
