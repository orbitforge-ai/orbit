//! Subscription reconciler.
//!
//! The dispatcher only fires on events that arrive at `trigger.emit`. Whether
//! any event *does* arrive is up to the plugin — Discord, Slack, etc. each
//! need to be told explicitly which channels and threads to listen on. The
//! reconciler is Orbit's side of that conversation: it computes the desired
//! subscription set (agent `listen_bindings` + enabled workflow triggers),
//! groups by plugin, and calls the plugin's declared `subscription_tool`.
//!
//! Declarative / idempotent: every call replaces the plugin's current set.
//! Fire on startup, on agent-binding save, on workflow enable/save.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::json;
use tauri::AppHandle;
use tracing::{info, warn};

use crate::db::repos::{sqlite::SqliteRepos, Repos};
use crate::db::DbPool;
use crate::executor::workspace;
use crate::plugins::manifest::PluginManifest;
use crate::plugins::{self, PluginManager};

/// One (channel, thread?) tuple a plugin should subscribe to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Subscription {
    channel_id: String,
    thread_id: Option<String>,
}

/// Compute and apply desired subscriptions across every installed plugin.
pub async fn reconcile_all(app: &AppHandle, db: &DbPool) {
    let manager = plugins::from_state(app);
    reconcile_all_for_manager(&manager, db).await;
}

/// Compute and apply desired subscriptions using an already-owned plugin
/// manager. This keeps command/shim paths from needing a live Tauri handle
/// just to reconcile trigger subscriptions.
pub async fn reconcile_all_for_manager(manager: &Arc<PluginManager>, db: &DbPool) {
    let repos: Arc<dyn Repos> = Arc::new(SqliteRepos::new(db.clone()));
    reconcile_all_for_manager_with_repos(manager, repos).await;
}

pub async fn reconcile_all_for_manager_with_repos(
    manager: &Arc<PluginManager>,
    repos: Arc<dyn Repos>,
) {
    let desired = compute_desired(repos.as_ref()).await;

    let mut plugin_ids: BTreeSet<String> = desired.keys().cloned().collect();
    // Also visit enabled plugins that currently have *no* desired
    // subscriptions — we still want to tell them "zero subscriptions now" so
    // they clear any stale listeners from a prior session.
    for manifest in manager.manifests() {
        if manager.is_enabled(&manifest.id) && subscription_tool_name(&manifest).is_some() {
            plugin_ids.insert(manifest.id);
        }
    }

    for plugin_id in plugin_ids {
        let subs = desired.get(&plugin_id).cloned().unwrap_or_default();
        if let Err(e) = apply_for_plugin(manager, &plugin_id, &subs).await {
            warn!(plugin_id = %plugin_id, error = %e, "reconcile: apply failed");
        }
    }
}

/// Reconcile just one plugin — used by code paths where we already know which
/// provider is affected (e.g. after saving a listen_binding for a specific
/// agent on a Discord channel).
#[allow(dead_code)]
pub async fn reconcile_plugin(app: &AppHandle, db: &DbPool, plugin_id: &str) {
    let manager = plugins::from_state(app);
    reconcile_plugin_for_manager(&manager, db, plugin_id).await;
}

#[allow(dead_code)]
pub async fn reconcile_plugin_for_manager(
    manager: &Arc<PluginManager>,
    db: &DbPool,
    plugin_id: &str,
) {
    let repos: Arc<dyn Repos> = Arc::new(SqliteRepos::new(db.clone()));
    reconcile_plugin_for_manager_with_repos(manager, repos, plugin_id).await;
}

pub async fn reconcile_plugin_for_manager_with_repos(
    manager: &Arc<PluginManager>,
    repos: Arc<dyn Repos>,
    plugin_id: &str,
) {
    let desired = compute_desired(repos.as_ref()).await;
    let subs = desired.get(plugin_id).cloned().unwrap_or_default();
    if let Err(e) = apply_for_plugin(manager, plugin_id, &subs).await {
        warn!(plugin_id, error = %e, "reconcile: apply failed");
    }
}

