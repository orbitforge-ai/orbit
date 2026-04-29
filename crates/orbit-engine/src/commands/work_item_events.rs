//! Work-item activity feed.
//!
//! Read path goes through `WorkItemEventRepo` like every other aggregate.
//! Append path is special: events are inserted as part of larger
//! transactions (when a work item is created, edited, commented on, etc.),
//! so callers in `commands/work_items.rs` keep using the `insert_event`
//! helper below against a raw `&Connection` they already hold open.

use crate::app_context::AppContext;
use crate::models::work_item_event::WorkItemEvent;
use rusqlite::{params, Connection};
use ulid::Ulid;

// ── Event kinds (canonical; keep in sync with the TS `WorkItemEventKind` union) ──

pub mod event_kind {
    pub const CREATED: &str = "created";
    pub const TITLE_CHANGED: &str = "title_changed";
    pub const DESCRIPTION_CHANGED: &str = "description_changed";
    pub const KIND_CHANGED: &str = "kind_changed";
    pub const PRIORITY_CHANGED: &str = "priority_changed";
    pub const LABELS_CHANGED: &str = "labels_changed";
    pub const COLUMN_CHANGED: &str = "column_changed";
    pub const ASSIGNEE_CHANGED: &str = "assignee_changed";
    pub const BLOCKED: &str = "blocked";
    pub const UNBLOCKED: &str = "unblocked";
    pub const COMPLETED: &str = "completed";
    pub const COMMENT_ADDED: &str = "comment_added";
    pub const COMMENT_EDITED: &str = "comment_edited";
    pub const COMMENT_DELETED: &str = "comment_deleted";
}

// ── Actor helpers ─────────────────────────────────────────────────────────────

/// v1: command handlers don't thread a caller identity through IPC, so every
/// event is attributed to `system`. Once we plumb user/agent identity into the
/// command layer, replace `Actor::System` call sites with the real actor.
#[derive(Clone, Copy, Debug)]
pub enum Actor<'a> {
    System,
    #[allow(dead_code)]
    User,
    #[allow(dead_code)]
    Agent {
        agent_id: &'a str,
    },
}

impl<'a> Actor<'a> {
    fn parts(self) -> (&'static str, Option<String>) {
        match self {
            Actor::System => ("system", None),
            Actor::User => ("user", None),
            Actor::Agent { agent_id } => ("agent", Some(agent_id.to_string())),
        }
    }
}

// ── Insert helper used by other command modules ──────────────────────────────

/// Append an event row inside an existing transaction. Callers build the
/// payload with `serde_json::json!(...)`. Returns the event id; errors
/// propagate as `String` to match the surrounding command style.
pub fn insert_event(
    conn: &Connection,
    work_item_id: &str,
    actor: Actor<'_>,
    kind: &str,
    payload: serde_json::Value,
) -> Result<String, String> {
    let id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let (actor_kind, actor_agent_id) = actor.parts();
    let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO work_item_events (
            id, work_item_id, actor_kind, actor_agent_id, kind, payload_json, created_at, tenant_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, COALESCE((SELECT tenant_id FROM work_items WHERE id = ?2), 'local'))",
        params![
            id,
            work_item_id,
            actor_kind,
            actor_agent_id,
            kind,
            payload_json,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

// ── Public command ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_work_item_events(
    work_item_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<WorkItemEvent>, String> {
    app.repos.work_item_events().list(&work_item_id).await
}

mod http {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Args {
        work_item_id: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_work_item_events", |ctx, args| async move {
            let a: Args = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.work_item_events().list(&a.work_item_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
