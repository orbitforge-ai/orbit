use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::json;
use tokio::process::Command;
use tracing::info;
use ulid::Ulid;

use crate::executor::llm_provider::ToolDefinition;
use crate::executor::session_worktree::{self, SessionWorktreeState};
use crate::executor::workspace;

use super::{context::ToolExecutionContext, ToolHandler};

pub struct WorktreeTool;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeEntry {
    path: String,
    branch: Option<String>,
    head: Option<String>,
    bare: bool,
    detached: bool,
    locked: Option<String>,
    prunable: Option<String>,
    is_active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeListResult {
    active_workspace: String,
    current_worktree: Option<serde_json::Value>,
    worktrees: Vec<WorktreeEntry>,
}

#[async_trait::async_trait]
impl ToolHandler for WorktreeTool {
    fn name(&self) -> &'static str {
        "worktree"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Create an isolated git worktree for safe experimentation. Actions: create, exit, and list.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "exit", "list"],
                        "description": "Action to perform"
                    },
                    "name": {
                        "type": "string",
                        "description": "Optional worktree name for create"
                    },
                    "base_branch": {
                        "type": "string",
                        "description": "Branch, commit, or ref to base the new worktree on. Defaults to HEAD."
                    },
                    "keep_changes": {
                        "type": "boolean",
                        "description": "For exit: keep the worktree and branch when true. Defaults to true."
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &serde_json::Value,
        _app: &tauri::AppHandle,
        run_id: &str,
    ) -> Result<(String, bool), String> {
        let action = input["action"]
            .as_str()
            .ok_or("worktree: missing 'action' field")?;

        match action {
            "create" => create_worktree(ctx, input, run_id).await,
            "exit" => exit_worktree(ctx, input).await,
            "list" => list_worktrees(ctx).await,
            other => Err(format!("worktree: unknown action '{}'", other)),
        }
    }
}

async fn create_worktree(
    ctx: &ToolExecutionContext,
    input: &serde_json::Value,
    run_id: &str,
) -> Result<(String, bool), String> {
    if ctx.current_worktree().is_some() {
        return Err(
            "worktree: already using a managed worktree for this session; exit it before creating another"
                .to_string(),
        );
    }

    let db = ctx.db.as_ref().ok_or("worktree: no database available")?;
    let session_id = ctx
        .current_session_id
        .as_deref()
        .ok_or("worktree: no current session available")?;
    let name = normalize_worktree_name(input["name"].as_str());
    let base_branch = input["base_branch"].as_str().unwrap_or("HEAD");
    let worktrees_dir = workspace::agent_worktrees_dir(&ctx.agent_id);
    std::fs::create_dir_all(&worktrees_dir)
        .map_err(|e| format!("worktree: failed to create worktrees directory: {}", e))?;

    let worktree_path = worktrees_dir.join(&name);
    if worktree_path.exists() {
        return Err(format!(
            "worktree: target path already exists for '{}'",
            worktree_path.display()
        ));
    }

    let branch = format!("orbit/{}", name);
    let main_workspace = ctx.main_workspace_root();
    ensure_git_repo(&main_workspace).await?;

    info!(
        run_id = run_id,
        name = name,
        branch = branch,
        path = %worktree_path.display(),
        "agent tool: worktree create"
    );

    run_git(
        &main_workspace,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &path_string(&worktree_path),
            base_branch,
        ],
    )
    .await?;

    let state = SessionWorktreeState {
        name: name.clone(),
        branch: branch.clone(),
        path: worktree_path.clone(),
    };
    session_worktree::set_session_worktree_state(db, session_id, Some(&state)).await?;
    sync_session_worktree_cloud(ctx, session_id, Some(&state)).await;
    ctx.set_current_worktree(Some(state.clone()));

    let result = serde_json::to_string_pretty(&json!({
        "status": "created",
        "worktreeName": name,
        "worktreePath": worktree_path,
        "branch": branch,
        "workspace": ctx.workspace_root(),
    }))
    .map_err(|e| format!("worktree: failed to serialize result: {}", e))?;
    Ok((result, false))
}

async fn exit_worktree(
    ctx: &ToolExecutionContext,
    input: &serde_json::Value,
) -> Result<(String, bool), String> {
    let current = match ctx.current_worktree() {
        Some(state) => state,
        None => {
            return Ok((
                serde_json::to_string_pretty(&json!({
                    "status": "unchanged",
                    "message": "Session is already using the main workspace.",
                    "workspace": ctx.workspace_root(),
                }))
                .map_err(|e| format!("worktree: failed to serialize result: {}", e))?,
                false,
            ));
        }
    };
    let keep_changes = input["keep_changes"].as_bool().unwrap_or(true);
    let db = ctx.db.as_ref().ok_or("worktree: no database available")?;
    let session_id = ctx
        .current_session_id
        .as_deref()
        .ok_or("worktree: no current session available")?;
    let main_workspace = ctx.main_workspace_root();

    let mut notes = Vec::new();
    if !keep_changes {
        run_git(
            &main_workspace,
            &["worktree", "remove", &path_string(&current.path), "--force"],
        )
        .await?;

        if let Err(err) = run_git(&main_workspace, &["branch", "-D", &current.branch]).await {
            notes.push(format!(
                "Removed the worktree directory, but branch '{}' could not be deleted: {}",
                current.branch, err
            ));
        }
    }

    session_worktree::set_session_worktree_state(db, session_id, None).await?;
    sync_session_worktree_cloud(ctx, session_id, None).await;
    ctx.set_current_worktree(None);

    let result = serde_json::to_string_pretty(&json!({
        "status": if keep_changes { "exited" } else { "removed" },
        "workspace": ctx.workspace_root(),
        "previousWorktree": {
            "name": current.name,
            "branch": current.branch,
            "path": current.path,
        },
        "keptChanges": keep_changes,
        "notes": notes,
    }))
    .map_err(|e| format!("worktree: failed to serialize result: {}", e))?;
    Ok((result, false))
}

