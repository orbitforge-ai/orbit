//! One-time migration that seeds the global settings file from legacy
//! per-agent `config.json` and `channels.json` files.
//!
//! Designed to run exactly once during app `setup`, before any Tauri command
//! that touches global settings becomes callable. Idempotent: if the global
//! settings file already exists and parses, this is a no-op.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use serde_json::Value;
use tracing::{error, info, warn};

use crate::executor::channels::ChannelConfig;
use crate::executor::global_settings::{
    self, AgentDefaults, ChatDisplaySettings, GlobalSettings, GLOBAL_SETTINGS_VERSION,
};
use crate::executor::workspace::{self, agent_dir, agents_root, PermissionRule};

/// Entry point invoked from `lib.rs` during setup.
pub fn migrate_global_settings() {
    // If the file exists and parses, we already migrated. Just prime the cache.
    if global_settings::global_settings_path().exists() {
        match try_reload_cache() {
            Ok(()) => {
                info!("global settings already present; skipping migration");
                return;
            }
            Err(e) => {
                warn!(
                    "global settings file exists but failed to load ({}); attempting to quarantine and re-seed",
                    e
                );
                global_settings::quarantine_corrupt_settings_file();
            }
        }
    }

    match seed_from_legacy() {
        Ok(settings) => {
            if let Err(e) = global_settings::save_global_settings(settings) {
                error!("failed to persist seeded global settings: {}", e);
                return;
            }
            if let Err(e) = rewrite_per_agent_configs_to_slim_shape() {
                error!("failed to rewrite per-agent configs to slim shape: {}", e);
            }
            info!("global settings migration completed");
        }
        Err(e) => {
            error!("global settings migration failed: {}", e);
        }
    }
}

fn try_reload_cache() -> Result<(), String> {
    global_settings::reload_cache_from_disk()
}

