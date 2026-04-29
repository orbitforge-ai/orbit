//! Inter-agent message bus commands.
//!
//! Read paths and CRUD on subscriptions go through the repo trait. The
//! cloud mirror side-effect (upsert / patch / delete on Supabase) stays in
//! this command file because it's a transport concern, not a data-store one,
//! and v1 fires-and-forgets it on a background tokio task.

use crate::app_context::AppContext;
use crate::db::cloud::CloudClientState;
use crate::models::bus::{BusMessage, BusSubscription, CreateBusSubscription, PaginatedBusThread};

#[tauri::command]
pub async fn list_bus_messages(
    agent_id: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<BusMessage>, String> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    app.repos.bus_messages().list(agent_id, limit, offset).await
}

#[tauri::command]
pub async fn get_bus_thread(
    agent_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
    app: tauri::State<'_, AppContext>,
) -> Result<PaginatedBusThread, String> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    app.repos
        .bus_messages()
        .thread_for_agent(&agent_id, limit, offset)
        .await
}

#[tauri::command]
pub async fn list_bus_subscriptions(
    agent_id: Option<String>,
    app: tauri::State<'_, AppContext>,
) -> Result<Vec<BusSubscription>, String> {
    app.repos.bus_subscriptions().list(agent_id).await
}

#[tauri::command]
pub async fn create_bus_subscription(
    payload: CreateBusSubscription,
    app: tauri::State<'_, AppContext>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<BusSubscription, String> {
    let cloud = cloud.inner().clone();
    let sub = app.repos.bus_subscriptions().create(payload).await?;

    // Mirror to cloud lazily — this is best-effort and shouldn't block the UI.
    if let Some(client) = cloud.get() {
        let s = sub.clone();
        tokio::spawn(async move {
            if let Err(e) = client.upsert_bus_subscription(&s).await {
                tracing::warn!("cloud upsert bus_subscription: {}", e);
            }
        });
    }
    Ok(sub)
}

#[tauri::command]
pub async fn toggle_bus_subscription(
    id: String,
    enabled: bool,
    app: tauri::State<'_, AppContext>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let cloud = cloud.inner().clone();
    let now = chrono::Utc::now().to_rfc3339();
    app.repos
        .bus_subscriptions()
        .set_enabled(&id, enabled)
        .await?;

    if let Some(client) = cloud.get() {
        let id_for_cloud = id.clone();
        tokio::spawn(async move {
            let _ = client
                .patch_by_id(
                    "bus_subscriptions",
                    &id_for_cloud,
                    serde_json::json!({"enabled": enabled, "updated_at": now}),
                )
                .await;
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_bus_subscription(
    id: String,
    app: tauri::State<'_, AppContext>,
    cloud: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    let cloud = cloud.inner().clone();
    app.repos.bus_subscriptions().delete(&id).await?;

    if let Some(client) = cloud.get() {
        let id_for_cloud = id.clone();
        tokio::spawn(async move {
            let _ = client
                .delete_by_id("bus_subscriptions", &id_for_cloud)
                .await;
        });
    }
    Ok(())
}

mod http {
    use super::*;

    #[derive(serde::Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct ListMessagesArgs {
        agent_id: Option<String>,
        limit: Option<i64>,
        offset: Option<i64>,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct ThreadArgs {
        agent_id: String,
        limit: Option<i64>,
        offset: Option<i64>,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct ListSubsArgs {
        agent_id: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CreateSubArgs {
        payload: CreateBusSubscription,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ToggleSubArgs {
        id: String,
        enabled: bool,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct IdArgs {
        id: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("list_bus_messages", |ctx, args| async move {
            let a: ListMessagesArgs = if args.is_null() {
                Default::default()
            } else {
                serde_json::from_value(args).map_err(|e| e.to_string())?
            };
            let limit = a.limit.unwrap_or(50);
            let offset = a.offset.unwrap_or(0);
            let r = ctx
                .repos
                .bus_messages()
                .list(a.agent_id, limit, offset)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("get_bus_thread", |ctx, args| async move {
            let a: ThreadArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let limit = a.limit.unwrap_or(50);
            let offset = a.offset.unwrap_or(0);
            let r = ctx
                .repos
                .bus_messages()
                .thread_for_agent(&a.agent_id, limit, offset)
                .await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_bus_subscriptions", |ctx, args| async move {
            let a: ListSubsArgs = if args.is_null() {
                Default::default()
            } else {
                serde_json::from_value(args).map_err(|e| e.to_string())?
            };
            let r = ctx.repos.bus_subscriptions().list(a.agent_id).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("create_bus_subscription", |ctx, args| async move {
            let a: CreateSubArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let sub = ctx.repos.bus_subscriptions().create(a.payload).await?;
            if let Some(client) = cloud.get() {
                let s = sub.clone();
                tokio::spawn(async move {
                    if let Err(e) = client.upsert_bus_subscription(&s).await {
                        tracing::warn!("cloud upsert bus_subscription: {}", e);
                    }
                });
            }
            serde_json::to_value(sub).map_err(|e| e.to_string())
        });
        reg.register("toggle_bus_subscription", |ctx, args| async move {
            let a: ToggleSubArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            let now = chrono::Utc::now().to_rfc3339();
            ctx.repos
                .bus_subscriptions()
                .set_enabled(&a.id, a.enabled)
                .await?;
            if let Some(client) = cloud.get() {
                let id = a.id.clone();
                let enabled = a.enabled;
                tokio::spawn(async move {
                    let _ = client
                        .patch_by_id(
                            "bus_subscriptions",
                            &id,
                            serde_json::json!({"enabled": enabled, "updated_at": now}),
                        )
                        .await;
                });
            }
            Ok(serde_json::Value::Null)
        });
        reg.register("delete_bus_subscription", |ctx, args| async move {
            let a: IdArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let cloud = ctx.cloud.clone();
            ctx.repos.bus_subscriptions().delete(&a.id).await?;
            if let Some(client) = cloud.get() {
                let id = a.id.clone();
                tokio::spawn(async move {
                    let _ = client.delete_by_id("bus_subscriptions", &id).await;
                });
            }
            Ok(serde_json::Value::Null)
        });
    }
}

pub use http::register as register_http;
