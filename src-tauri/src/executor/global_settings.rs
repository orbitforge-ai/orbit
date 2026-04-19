use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use crate::executor::channels::ChannelConfig;
use crate::executor::workspace::{AgentWorkspaceConfig, PermissionRule};

/// Current schema version for the global settings file.
pub const GLOBAL_SETTINGS_VERSION: u32 = 1;

const DEFAULT_ALLOWED_TOOLS: &[&str] = &[
    "shell_command",
    "read_file",
    "write_file",
    "edit_file",
    "list_files",
    "grep",
    "web_search",
    "web_fetch",
    "image_analysis",
    "image_generation",
    "config",
    "task",
    "work_item",
    "schedule",
    "worktree",
    "session_history",
    "session_status",
    "sessions_list",
    "session_send",
    "sessions_spawn",
    "subagents",
    "message",
    "yield_turn",
    "ask_user",
    "activate_skill",
    "remember",
    "search_memory",
];

/// Chat display toggles shared across all agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDisplaySettings {
    #[serde(default)]
    pub show_agent_thoughts: bool,
    #[serde(default)]
    pub show_verbose_tool_details: bool,
}

impl Default for ChatDisplaySettings {
    fn default() -> Self {
        Self {
            show_agent_thoughts: false,
            show_verbose_tool_details: false,
        }
    }
}

/// Defaults applied to every agent at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefaults {
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
    #[serde(default)]
    pub permission_rules: Vec<PermissionRule>,
    #[serde(default = "default_search_provider")]
    pub web_search_provider: String,
}

fn default_permission_mode() -> String {
    "normal".to_string()
}

fn default_search_provider() -> String {
    "brave".to_string()
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            allowed_tools: DEFAULT_ALLOWED_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            permission_mode: default_permission_mode(),
            permission_rules: Vec::new(),
            web_search_provider: default_search_provider(),
        }
    }
}

/// Machine-local global settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettings {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub chat_display: ChatDisplaySettings,
    #[serde(default)]
    pub agent_defaults: AgentDefaults,
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
}

fn default_version() -> u32 {
    GLOBAL_SETTINGS_VERSION
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            version: GLOBAL_SETTINGS_VERSION,
            chat_display: ChatDisplaySettings::default(),
            agent_defaults: AgentDefaults::default(),
            channels: Vec::new(),
        }
    }
}

/// Path to the global settings file.
pub fn global_settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".orbit").join("settings.json")
}

/// Process-wide cache guarded by an RwLock. The cache is lazily initialised
/// on first access via `with_cache`.
static CACHE: OnceLock<RwLock<GlobalSettings>> = OnceLock::new();

fn cache() -> &'static RwLock<GlobalSettings> {
    CACHE.get_or_init(|| {
        let settings = read_from_disk().unwrap_or_else(|e| {
            error!(
                "failed to load global settings ({}); falling back to defaults",
                e
            );
            GlobalSettings::default()
        });
        RwLock::new(settings)
    })
}

/// Read the global settings file from disk. Returns `Ok(default)` if the file
/// does not exist, and an error if the file exists but is unreadable. Parse
/// errors are handled by the caller (see [`load_global_settings`]).
fn read_from_disk() -> Result<GlobalSettings, String> {
    let path = global_settings_path();
    if !path.exists() {
        return Ok(GlobalSettings::default());
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("failed to read global settings: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("failed to parse global settings: {}", e))
}

/// Return a clone of the currently cached global settings.
pub fn load_global_settings() -> GlobalSettings {
    cache()
        .read()
        .expect("global settings lock poisoned")
        .clone()
}

/// Persist a new settings value to disk atomically (temp + rename) and update
/// the in-memory cache. Callers hold the write lock for the full operation so
/// concurrent writers cannot interleave.
pub fn save_global_settings(mut settings: GlobalSettings) -> Result<GlobalSettings, String> {
    if settings.version == 0 {
        settings.version = GLOBAL_SETTINGS_VERSION;
    }

    let cache = cache();
    let mut guard = cache.write().expect("global settings lock poisoned");

    let path = global_settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create settings directory: {}", e))?;
    }

    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("failed to serialize global settings: {}", e))?;
    fs::write(&tmp_path, &json)
        .map_err(|e| format!("failed to write settings temp file: {}", e))?;
    fs::rename(&tmp_path, &path)
        .map_err(|e| format!("failed to atomically replace settings file: {}", e))?;

    *guard = settings.clone();
    Ok(settings)
}