async fn compute_desired(
    repos: &dyn Repos,
) -> std::collections::BTreeMap<String, BTreeSet<Subscription>> {
    let mut out: std::collections::BTreeMap<String, BTreeSet<Subscription>> =
        std::collections::BTreeMap::new();

    // Agent listen_bindings.
    if let Ok(agents) = repos.agents().list().await {
        for agent in agents {
            let Ok(cfg) = workspace::load_agent_config(&agent.id) else {
                continue;
            };
            for binding in cfg.listen_bindings {
                out.entry(binding.plugin_id)
                    .or_default()
                    .insert(Subscription {
                        channel_id: binding.provider_channel_id,
                        thread_id: binding.provider_thread_id,
                    });
            }
        }
    }

    // Enabled workflow triggers — `trigger_kind` starts with
    // `trigger.<slug>.` and the config supplies channelId/threadId.
    if let Ok(workflows) = repos.project_workflows().list_enabled_triggers().await {
        for workflow in workflows {
            let Some(plugin_id) = plugin_id_from_trigger_kind(&workflow.trigger_kind) else {
                continue;
            };
            let Some(channel_id) = workflow
                .trigger_config
                .get("channelId")
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            let thread_id = workflow
                .trigger_config
                .get("threadId")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            out.entry(plugin_id).or_default().insert(Subscription {
                channel_id: channel_id.to_string(),
                thread_id,
            });
        }
    }

    out
}

/// Reverse `trigger.<slug>.<name>` → plugin id. Works by matching each
/// installed plugin's slug. Returns the first match (slugs are unique).
fn plugin_id_from_trigger_kind(kind: &str) -> Option<String> {
    let rest = kind.strip_prefix("trigger.")?;
    // The slug ends at the next `.` — everything before that, unslugified,
    // must match an installed plugin id.
    let slug = rest.split('.').next()?;
    Some(unslugify_id(slug))
}

/// Inverse of `slugify_id` from the plugin manifest module. The plugin
/// manifest only replaces `.` and `-` with `_`, so we can't perfectly
/// recover — but for the IDs Orbit ships (`com.orbit.discord`) the `_` maps
/// back to `.`. This is only a heuristic used to key into `BTreeMap`.
fn unslugify_id(slug: &str) -> String {
    slug.replace('_', ".")
}

async fn apply_for_plugin(
    manager: &Arc<PluginManager>,
    plugin_id: &str,
    subs: &BTreeSet<Subscription>,
) -> Result<(), String> {
    let Some(manifest) = manager.manifest(plugin_id) else {
        return Ok(()); // Plugin not installed (e.g. the id came from a stale workflow).
    };
    if !manager.is_enabled(plugin_id) {
        return Ok(());
    }
    let Some(tool_name) = subscription_tool_name(&manifest) else {
        // Plugin does not declare a trigger with a `subscription_tool`. The
        // bindings-as-subscriptions model only applies when the plugin opted
        // in.
        return Ok(());
    };

    let payload = json!({
        "subscriptions": subs
            .iter()
            .map(|s| match &s.thread_id {
                Some(t) => json!({ "channelId": s.channel_id, "threadId": t }),
                None => json!({ "channelId": s.channel_id }),
            })
            .collect::<Vec<_>>(),
    });

    info!(
        plugin_id,
        count = subs.len(),
        "reconcile: applying subscriptions"
    );
    let extra_env = plugins::oauth::build_env_for_subprocess(&manifest);
    manager
        .runtime
        .call_tool(&manifest, &tool_name, &payload, &extra_env)
        .await
        .map(|_| ())
}

/// The first declared `subscription_tool` across all trigger specs.
fn subscription_tool_name(manifest: &PluginManifest) -> Option<String> {
    manifest
        .workflow
        .triggers
        .iter()
        .find_map(|t| t.subscription_tool.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_recovered_from_trigger_kind() {
        assert_eq!(
            plugin_id_from_trigger_kind("trigger.com_orbit_discord.message"),
            Some("com.orbit.discord".to_string())
        );
    }

    #[test]
    fn plugin_id_none_when_prefix_missing() {
        assert_eq!(
            plugin_id_from_trigger_kind("integration.com_orbit_x.run"),
            None
        );
    }
}
