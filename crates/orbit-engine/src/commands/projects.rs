use crate::app_context::AppContext;
use crate::commands::project_board_columns::ensure_project_board_columns;
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

#[tauri::command]
pub async fn list_projects(
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<ProjectSummary>, String> {
    app.repos.projects().list().await
}

#[tauri::command]
pub async fn get_project(id: String, app: tauri::State<'_, AppContext>) -> Result<Project, String> {
    app.repos
        .projects()
        .get(&id)
        .await?
        .ok_or_else(|| format!("project not found: {id}"))
}

#[tauri::command]
pub async fn create_project(
    payload: CreateProject,
    app: tauri::State<'_, AppContext>,
) -> Result<Project, String> {
    create_project_inner(payload, &app).await
}

async fn create_project_inner(payload: CreateProject, app: &AppContext) -> Result<Project, String> {
    let cloud = app.cloud.clone();
    let pool = app.db.0.clone();
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
                    "SELECT 1 FROM projects WHERE id = ?1 AND tenant_id = 'local'",
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
            "INSERT INTO projects (id, name, description, created_at, updated_at, tenant_id)
             VALUES (?1, ?2, ?3, ?4, ?4, 'local')",
            rusqlite::params![id, payload.name, payload.description, now],
        )
        .map_err(|e| e.to_string())?;

        let project = conn
            .query_row(
                "SELECT id, name, description, created_at, updated_at
                   FROM projects
                  WHERE id = ?1 AND tenant_id = 'local'",
                rusqlite::params![id],
                map_project,
            )
            .map_err(|e| e.to_string())?;

        ensure_project_board_columns(
            &conn,
            &project.id,
            &project.created_at,
            payload.board_preset_id.as_deref(),
        )?;

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
    app: tauri::State<'_, AppContext>,
) -> Result<Project, String> {
    let cloud = app.cloud.clone();
    let project = app.repos.projects().update(&id, payload).await?;
    cloud_upsert_project!(cloud, project);
    Ok(project)
}

#[tauri::command]
pub async fn delete_project(id: String, app: tauri::State<'_, AppContext>) -> Result<(), String> {
    let cloud = app.cloud.clone();
    app.repos.projects().delete(&id).await?;
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
            "SELECT EXISTS(
                SELECT 1 FROM project_agents
                 WHERE project_id = ?1
                   AND agent_id = ?2
                   AND tenant_id = COALESCE((SELECT tenant_id FROM projects WHERE id = ?1), 'local')
            )",
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