async fn list_worktrees(ctx: &ToolExecutionContext) -> Result<(String, bool), String> {
    let repo_root = ctx
        .current_worktree()
        .map(|state| state.path)
        .unwrap_or_else(|| ctx.main_workspace_root());
    ensure_git_repo(&repo_root).await?;

    let output = run_git(&repo_root, &["worktree", "list", "--porcelain"]).await?;
    let worktrees = parse_worktree_list(&output, &ctx.workspace_root());
    let current_worktree = ctx.current_worktree().map(|state| {
        json!({
            "name": state.name,
            "branch": state.branch,
            "path": state.path,
        })
    });
    let result = WorktreeListResult {
        active_workspace: ctx.workspace_root().to_string_lossy().to_string(),
        current_worktree,
        worktrees,
    };
    let serialized = serde_json::to_string_pretty(&result)
        .map_err(|e| format!("worktree: failed to serialize result: {}", e))?;
    Ok((serialized, false))
}

fn normalize_worktree_name(name: Option<&str>) -> String {
    match name {
        Some(name) => {
            let slug = workspace::slugify(name);
            if slug.is_empty() {
                format!("wt-{}", &Ulid::new().to_string().to_lowercase()[..8])
            } else {
                slug
            }
        }
        None => format!("wt-{}", &Ulid::new().to_string().to_lowercase()[..8]),
    }
}

async fn ensure_git_repo(path: &Path) -> Result<(), String> {
    run_git(path, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map(|_| ())
        .map_err(|_| {
            format!(
                "worktree: '{}' is not inside a git repository",
                path.display()
            )
        })
}

async fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("worktree: failed to run git {:?}: {}", args, e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if stderr.is_empty() { stdout } else { stderr };
        Err(if message.is_empty() {
            format!("worktree: git {:?} failed", args)
        } else {
            format!("worktree: {}", message)
        })
    }
}

fn parse_worktree_list(output: &str, active_workspace: &Path) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    let active_workspace = active_workspace.to_string_lossy().to_string();

    for line in output.lines() {
        if line.trim().is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: rest.to_string(),
                branch: None,
                head: None,
                bare: false,
                detached: false,
                locked: None,
                prunable: None,
                is_active: rest == active_workspace,
            });
            continue;
        }

        let Some(entry) = current.as_mut() else {
            continue;
        };

        if let Some(rest) = line.strip_prefix("branch refs/heads/") {
            entry.branch = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            entry.head = Some(rest.to_string());
        } else if line == "bare" {
            entry.bare = true;
        } else if line == "detached" {
            entry.detached = true;
        } else if let Some(rest) = line.strip_prefix("locked ") {
            entry.locked = Some(rest.to_string());
        } else if line == "locked" {
            entry.locked = Some("locked".to_string());
        } else if let Some(rest) = line.strip_prefix("prunable ") {
            entry.prunable = Some(rest.to_string());
        }
    }

    if let Some(entry) = current.take() {
        entries.push(entry);
    }

    entries
}

fn path_string(path: &PathBuf) -> String {
    path.to_string_lossy().to_string()
}

async fn sync_session_worktree_cloud(
    ctx: &ToolExecutionContext,
    session_id: &str,
    state: Option<&SessionWorktreeState>,
) {
    let Some(client) = ctx.cloud_client.clone() else {
        return;
    };
    let session_id = session_id.to_string();
    let body = match state {
        Some(state) => json!({
            "worktree_name": state.name,
            "worktree_branch": state.branch,
            "worktree_path": state.path.to_string_lossy().to_string(),
            "updated_at": chrono::Utc::now().to_rfc3339(),
        }),
        None => json!({
            "worktree_name": serde_json::Value::Null,
            "worktree_branch": serde_json::Value::Null,
            "worktree_path": serde_json::Value::Null,
            "updated_at": chrono::Utc::now().to_rfc3339(),
        }),
    };
    tokio::spawn(async move {
        if let Err(err) = client.patch_by_id("chat_sessions", &session_id, body).await {
            tracing::warn!("cloud patch session worktree {}: {}", session_id, err);
        }
    });
}
