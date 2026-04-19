//! HookBus — fires core events to subscribed plugins.
//!
//! V1 hook catalog (fixed — adding a new event is a documented recipe):
//! - `session.started` / `session.ended`
//! - `agent.tool.before_call` (blocking) / `agent.tool.after_call`
//! - `entity.work_item.after_create` / `after_complete` / `before_delete`
//! - `oauth.connected`
//! - `plugin.enabled` / `plugin.disabled`
//!
//! This slice lays down the enum + bus shape. The actual dispatch to plugin
//! subprocesses is wired alongside the runtime MCP client in the follow-up.

use std::sync::Arc;

use super::PluginManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    SessionStarted,
    SessionEnded,
    AgentToolBeforeCall,
    AgentToolAfterCall,
    EntityWorkItemAfterCreate,
    EntityWorkItemAfterComplete,
    EntityWorkItemBeforeDelete,
    OauthConnected,
    PluginEnabled,
    PluginDisabled,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::SessionStarted => "session.started",
            HookEvent::SessionEnded => "session.ended",
            HookEvent::AgentToolBeforeCall => "agent.tool.before_call",
            HookEvent::AgentToolAfterCall => "agent.tool.after_call",
            HookEvent::EntityWorkItemAfterCreate => "entity.work_item.after_create",
            HookEvent::EntityWorkItemAfterComplete => "entity.work_item.after_complete",
            HookEvent::EntityWorkItemBeforeDelete => "entity.work_item.before_delete",
            HookEvent::OauthConnected => "oauth.connected",
            HookEvent::PluginEnabled => "plugin.enabled",
            HookEvent::PluginDisabled => "plugin.disabled",
        }
    }

    pub fn is_blocking(self) -> bool {
        matches!(self, HookEvent::AgentToolBeforeCall | HookEvent::EntityWorkItemBeforeDelete)
    }
}

/// Returned from a blocking hook call.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct HookOutcome {
    pub veto: bool,
    pub reason: Option<String>,
}

impl HookOutcome {
    pub fn proceed() -> Self {
        Self {
            veto: false,
            reason: None,
        }
    }
}

/// Fire a hook event. For non-blocking events, delivery is best-effort and
/// this returns immediately. For blocking events, we collect responses from
/// every subscriber and veto if any of them do.
///
/// V1 note: this is a well-shaped no-op. Once the runtime MCP client lands
/// the internals call `hooks/fire` / `hooks/request` against each plugin.
pub fn fire(
    manager: &Arc<PluginManager>,
    event: HookEvent,
    _payload: &serde_json::Value,
) -> HookOutcome {
    let _subscribers: Vec<String> = manager
        .manifests()
        .into_iter()
        .filter(|m| manager.is_enabled(&m.id))
        .filter(|m| m.hooks.subscribe.iter().any(|s| s == event.as_str()))
        .map(|m| m.id)
        .collect();
    // Real dispatch is implemented alongside the MCP client; track the
    // subscribers for forthcoming wiring so we can debug which plugins care.
    HookOutcome::proceed()
}
