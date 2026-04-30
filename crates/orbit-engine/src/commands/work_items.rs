use crate::models::work_item::{CreateWorkItem, UpdateWorkItem, WorkItem};
use crate::models::work_item_comment::{CommentAuthor, WorkItemComment};

// ── Cloud helpers ─────────────────────────────────────────────────────────────

macro_rules! cloud_upsert_work_item {
    ($cloud:expr, $item:expr) => {
        if let Some(client) = $cloud.get() {
            let w = $item.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_work_item(&w).await {
                    tracing::warn!("cloud upsert work_item: {}", e);
                }
            });
        }
    };
}

macro_rules! cloud_upsert_work_item_comment {
    ($cloud:expr, $comment:expr) => {
        if let Some(client) = $cloud.get() {
            let c = $comment.clone();
            tokio::spawn(async move {
                if let Err(e) = client.upsert_work_item_comment(&c).await {
                    tracing::warn!("cloud upsert work_item_comment: {}", e);
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

// ── Work Item Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_work_items(
    project_id: String,
    board_id: Option<String>,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<Vec<WorkItem>, String> {
    app.repos.work_items().list(&project_id, board_id).await
}

#[tauri::command]
pub async fn get_work_item(
    id: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItem, String> {
    app.repos.work_items().get(&id).await
}

#[tauri::command]
pub async fn create_work_item(
    payload: CreateWorkItem,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItem, String> {
    let cloud = app.cloud.clone();
    let item = app.repos.work_items().create(payload).await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn update_work_item(
    id: String,
    payload: UpdateWorkItem,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItem, String> {
    let cloud = app.cloud.clone();
    let item = app.repos.work_items().update(&id, payload).await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn delete_work_item(
    id: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<(), String> {
    let cloud = app.cloud.clone();
    app.repos.work_items().delete(&id).await?;

    cloud_delete!(cloud, "work_items", id);
    Ok(())
}

#[tauri::command]
pub async fn claim_work_item(
    id: String,
    agent_id: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItem, String> {
    let cloud = app.cloud.clone();
    let item = app.repos.work_items().claim(&id, &agent_id).await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn move_work_item(
    id: String,
    column_id: Option<String>,
    position: Option<f64>,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItem, String> {
    let cloud = app.cloud.clone();
    let item = app
        .repos
        .work_items()
        .move_item(&id, column_id, position)
        .await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn reorder_work_items(
    project_id: String,
    board_id: Option<String>,
    status: Option<String>,
    column_id: Option<String>,
    ordered_ids: Vec<String>,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<(), String> {
    app.repos
        .work_items()
        .reorder(&project_id, board_id, status, column_id, ordered_ids)
        .await
}

#[tauri::command]
pub async fn block_work_item(
    id: String,
    reason: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItem, String> {
    let cloud = app.cloud.clone();
    let item = app.repos.work_items().block(&id, reason).await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

#[tauri::command]
pub async fn complete_work_item(
    id: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItem, String> {
    let cloud = app.cloud.clone();
    let item = app.repos.work_items().complete(&id).await?;

    cloud_upsert_work_item!(cloud, item);
    Ok(item)
}

// ── Work Item Comment Commands ────────────────────────────────────────────────

#[tauri::command]
pub async fn list_work_item_comments(
    work_item_id: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<Vec<WorkItemComment>, String> {
    app.repos.work_items().list_comments(&work_item_id).await
}

#[tauri::command]
pub async fn create_work_item_comment(
    work_item_id: String,
    body: String,
    author: CommentAuthor,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItemComment, String> {
    let cloud = app.cloud.clone();
    let comment = app
        .repos
        .work_items()
        .create_comment(&work_item_id, body, author)
        .await?;

    cloud_upsert_work_item_comment!(cloud, comment);
    Ok(comment)
}

#[tauri::command]
pub async fn update_work_item_comment(
    id: String,
    body: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<WorkItemComment, String> {
    let cloud = app.cloud.clone();
    let comment = app.repos.work_items().update_comment(&id, body).await?;

    cloud_upsert_work_item_comment!(cloud, comment);
    Ok(comment)
}

#[tauri::command]
pub async fn delete_work_item_comment(
    id: String,
    app: tauri::State<'_, crate::app_context::AppContext>,
) -> Result<(), String> {
    let cloud = app.cloud.clone();
    app.repos.work_items().delete_comment(&id).await?;

    cloud_delete!(cloud, "work_item_comments", id);
    Ok(())
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ListArgs {
        project_id: String,
        #[serde(default)]
        board_id: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct IdArgs {
        id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateArgs {
        payload: CreateWorkItem,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateArgs {
        id: String,
        payload: UpdateWorkItem,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ClaimArgs {
        id: String,
        agent_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct MoveArgs {
        id: String,
        #[serde(default)]
        column_id: Option<String>,
        #[serde(default)]
        position: Option<f64>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ReorderArgs {
        project_id: String,
        #[serde(default)]
        board_id: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        column_id: Option<String>,
        ordered_ids: Vec<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BlockArgs {
        id: String,
        reason: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WorkItemIdArgs {
        work_item_id: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateCommentArgs {
        work_item_id: String,
        body: String,
        author: CommentAuthor,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateCommentArgs {
        id: String,
        body: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_work_items", |ctx, args| async move {
            let a: ListArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx
                .repos
                .work_items()
                .list(&a.project_id, a.board_id)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_work_item", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx.repos.work_items().get(&a.id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_work_item", |ctx, args| async move {
            let a: CreateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx.repos.work_items().create(a.payload).await?;
            cloud_upsert_work_item!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_work_item", |ctx, args| async move {
            let a: UpdateArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx.repos.work_items().update(&a.id, a.payload).await?;
            cloud_upsert_work_item!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_work_item", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            ctx.repos.work_items().delete(&a.id).await?;
            cloud_delete!(cloud, "work_items", a.id);
            Ok(serde_json::Value::Null)
        });
        reg.register("claim_work_item", |ctx, args| async move {
            let a: ClaimArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx.repos.work_items().claim(&a.id, &a.agent_id).await?;
            cloud_upsert_work_item!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("move_work_item", |ctx, args| async move {
            let a: MoveArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx
                .repos
                .work_items()
                .move_item(&a.id, a.column_id, a.position)
                .await?;
            cloud_upsert_work_item!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("reorder_work_items", |ctx, args| async move {
            let a: ReorderArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            ctx.repos
                .work_items()
                .reorder(
                    &a.project_id,
                    a.board_id,
                    a.status,
                    a.column_id,
                    a.ordered_ids,
                )
                .await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("block_work_item", |ctx, args| async move {
            let a: BlockArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx.repos.work_items().block(&a.id, a.reason).await?;
            cloud_upsert_work_item!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("complete_work_item", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx.repos.work_items().complete(&a.id).await?;
            cloud_upsert_work_item!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_work_item_comments", |ctx, args| async move {
            let a: WorkItemIdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = ctx
                .repos
                .work_items()
                .list_comments(&a.work_item_id)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_work_item_comment", |ctx, args| async move {
            let a: CreateCommentArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx
                .repos
                .work_items()
                .create_comment(&a.work_item_id, a.body, a.author)
                .await?;
            cloud_upsert_work_item_comment!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("update_work_item_comment", |ctx, args| async move {
            let a: UpdateCommentArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let r = ctx.repos.work_items().update_comment(&a.id, a.body).await?;
            cloud_upsert_work_item_comment!(cloud, r);
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("delete_work_item_comment", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            ctx.repos.work_items().delete_comment(&a.id).await?;
            cloud_delete!(cloud, "work_item_comments", a.id);
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