/// Seed a fresh `GlobalSettings` by scanning every per-agent directory and
/// extracting channels, permission rules, and singleton policy fields.
fn seed_from_legacy() -> Result<GlobalSettings, String> {
    let root = agents_root();
    let agent_dirs = enumerate_agent_dirs(&root);

    // Sort by modification time descending so "newest wins" for singleton fields.
    let mut raw_configs: Vec<(String, PathBuf, Value, SystemTime)> = Vec::new();
    for (agent_id, path) in &agent_dirs {
        let config_path = path.join("config.json");
        if !config_path.exists() {
            continue;
        }
        let content = match fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(e) => {
                warn!("migration: failed to read {}: {}", config_path.display(), e);
                continue;
            }
        };
        let value: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    "migration: failed to parse {}: {}",
                    config_path.display(),
                    e
                );
                continue;
            }
        };
        let modified = fs::metadata(&config_path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        raw_configs.push((agent_id.clone(), config_path, value, modified));
    }
    raw_configs.sort_by(|a, b| b.3.cmp(&a.3));

    // Seed singleton fields from the newest agent config (if any).
    let mut defaults = AgentDefaults::default();
    if let Some((_, _, newest, _)) = raw_configs.first() {
        if let Some(tools) = newest.get("allowedTools").and_then(|v| v.as_array()) {
            let parsed: Vec<String> = tools
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !parsed.is_empty() {
                defaults.allowed_tools = parsed;
            }
        }
        if let Some(mode) = newest.get("permissionMode").and_then(|v| v.as_str()) {
            defaults.permission_mode = mode.to_string();
        }
        if let Some(provider) = newest.get("webSearchProvider").and_then(|v| v.as_str()) {
            defaults.web_search_provider = provider.to_string();
        }
    }

    // Collect rules from every agent (newest first).
    let mut collected_rules: Vec<PermissionRule> = Vec::new();
    for (agent_id, _, value, _) in &raw_configs {
        if let Some(arr) = value.get("permissionRules").and_then(|v| v.as_array()) {
            for raw in arr {
                match serde_json::from_value::<PermissionRule>(raw.clone()) {
                    Ok(rule) => collected_rules.push(rule),
                    Err(e) => warn!(
                        "migration: agent {} had an unparseable permission rule: {}",
                        agent_id, e
                    ),
                }
            }
        }
    }
    defaults.permission_rules = merge_permission_rules(collected_rules);

    // Merge channels from every agent's channels.json. Dedupe on id; prefer
    // the entry whose containing file was modified most recently.
    let mut merged_channels: HashMap<String, (ChannelConfig, SystemTime, String)> = HashMap::new();
    for (agent_id, path) in &agent_dirs {
        let channels_path = path.join("channels.json");
        if !channels_path.exists() {
            continue;
        }
        let content = match fs::read_to_string(&channels_path) {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "migration: failed to read {}: {}",
                    channels_path.display(),
                    e
                );
                continue;
            }
        };
        let parsed: Result<LegacyChannelsFile, _> = serde_json::from_str(&content);
        let file = match parsed {
            Ok(f) => f,
            Err(e) => {
                warn!(
                    "migration: failed to parse {}: {}",
                    channels_path.display(),
                    e
                );
                continue;
            }
        };
        let modified = fs::metadata(&channels_path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        for channel in file.channels {
            let id = channel.id.clone();
            match merged_channels.get(&id) {
                None => {
                    merged_channels.insert(id, (channel, modified, agent_id.clone()));
                }
                Some((_, existing_mtime, existing_agent)) => {
                    if modified > *existing_mtime {
                        warn!(
                            "migration: duplicate channel id '{}' in agents '{}' and '{}'; preferring newer file from '{}'",
                            id, existing_agent, agent_id, agent_id
                        );
                        merged_channels.insert(id, (channel, modified, agent_id.clone()));
                    } else {
                        warn!(
                            "migration: duplicate channel id '{}' in agents '{}' and '{}'; keeping older-but-newer file from '{}'",
                            id, existing_agent, agent_id, existing_agent
                        );
                    }
                }
            }
        }
    }
    let channels: Vec<ChannelConfig> = merged_channels
        .into_values()
        .map(|(channel, _, _)| channel)
        .collect();

    let settings = GlobalSettings {
        version: GLOBAL_SETTINGS_VERSION,
        chat_display: ChatDisplaySettings::default(),
        agent_defaults: defaults,
        channels,
        developer: Default::default(),
    };
    Ok(settings)
}

/// Walk `~/.orbit/agents/*` and return `(agent_id, agent_dir)` tuples for every
/// subdirectory found.
fn enumerate_agent_dirs(root: &PathBuf) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let read_dir = match fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let agent_id = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => continue,
        };
        out.push((agent_id, path));
    }
    out
}

/// For every per-agent `config.json`, rewrite the file in the slim shape and
/// drop the moved fields. Leaves a `config.json.bak-<timestamp>` sidecar for
/// rollback visibility.
fn rewrite_per_agent_configs_to_slim_shape() -> Result<(), String> {
    let root = agents_root();
    let global = global_settings::load_global_settings();
    let default_channel_id_fallback: Option<String> = global
        .channels
        .iter()
        .find(|c| c.enabled)
        .map(|c| c.id.clone());

    for (agent_id, path) in enumerate_agent_dirs(&root) {
        let config_path = path.join("config.json");
        if !config_path.exists() {
            continue;
        }

        // Read the legacy JSON as a raw Value so we can look at the pre-slim
        // fields we care about (first-enabled channel from the agent's own
        // channels.json, for example).
        let legacy_content = match fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(e) => {
                warn!("migration: failed to read {}: {}", config_path.display(), e);
                continue;
            }
        };

        // Back up the legacy config before rewriting.
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S").to_string();
        let backup = config_path.with_file_name(format!("config.json.bak-{}", ts));
        if let Err(e) = fs::write(&backup, &legacy_content) {
            warn!(
                "migration: failed to write backup {}: {}",
                backup.display(),
                e
            );
        }

        // Parse as slim config — serde silently drops the moved fields.
        let mut slim: workspace::AgentWorkspaceConfig = match serde_json::from_str(&legacy_content)
        {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "migration: failed to parse slim shape from {}: {}; replacing with defaults",
                    config_path.display(),
                    e
                );
                workspace::AgentWorkspaceConfig::default()
            }
        };

        // Pick the default channel: the agent's own first enabled legacy
        // channel, or the first enabled global channel, or nothing.
        let own_default: Option<String> = read_first_enabled_channel_id(&agent_dir(&agent_id));
        slim.default_channel_id = own_default.or_else(|| default_channel_id_fallback.clone());

        if let Err(e) = workspace::save_agent_config(&agent_id, &slim) {
            warn!(
                "migration: failed to save slim config for {}: {}",
                agent_id, e
            );
        }
    }

    Ok(())
}