/// Replace the cached settings from disk (used after migration or in tests).
pub fn reload_cache_from_disk() -> Result<(), String> {
    let settings = read_from_disk()?;
    let cache = cache();
    let mut guard = cache.write().expect("global settings lock poisoned");
    *guard = settings;
    Ok(())
}

/// Overwrite the cached settings in-memory without touching disk. Used by the
/// migration path to publish seeded settings before they would otherwise be
/// read by other code.
#[allow(dead_code)]
pub fn set_cache(settings: GlobalSettings) {
    let cache = cache();
    let mut guard = cache.write().expect("global settings lock poisoned");
    *guard = settings;
}

/// If the settings file exists but fails to parse, rename it aside so the app
/// can recover with defaults. Returns `true` if a file was quarantined.
pub fn quarantine_corrupt_settings_file() -> bool {
    let path = global_settings_path();
    if !path.exists() {
        return false;
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if serde_json::from_str::<GlobalSettings>(&content).is_ok() {
        return false;
    }
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S").to_string();
    let backup = path.with_file_name(format!("settings.json.corrupt-{}", ts));
    match fs::rename(&path, &backup) {
        Ok(()) => {
            warn!(
                "global settings file was corrupt; quarantined to {}",
                backup.display()
            );
            true
        }
        Err(e) => {
            error!(
                "failed to quarantine corrupt settings file {}: {}",
                path.display(),
                e
            );
            false
        }
    }
}

/// Add or replace a permission rule in the global settings. Rules with the
/// same (tool, pattern) pair are deduped — the newest entry wins.
pub fn save_global_permission_rule(rule: PermissionRule) -> Result<(), String> {
    let mut settings = load_global_settings();
    settings
        .agent_defaults
        .permission_rules
        .retain(|r| !(r.tool == rule.tool && r.pattern == rule.pattern));
    settings.agent_defaults.permission_rules.push(rule);
    save_global_settings(settings).map(|_| ())
}

/// Remove a permission rule by its id.
pub fn delete_global_permission_rule(rule_id: &str) -> Result<(), String> {
    let mut settings = load_global_settings();
    settings
        .agent_defaults
        .permission_rules
        .retain(|r| r.id != rule_id);
    save_global_settings(settings).map(|_| ())
}

/// Find a channel in the cached global settings by id or case-insensitive name.
pub fn find_channel_in_global(needle: &str) -> Option<ChannelConfig> {
    let settings = load_global_settings();
    let lowered = needle.to_lowercase();
    settings
        .channels
        .into_iter()
        .find(|c| c.id == needle || c.name.to_lowercase() == lowered)
}

/// Look up a channel by its id (exact match) in the cached global settings.
pub fn find_channel_by_id(id: &str) -> Option<ChannelConfig> {
    let settings = load_global_settings();
    settings.channels.into_iter().find(|c| c.id == id)
}

/// A snapshot of the merged runtime config for a single run. It combines the
/// slim per-agent config with the global defaults so call sites can read a
/// single struct without knowing where each value lives.
///
/// The merge is a point-in-time snapshot: changes made to the global settings
/// file mid-run do not affect an already-started run. This matches the plan's
/// "per-run snapshot" semantics and keeps execution deterministic.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MergedAgentConfig {
    pub agent: AgentWorkspaceConfig,
    pub allowed_tools: Vec<String>,
    pub permission_mode: String,
    pub permission_rules: Vec<PermissionRule>,
    pub web_search_provider: String,
    pub channels: Vec<ChannelConfig>,
    pub chat_display: ChatDisplaySettings,
}

impl MergedAgentConfig {
    /// Compute the effective merged config from a per-agent config and the
    /// current global settings snapshot.
    pub fn build(agent: AgentWorkspaceConfig) -> Self {
        let global = load_global_settings();
        Self::build_with_global(agent, global)
    }

