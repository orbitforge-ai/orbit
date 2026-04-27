use crate::db::DbPool;
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

/// Append an event row. Callers build the payload with `serde_json::json!(...)`.
/// Returns the event id; errors propagate as `String` to match the surrounding
/// command style.
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
            id, work_item_id, actor_kind, actor_agent_id, kind, payload_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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

// ── Row mapper / columns ─────────────────────────────────────────────────────

const EVENT_COLUMNS: &str =
    "id, work_item_id, actor_kind, actor_agent_id, kind, payload_json, created_at";

fn map_event(row: &rusqlite::Row) -> rusqlite::Result<WorkItemEvent> {
    let payload_json: String = row.get(5)?;
    Ok(WorkItemEvent {
        id: row.get(0)?,
        work_item_id: row.get(1)?,
        actor_kind: row.get(2)?,
        actor_agent_id: row.get(3)?,
        kind: row.get(4)?,
        payload: serde_json::from_str(&payload_json).unwrap_or_else(|_| serde_json::json!({})),
        created_at: row.get(6)?,
    })
}

// ── Public command ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_work_item_events(
    work_item_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<WorkItemEvent>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<WorkItemEvent>, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT {} FROM work_item_events
             WHERE work_item_id = ?1
             ORDER BY created_at ASC, id ASC",
            EVENT_COLUMNS
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let events = stmt
            .query_map(params![work_item_id], map_event)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(events)
    })
    .await
    .map_err(|e| e.to_string())?
}

mod http {
    use tauri::Manager;
    use super::*;
    use crate::db::DbPool;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Args { work_item_id: String }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_work_item_events", |ctx, args| async move {
            let app = ctx.app()?;
            let a: Args = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = list_work_item_events(a.work_item_id, app.state::<DbPool>()).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
    }
}

pub use http::register as register_http;