fn read_first_enabled_channel_id(agent_dir: &PathBuf) -> Option<String> {
    let path = agent_dir.join("channels.json");
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    let file: LegacyChannelsFile = serde_json::from_str(&content).ok()?;
    file.channels.into_iter().find(|c| c.enabled).map(|c| c.id)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyChannelsFile {
    #[serde(default)]
    channels: Vec<ChannelConfig>,
}

/// Merge permission rules from multiple agents into a single global list.
/// Rules are deduped on `(tool, pattern)`. On a decision conflict, `allow`
/// wins — if two agents had divergent rules for the same tool/pattern pair,
/// the less restrictive one is kept so legitimate workflows do not break at
/// migration time.
///
/// The input is expected to be ordered newest-first; when the decision ties,
/// the earlier (newer) entry is retained so its description/id are preferred.
fn merge_permission_rules(rules: Vec<PermissionRule>) -> Vec<PermissionRule> {
    let mut merged: HashMap<(String, String), PermissionRule> = HashMap::new();
    for rule in rules {
        let key = (rule.tool.clone(), rule.pattern.clone());
        match merged.get(&key) {
            None => {
                merged.insert(key, rule);
            }
            Some(existing) if existing.decision != rule.decision => {
                // Conflict — allow wins.
                if rule.decision == "allow" && existing.decision != "allow" {
                    merged.insert(key, rule);
                }
            }
            _ => {
                // Same decision, keep the already-seen (newer) one.
            }
        }
    }
    merged.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, tool: &str, pattern: &str, decision: &str) -> PermissionRule {
        PermissionRule {
            id: id.to_string(),
            tool: tool.to_string(),
            pattern: pattern.to_string(),
            decision: decision.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            description: None,
        }
    }

    #[test]
    fn merge_rules_dedupes_by_tool_and_pattern() {
        // Two agents persisted a rule for the same (tool, pattern). After
        // migration the union should contain exactly one entry.
        let merged = merge_permission_rules(vec![
            rule("1", "shell_command", "ls *", "allow"),
            rule("2", "shell_command", "ls *", "allow"),
        ]);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn merge_rules_allow_wins_on_conflict() {
        // Newest first: the newer entry is a deny, but a later (older) entry
        // is an allow for the same (tool, pattern). Allow must win so the
        // migration does not quietly lock the user out of an already-working
        // workflow.
        let merged = merge_permission_rules(vec![
            rule("new", "shell_command", "git *", "deny"),
            rule("old", "shell_command", "git *", "allow"),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].decision, "allow");
    }

    #[test]
    fn merge_rules_preserves_distinct_patterns() {
        let merged = merge_permission_rules(vec![
            rule("a", "shell_command", "ls *", "allow"),
            rule("b", "shell_command", "rm *", "deny"),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_rules_same_decision_keeps_newest() {
        // Both rules allow the same (tool, pattern). The newest (first in the
        // input) wins so its id/description are retained.
        let merged = merge_permission_rules(vec![
            rule("newest", "write_file", "/tmp/*", "allow"),
            rule("older", "write_file", "/tmp/*", "allow"),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "newest");
    }
}