    /// Same as `build`, but uses the supplied global settings (useful for
    /// tests and migration-time code paths).
    pub fn build_with_global(agent: AgentWorkspaceConfig, global: GlobalSettings) -> Self {
        let mut effective_allowed: Vec<String> = if global.agent_defaults.allowed_tools.is_empty() {
            // Empty allow list means "all default tools".
            DEFAULT_ALLOWED_TOOLS
                .iter()
                .filter(|tool| !agent.disabled_tools.iter().any(|d| d == *tool))
                .map(|s| s.to_string())
                .collect()
        } else {
            global
                .agent_defaults
                .allowed_tools
                .iter()
                .filter(|tool| !agent.disabled_tools.iter().any(|d| d == *tool))
                .cloned()
                .collect()
        };

        // Backfill `work_item` for older saved global settings that predate the
        // persistent board tool. If `task` is allowed, the agent should also be
        // able to create project board cards unless the per-agent disabled list
        // explicitly opts out.
        if effective_allowed.iter().any(|tool| tool == "task")
            && !effective_allowed.iter().any(|tool| tool == "work_item")
            && !agent.disabled_tools.iter().any(|tool| tool == "work_item")
        {
            effective_allowed.push("work_item".to_string());
        }

        Self {
            agent,
            allowed_tools: effective_allowed,
            permission_mode: global.agent_defaults.permission_mode,
            permission_rules: global.agent_defaults.permission_rules,
            web_search_provider: global.agent_defaults.web_search_provider,
            channels: global.channels,
            chat_display: global.chat_display,
        }
    }
}

/// Built-in default allowed-tool list (used by the UI to populate the
/// disabled-tools multi-select when the global list is empty).
#[allow(dead_code)]
pub fn default_allowed_tools() -> Vec<String> {
    DEFAULT_ALLOWED_TOOLS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_global_settings_roundtrips() {
        let default = GlobalSettings::default();
        let json = serde_json::to_string(&default).unwrap();
        let parsed: GlobalSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, GLOBAL_SETTINGS_VERSION);
        assert_eq!(parsed.agent_defaults.permission_mode, "normal");
    }

    #[test]
    fn defaults_include_expected_tools() {
        let defaults = AgentDefaults::default();
        assert!(defaults.allowed_tools.contains(&"read_file".to_string()));
        assert!(defaults.allowed_tools.contains(&"message".to_string()));
        assert!(defaults.allowed_tools.contains(&"work_item".to_string()));
        assert!(!defaults.allowed_tools.contains(&"finish".to_string()));
    }

    #[test]
    fn merged_config_subtracts_disabled_tools() {
        let mut agent = AgentWorkspaceConfig::default();
        agent.disabled_tools = vec!["web_search".to_string()];
        let merged = MergedAgentConfig::build_with_global(agent, GlobalSettings::default());
        assert!(!merged.allowed_tools.iter().any(|t| t == "web_search"));
        assert!(merged.allowed_tools.iter().any(|t| t == "read_file"));
    }

    #[test]
    fn merged_config_with_empty_allow_list_uses_defaults() {
        let mut global = GlobalSettings::default();
        global.agent_defaults.allowed_tools = Vec::new();
        let mut agent = AgentWorkspaceConfig::default();
        agent.disabled_tools = vec!["grep".to_string()];
        let merged = MergedAgentConfig::build_with_global(agent, global);
        assert!(!merged.allowed_tools.iter().any(|t| t == "grep"));
        assert!(merged.allowed_tools.iter().any(|t| t == "read_file"));
    }

    #[test]
    fn merged_config_backfills_work_item_for_legacy_task_tool_access() {
        let mut global = GlobalSettings::default();
        global.agent_defaults.allowed_tools = vec!["task".to_string(), "read_file".to_string()];
        let agent = AgentWorkspaceConfig::default();
        let merged = MergedAgentConfig::build_with_global(agent, global);
        assert!(merged.allowed_tools.iter().any(|t| t == "task"));
        assert!(merged.allowed_tools.iter().any(|t| t == "work_item"));
    }

    #[test]
    fn merged_config_respects_disabled_work_item_even_when_backfilling() {
        let mut global = GlobalSettings::default();
        global.agent_defaults.allowed_tools = vec!["task".to_string(), "read_file".to_string()];
        let mut agent = AgentWorkspaceConfig::default();
        agent.disabled_tools = vec!["work_item".to_string()];
        let merged = MergedAgentConfig::build_with_global(agent, global);
        assert!(merged.allowed_tools.iter().any(|t| t == "task"));
        assert!(!merged.allowed_tools.iter().any(|t| t == "work_item"));
    }
}