#[tauri::command]
pub async fn list_project_agents(
    project_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<Agent>, String> {
    app.repos.projects().list_agents(&project_id).await
}

// `ProjectAgentWithMeta` is defined in `models/project.rs` so the repo trait
// can return it. Re-imported below for in-module use.
use crate::models::project::ProjectAgentWithMeta;

#[tauri::command]
pub async fn list_project_agents_with_meta(
    project_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<ProjectAgentWithMeta>, String> {
    app.repos
        .projects()
        .list_agents_with_meta(&project_id)
        .await
}

#[tauri::command]
pub async fn list_agent_projects(
    agent_id: String,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<Project>, String> {
    app.repos.projects().list_for_agent(&agent_id).await
}

#[tauri::command]
pub async fn add_agent_to_project(
    project_id: String,
    agent_id: String,
    is_default: bool,
    app: tauri::State<'_, AppContext>,
) -> Result<ProjectAgent, String> {
    let cloud = app.cloud.clone();
    let pa = app
        .repos
        .projects()
        .add_agent(&project_id, &agent_id, is_default)
        .await?;
    // Cloud mirror is fire-and-forget — UI shouldn't wait on it.
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
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    let cloud = app.cloud.clone();
    let pid_for_cloud = project_id.clone();
    let aid_for_cloud = agent_id.clone();
    app.repos
        .projects()
        .remove_agent(&project_id, &agent_id)
        .await?;
    if let Some(client) = cloud.get() {
        tokio::spawn(async move {
            if let Err(e) = client
                .delete_project_agent(&pid_for_cloud, &aid_for_cloud)
                .await
            {
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

#[tauri::command]
pub async fn create_project_workspace_dir(project_id: String, path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || workspace::create_project_workspace_dir(&project_id, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rename_project_workspace_entry(
    project_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        workspace::rename_project_workspace_entry(&project_id, &from, &to)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdArgs {
    id: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectArgs {
    payload: crate::models::project::CreateProject,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProjectArgs {
    id: String,
    payload: crate::models::project::UpdateProject,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectIdArgs {
    project_id: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentIdArgs {
    agent_id: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddAgentToProjectArgs {
    project_id: String,
    agent_id: String,
    #[serde(default)]
    is_default: bool,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveAgentFromProjectArgs {
    project_id: String,
    agent_id: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPathArgs {
    project_id: String,
    #[serde(default)]
    path: Option<String>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectFileArgs {
    project_id: String,
    path: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteProjectFileArgs {
    project_id: String,
    path: String,
    content: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameProjectEntryArgs {
    project_id: String,
    from: String,
    to: String,
}

pub fn register_http(reg: &mut crate::shim::registry::Registry) {
    reg.register("list_projects", |ctx, _args| async move {
        let result = ctx.repos.projects().list().await?;
        serde_json::to_value(result).map_err(|e| e.to_string())
    });
    reg.register("get_project", |ctx, args| async move {
        let IdArgs { id } = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let result = ctx
            .repos
            .projects()
            .get(&id)
            .await?
            .ok_or_else(|| format!("project not found: {id}"))?;
        serde_json::to_value(result).map_err(|e| e.to_string())
    });
    reg.register("create_project", |ctx, args| async move {
        let a: CreateProjectArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let r = create_project_inner(a.payload, &ctx).await?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    });
    reg.register("update_project", |ctx, args| async move {
        let a: UpdateProjectArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let cloud = ctx.cloud.clone();
        let project = ctx.repos.projects().update(&a.id, a.payload).await?;
        cloud_upsert_project!(cloud, project);
        serde_json::to_value(project).map_err(|e| e.to_string())
    });
    reg.register("delete_project", |ctx, args| async move {
        let IdArgs { id } = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let cloud = ctx.cloud.clone();
        ctx.repos.projects().delete(&id).await?;
        cloud_delete!(cloud, "projects", id);
        Ok(serde_json::Value::Null)
    });
    reg.register("list_project_agents", |ctx, args| async move {
        let ProjectIdArgs { project_id } =
            serde_json::from_value(args).map_err(|e| e.to_string())?;
        let r = ctx.repos.projects().list_agents(&project_id).await?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    });
    reg.register("list_project_agents_with_meta", |ctx, args| async move {
        let ProjectIdArgs { project_id } =
            serde_json::from_value(args).map_err(|e| e.to_string())?;
        let r = ctx
            .repos
            .projects()
            .list_agents_with_meta(&project_id)
            .await?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    });
    reg.register("list_agent_projects", |ctx, args| async move {
        let AgentIdArgs { agent_id } = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let r = ctx.repos.projects().list_for_agent(&agent_id).await?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    });
    reg.register("add_agent_to_project", |ctx, args| async move {
        let a: AddAgentToProjectArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let cloud = ctx.cloud.clone();
        let pa = ctx
            .repos
            .projects()
            .add_agent(&a.project_id, &a.agent_id, a.is_default)
            .await?;
        if let Some(client) = cloud.get() {
            let pa_clone = pa.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_project_agent(&pa_clone).await {
                    tracing::warn!("cloud upsert project_agent: {}", e);
                }
            });
        }
        serde_json::to_value(pa).map_err(|e| e.to_string())
    });
    reg.register("remove_agent_from_project", |ctx, args| async move {
        let a: RemoveAgentFromProjectArgs =
            serde_json::from_value(args).map_err(|e| e.to_string())?;
        let cloud = ctx.cloud.clone();
        let pid = a.project_id.clone();
        let aid = a.agent_id.clone();
        ctx.repos
            .projects()
            .remove_agent(&a.project_id, &a.agent_id)
            .await?;
        if let Some(client) = cloud.get() {
            tokio::spawn(async move {
                if let Err(e) = client.delete_project_agent(&pid, &aid).await {
                    tracing::warn!("cloud delete project_agent: {}", e);
                }
            });
        }
        Ok(serde_json::Value::Null)
    });
    reg.register("get_project_workspace_path", |_ctx, args| async move {
        let ProjectIdArgs { project_id } =
            serde_json::from_value(args).map_err(|e| e.to_string())?;
        Ok(serde_json::Value::String(get_project_workspace_path(
            project_id,
        )))
    });
    reg.register("list_project_workspace_files", |_ctx, args| async move {
        let a: ProjectPathArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let r = list_project_workspace_files(a.project_id, a.path).await?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    });
    reg.register("read_project_workspace_file", |_ctx, args| async move {
        let a: ProjectFileArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        let r = read_project_workspace_file(a.project_id, a.path).await?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    });
    reg.register("write_project_workspace_file", |_ctx, args| async move {
        let a: WriteProjectFileArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        write_project_workspace_file(a.project_id, a.path, a.content).await?;
        Ok(serde_json::Value::Null)
    });
    reg.register("delete_project_workspace_file", |_ctx, args| async move {
        let a: ProjectFileArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        delete_project_workspace_file(a.project_id, a.path).await?;
        Ok(serde_json::Value::Null)
    });
    reg.register("create_project_workspace_dir", |_ctx, args| async move {
        let a: ProjectFileArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        create_project_workspace_dir(a.project_id, a.path).await?;
        Ok(serde_json::Value::Null)
    });
    reg.register("rename_project_workspace_entry", |_ctx, args| async move {
        let a: RenameProjectEntryArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
        rename_project_workspace_entry(a.project_id, a.from, a.to).await?;
        Ok(serde_json::Value::Null)
    });
}
