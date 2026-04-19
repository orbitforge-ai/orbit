use crate::db::cloud::CloudClientState;
use crate::db::DbPool;
use crate::executor::workspace;
use crate::models::agent::Agent;
use crate::models::project::{CreateProject, Project, ProjectAgent, ProjectSummary, UpdateProject};

macro_rules! cloud_upsert_project {
    ($cloud:expr, $project:expr) => {
        if let Some(client) = $cloud.get() {
            let p = $project.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_project(&p).await {
                    tracing::warn!("cloud upsert project: {}", e);
                }
            });
        }
    };
}

macro_rules! cloud_delete {
    ($cloud:expr, $table:expr, $id:expr) => {
        if let Some(client) = $cloud.get() {
            let id = $id.to_string();
            tokio::spawn(async move {
                if let Err(e) = client.delete_by_id($table, &id).await {
                    tracing::warn!("cloud delete {}: {}", $table, e);
                }
            });
        }
    };
}

fn map_project(row: &rusqlite::Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn map_project_summary(row: &rusqlite::Row) -> rusqlite::Result<ProjectSummary> {
    Ok(ProjectSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        agent_count: row.get(5)?,
    })
}

#[tauri::command]
pub async fn list_projects(db: tauri::State<'_, DbPool>) -> Result<Vec<ProjectSummary>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.name, p.description, p.created_at, p.updated_at,
                        COALESCE(pa.agent_count, 0) AS agent_count
                 FROM projects p
                 LEFT JOIN (
                     SELECT project_id, COUNT(*) AS agent_count
                     FROM project_agents
                     GROUP BY project_id
                 ) pa ON pa.project_id = p.id
                 ORDER BY p.created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let projects = stmt
            .query_map([], map_project_summary)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(projects)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_project(id: String, db: tauri::State<'_, DbPool>) -> Result<Project, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
            rusqlite::params![id],
            map_project,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_project(
    payload: CreateProject,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<Project, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let project: Project = tokio::task::spawn_blocking(move || -> Result<Project, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let base_slug = workspace::slugify(&payload.name);
        let base_slug = if base_slug.is_empty() {
            "project".to_string()
        } else {
            base_slug
        };

        let id = {
            let mut candidate = base_slug.clone();
            let mut suffix = 1;
            while conn
                .query_row(
                    "SELECT 1 FROM projects WHERE id = ?1",
                    rusqlite::params![candidate],
                    |_| Ok(()),
                )
                .is_ok()
            {
                suffix += 1;
                candidate = format!("{}-{}", base_slug, suffix);
            }
            candidate
        };

        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO projects (id, name, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            rusqlite::params![id, payload.name, payload.description, now],
        )
        .map_err(|e| e.to_string())?;

        let project = conn
            .query_row(
                "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
                rusqlite::params![id],
                map_project,
            )
            .map_err(|e| e.to_string())?;

        workspace::init_project_workspace(&project.id)?;

        Ok(project)
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_project!(cloud, project);
    Ok(project)
}

#[tauri::command]
pub async fn update_project(
    id: String,
    payload: UpdateProject,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<Project, String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let project: Project = tokio::task::spawn_blocking(move || -> Result<Project, String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(name) = &payload.name {
            conn.execute(
                "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![name, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(desc) = &payload.description {
            conn.execute(
                "UPDATE projects SET description = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![desc, now, id],
            )
            .map_err(|e| e.to_string())?;
        }
        conn.query_row(
            "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
            rusqlite::params![id],
            map_project,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_upsert_project!(cloud, project);
    Ok(project)
}

#[tauri::command]
pub async fn delete_project(
    id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let cloud = cloud.inner().clone();
    let pool = db.0.clone();
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM projects WHERE id = ?1",
            rusqlite::params![id_clone],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    cloud_delete!(cloud, "projects", id);
    Ok(())
}

// ─── Project Agent Membership ────────────────────────────────────────────────

/// Synchronous membership check — for use inside `spawn_blocking` contexts
/// that already hold a connection.
pub fn assert_agent_in_project_sync(
    conn: &rusqlite::Connection,
    project_id: &str,
    agent_id: &str,
) -> Result<(), String> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM project_agents WHERE project_id = ?1 AND agent_id = ?2)",
            rusqlite::params![project_id, agent_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err(format!(
            "agent '{}' is not a member of project '{}'",
            agent_id, project_id
        ));
    }
    Ok(())
}

/// Async membership check — for use from async call sites that don't already
/// hold a DB connection (e.g. before spawning the agent session loop).
pub async fn assert_agent_in_project(
    db: &DbPool,
    project_id: &str,
    agent_id: &str,
) -> Result<(), String> {
    let pool = db.0.clone();
    let project_id = project_id.to_string();
    let agent_id = agent_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = pool.get().map_err(|e| e.to_string())?;
        assert_agent_in_project_sync(&conn, &project_id, &agent_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_project_agents(
    project_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<Agent>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.name, a.description, a.state, a.max_concurrent_runs,
                        a.heartbeat_at, a.created_at, a.updated_at
                 FROM agents a
                 JOIN project_agents pa ON pa.agent_id = a.id
                 WHERE pa.project_id = ?1
                 ORDER BY a.created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let agents = stmt
            .query_map(rusqlite::params![project_id], |row| {
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    state: row.get(3)?,
                    max_concurrent_runs: row.get(4)?,
                    heartbeat_at: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(agents)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAgentWithMeta {
    pub agent: Agent,
    pub is_default: bool,
}

#[tauri::command]
pub async fn list_project_agents_with_meta(
    project_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<ProjectAgentWithMeta>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.name, a.description, a.state, a.max_concurrent_runs,
                        a.heartbeat_at, a.created_at, a.updated_at, pa.is_default
                 FROM agents a
                 JOIN project_agents pa ON pa.agent_id = a.id
                 WHERE pa.project_id = ?1
                 ORDER BY pa.is_default DESC, a.created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![project_id], |row| {
                Ok(ProjectAgentWithMeta {
                    agent: Agent {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        state: row.get(3)?,
                        max_concurrent_runs: row.get(4)?,
                        heartbeat_at: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    },
                    is_default: row.get::<_, bool>(8)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_agent_projects(
    agent_id: String,
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<Project>, String> {
    let pool = db.0.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.name, p.description, p.created_at, p.updated_at
                 FROM projects p
                 JOIN project_agents pa ON pa.project_id = p.id
                 WHERE pa.agent_id = ?1
                 ORDER BY pa.added_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let projects = stmt
            .query_map(rusqlite::params![agent_id], map_project)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(projects)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn add_agent_to_project(
    project_id: String,
    agent_id: String,
    is_default: bool,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<ProjectAgent, String> {
    let pool = db.0.clone();
    let pa = tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO project_agents (project_id, agent_id, is_default, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![project_id, agent_id, is_default as i64, now],
        )
        .map_err(|e| e.to_string())?;
        Ok::<ProjectAgent, String>(ProjectAgent {
            project_id,
            agent_id,
            is_default,
            added_at: now,
        })
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(client) = cloud.get() {
        let pa_clone = pa.clone();
        tokio::spawn(async move {
            if let Err(e) = client.upsert_project_agent(&pa_clone).await {
                tracing::warn!("cloud upsert project_agent: {}", e);
            }
        });
    }
    Ok(pa)
}

#[tauri::command]
pub async fn remove_agent_from_project(
    project_id: String,
    agent_id: String,
    db: tauri::State<'_, DbPool>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let pool = db.0.clone();
    let pid = project_id.clone();
    let aid = agent_id.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM project_agents WHERE project_id = ?1 AND agent_id = ?2",
            rusqlite::params![project_id, agent_id],
        )
        .map_err(|e| e.to_string())?;
        // Clear any work item assignments held by this agent in this project.
        // Cards stay in their current column (no auto-move); a new claimant
        // is needed for work to continue. See plan §3 "Unassign side effect".
        tx.execute(
            "UPDATE work_items
                SET assignee_agent_id = NULL, updated_at = ?1
              WHERE project_id = ?2 AND assignee_agent_id = ?3",
            rusqlite::params![now, project_id, agent_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(client) = cloud.get() {
        tokio::spawn(async move {
            if let Err(e) = client.delete_project_agent(&pid, &aid).await {
                tracing::warn!("cloud delete project_agent: {}", e);
            }
        });
    }
    Ok(())
}

// ─── Project Workspace File Operations ───────────────────────────────────────

#[tauri::command]
pub fn get_project_workspace_path(project_id: String) -> String {
    workspace::project_workspace_dir(&project_id)
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
pub async fn list_project_workspace_files(
    project_id: String,
    path: Option<String>,
) -> Result<Vec<workspace::FileEntry>, String> {
    let rel = path.unwrap_or_else(|| ".".to_string());
    tokio::task::spawn_blocking(move || workspace::list_project_workspace_files(&project_id, &rel))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_project_workspace_file(
    project_id: String,
    path: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || workspace::read_project_workspace_file(&project_id, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn write_project_workspace_file(
    project_id: String,
    path: String,
    content: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        workspace::write_project_workspace_file(&project_id, &path, &content)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_project_workspace_file(project_id: String, path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        workspace::delete_project_workspace_file(&project_id, &path)
    })
    .await
    .map_err(|e| e.to_string())?
}
