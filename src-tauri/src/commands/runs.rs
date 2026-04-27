use crate::db::DbPool;
use crate::models::run::{Run, RunSummary};

pub async fn list_runs_impl(
    limit: Option<i64>,
    offset: Option<i64>,
    task_id: Option<String>,
    state_filter: Option<String>,
    project_id: Option<String>,
    db: &DbPool,
) -> Result<Vec<RunSummary>, String> {
    let pool = db.0.clone();
    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);

    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;

        // Build params in ORDER: limit, offset (fixed positions), then optional filters
        let mut extra_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        extra_params.push(Box::new(limit));
        extra_params.push(Box::new(offset));

        let mut sql = String::from(
            "SELECT r.id, r.task_id, t.name as task_name, r.schedule_id,
                    r.agent_id, a.name as agent_name,
                    r.state, r.trigger, r.exit_code,
                    r.started_at, r.finished_at, r.duration_ms, r.retry_count, r.is_sub_agent,
                    r.created_at,
                    json_extract(r.metadata, '$.chat_session_id') as chat_session_id,
                    r.project_id
             FROM runs r
             LEFT JOIN tasks t ON t.id = r.task_id
             LEFT JOIN agents a ON a.id = r.agent_id
             WHERE 1=1",
        );

        // Add filter conditions after WHERE 1=1, binding to params after limit/offset
        if let Some(ref tid) = task_id {
            let n = extra_params.len() + 1;
            sql.push_str(&format!(" AND r.task_id = ?{n}"));
            extra_params.push(Box::new(tid.clone()));
        }

        let apply_state = state_filter.as_deref().map(|s| s != "all").unwrap_or(false);
        if apply_state {
            let n = extra_params.len() + 1;
            sql.push_str(&format!(" AND r.state = ?{n}"));
            extra_params.push(Box::new(state_filter.clone().unwrap()));
        }

        if let Some(ref pid) = project_id {
            let n = extra_params.len() + 1;
            sql.push_str(&format!(" AND r.project_id = ?{n}"));
            extra_params.push(Box::new(pid.clone()));
        }

        sql.push_str(" ORDER BY r.created_at DESC LIMIT ?1 OFFSET ?2");

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            extra_params.iter().map(|p| p.as_ref()).collect();

        let runs = stmt
            .query_map(params_refs.as_slice(), map_row_to_run_summary)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(runs)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_runs(
    limit: Option<i64>,
    offset: Option<i64>,
    task_id: Option<String>,
    state_filter: Option<String>,
    project_id: Option<String>,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<RunSummary>, String> {
    list_runs_impl(limit, offset, task_id, state_filter, project_id, db.inner()).await
}

fn map_row_to_run_summary(row: &rusqlite::Row) -> rusqlite::Result<RunSummary> {
    Ok(RunSummary {
        id: row.get(0)?,
        task_id: row.get(1)?,
        task_name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        schedule_id: row.get(3)?,
        agent_id: row.get(4)?,
        agent_name: row.get(5)?,
        state: row.get(6)?,
        trigger: row.get(7)?,
        exit_code: row.get(8)?,
        started_at: row.get(9)?,
        finished_at: row.get(10)?,
        duration_ms: row.get(11)?,
        retry_count: row.get(12)?,
        is_sub_agent: row.get::<_, i64>(13)? != 0,
        created_at: row.get(14)?,
        chat_session_id: row.get(15)?,
        project_id: row.get(16)?,
    })
}

#[tauri::command]
pub async fn get_run(id: String, db: tauri::State<'_, DbPool>) -> Result<Run, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, task_id, schedule_id, agent_id, state, trigger, exit_code, pid,
                    log_path, started_at, finished_at, duration_ms, retry_count,
                    parent_run_id, metadata, is_sub_agent, created_at, project_id
             FROM runs WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                let meta_str: String = row.get(14)?;
                Ok(Run {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    schedule_id: row.get(2)?,
                    agent_id: row.get(3)?,
                    state: row.get(4)?,
                    trigger: row.get(5)?,
                    exit_code: row.get(6)?,
                    pid: row.get(7)?,
                    log_path: row.get(8)?,
                    started_at: row.get(9)?,
                    finished_at: row.get(10)?,
                    duration_ms: row.get(11)?,
                    retry_count: row.get(12)?,
                    parent_run_id: row.get(13)?,
                    metadata: serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null),
                    is_sub_agent: row.get::<_, i64>(15)? != 0,
                    created_at: row.get(16)?,
                    project_id: row.get(17)?,
                })
            },
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_active_runs(db: tauri::State<'_, DbPool>) -> Result<Vec<RunSummary>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.task_id, t.name as task_name, r.schedule_id,
                        r.agent_id, a.name as agent_name,
                        r.state, r.trigger, r.exit_code,
                        r.started_at, r.finished_at, r.duration_ms, r.retry_count, r.is_sub_agent,
                        r.created_at,
                        json_extract(r.metadata, '$.chat_session_id') as chat_session_id
                 FROM runs r
                 LEFT JOIN tasks t ON t.id = r.task_id
                 LEFT JOIN agents a ON a.id = r.agent_id
                 WHERE r.state IN ('pending', 'queued', 'running')
                 ORDER BY r.created_at DESC",
            )
            .map_err(|e| e.to_string())?;

        let runs = stmt
            .query_map([], map_row_to_run_summary)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(runs)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_agent_conversation(
    run_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Option<serde_json::Value>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let result = conn.query_row(
            "SELECT messages FROM agent_conversations WHERE run_id = ?1",
            rusqlite::params![run_id],
            |row| {
                let json_str: String = row.get(0)?;
                Ok(json_str)
            },
        );
        match result {
            Ok(json_str) => {
                let parsed: serde_json::Value =
                    serde_json::from_str(&json_str).map_err(|e| e.to_string())?;
                Ok(Some(parsed))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_sub_agent_runs(
    parent_run_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<RunSummary>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.task_id, t.name as task_name, r.schedule_id,
                        r.agent_id, a.name as agent_name,
                        r.state, r.trigger, r.exit_code,
                        r.started_at, r.finished_at, r.duration_ms, r.retry_count, r.is_sub_agent,
                        r.created_at,
                        json_extract(r.metadata, '$.chat_session_id') as chat_session_id
                 FROM runs r
                 LEFT JOIN tasks t ON t.id = r.task_id
                 LEFT JOIN agents a ON a.id = r.agent_id
                 WHERE r.parent_run_id = ?1 AND r.is_sub_agent = 1
                 ORDER BY r.created_at ASC",
            )
            .map_err(|e| e.to_string())?;

        let runs = stmt
            .query_map(rusqlite::params![parent_run_id], map_row_to_run_summary)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        Ok(runs)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_run_log(run_id: String, db: tauri::State<'_, DbPool>) -> Result<String, String> {
    let pool = db.0.clone();
    let log_path: String = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT log_path FROM runs WHERE id = ?1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    tokio::fs::read_to_string(&log_path)
        .await
        .map_err(|e| format!("cannot read log file: {}", e))
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct ListRunsArgs {
    limit: Option<i64>,
    offset: Option<i64>,
    task_id: Option<String>,
    state_filter: Option<String>,
    project_id: Option<String>,
}

pub fn register_http(reg: &mut crate::shim::registry::Registry) {
    reg.register("list_runs", |ctx, args| async move {
        let a: ListRunsArgs = if args.is_null() {
            ListRunsArgs::default()
        } else {
            serde_json::from_value(args).map_err(|e| e.to_string())?
        };
        let result = list_runs_impl(
            a.limit,
            a.offset,
            a.task_id,
            a.state_filter,
            a.project_id,
            &ctx.db,
        )
        .await?;
        serde_json::to_value(result).map_err(|e| e.to_string())
    });
}
