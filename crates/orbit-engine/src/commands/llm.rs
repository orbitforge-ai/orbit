use serde::Serialize;
use ulid::Ulid;

use crate::app_context::AppContext;
#[cfg(feature = "desktop")]
use crate::executor::cli_common;
use crate::executor::engine::RunRequest;
use crate::executor::keychain;
use crate::executor::llm_provider::is_cli_provider;
use crate::executor::vercel::{self, VercelGatewayModelOption};
use crate::models::task::CreateTask;

#[tauri::command]
pub async fn set_api_key(
    provider: String,
    key: String,
    app: tauri::State<'_, AppContext>,
) -> Result<(), String> {
    set_api_key_inner(provider, key, &app).await
}

async fn set_api_key_inner(provider: String, key: String, app: &AppContext) -> Result<(), String> {
    let cloud = app.cloud.clone();
    let prov = provider.clone();
    let k = key.clone();
    tokio::task::spawn_blocking(move || keychain::store_api_key(&prov, &k))
        .await
        .map_err(|e| e.to_string())??;
    // Also push to Supabase Vault so other devices can sync.
    // Awaited (not fire-and-forget) so the call completes before the command returns.
    // Best-effort: a vault failure still returns Ok so the local key is usable.
    match cloud.get() {
        Some(client) => {
            if let Err(e) = client.upsert_api_key_in_vault(&provider, &key).await {
                tracing::warn!("vault upsert_api_key '{}': {}", provider, e);
            }
        }
        None => tracing::debug!("set_api_key: no cloud client, skipping vault sync"),
    }
    Ok(())
}

#[tauri::command]
pub async fn has_api_key(provider: String) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || Ok(keychain::has_api_key(&provider)))
        .await
        .map_err(|e: tokio::task::JoinError| e.to_string())?
}

#[tauri::command]
pub async fn delete_api_key(provider: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || keychain::delete_api_key(&provider))
        .await
        .map_err(|e| e.to_string())?
}

/// Readiness report for a provider. `kind` tells the Settings UI how to
/// render the row — API-key providers show an input, CLI providers show a
/// binary path / install hint.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub kind: &'static str,
    pub ready: bool,
    pub binary_path: Option<String>,
    pub message: Option<String>,
}

#[tauri::command]
pub async fn get_provider_status(provider: String) -> Result<ProviderStatus, String> {
    tokio::task::spawn_blocking(move || {
        if is_cli_provider(&provider) {
            #[cfg(feature = "desktop")]
            {
                let binary = match provider.as_str() {
                    "claude-cli" => "claude",
                    "codex-cli" => "codex",
                    _ => return Err(format!("unknown CLI provider: {}", provider)),
                };
                return match cli_common::resolve_cli(binary) {
                    Some(path) => Ok(ProviderStatus {
                        kind: "cli",
                        ready: true,
                        binary_path: Some(path.display().to_string()),
                        message: None,
                    }),
                    None => Ok(ProviderStatus {
                        kind: "cli",
                        ready: false,
                        binary_path: None,
                        message: Some(format!(
                            "`{}` binary not found on PATH. Install and authenticate it before selecting this provider.",
                            binary
                        )),
                    }),
                };
            }
            #[cfg(not(feature = "desktop"))]
            {
                Ok(ProviderStatus {
                    kind: "cli",
                    ready: false,
                    binary_path: None,
                    message: Some(
                        "CLI providers are desktop-only and unavailable in cloud mode."
                            .to_string(),
                    ),
                })
            }
        } else {
            Ok(ProviderStatus {
                kind: "api_key",
                ready: keychain::has_api_key(&provider),
                binary_path: None,
                message: None,
            })
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_vercel_gateway_models() -> Result<Vec<VercelGatewayModelOption>, String> {
    let api_key = tokio::task::spawn_blocking(|| keychain::retrieve_api_key("vercel").ok())
        .await
        .map_err(|e| e.to_string())?;
    vercel::list_gateway_models(api_key).await
}

/// Trigger an autonomous agent loop run.
/// Creates an ephemeral agent_loop task and dispatches it to the executor.
#[tauri::command]
pub async fn trigger_agent_loop(
    agent_id: String,
    goal: String,
    app: tauri::State<'_, AppContext>,
) -> Result<String, String> {
    trigger_agent_loop_inner(agent_id, goal, &app).await
}

async fn trigger_agent_loop_inner(
    agent_id: String,
    goal: String,
    app: &AppContext,
) -> Result<String, String> {
    let tx = app.executor_tx.0.clone();

    let task = app
        .repos
        .tasks()
        .create(CreateTask {
            name: format!("Agent loop: {}", &goal.chars().take(60).collect::<String>()),
            description: Some(goal.clone()),
            kind: "agent_loop".to_string(),
            config: serde_json::json!({ "goal": goal }),
            max_duration_seconds: Some(7200),
            max_retries: Some(0),
            retry_delay_seconds: Some(60),
            concurrency_policy: Some("allow".to_string()),
            tags: Some(Vec::new()),
            agent_id: Some(agent_id),
            project_id: None,
        })
        .await?;

    let run_id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let log_path = format!(
        "{}/.orbit/logs/{}.log",
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        run_id
    );

    app.repos
        .runs()
        .create_manual_run(&run_id, &task, None, &log_path, &now)
        .await?;

    let run_id_clone = run_id.clone();
    tx.send(RunRequest {
        run_id: run_id_clone,
        task,
        schedule_id: None,
        _trigger: "manual".to_string(),
        retry_count: 0,
        _parent_run_id: None,
        chain_depth: 0,
    })
    .map_err(|e| format!("failed to send to executor: {}", e))?;

    Ok(run_id)
}

mod http {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SetKeyArgs {
        provider: String,
        key: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ProviderArgs {
        provider: String,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TriggerLoopArgs {
        agent_id: String,
        goal: String,
    }

    pub fn register(reg: &mut crate::shim::registry::Registry) {
        reg.register("set_api_key", |ctx, args| async move {
            let a: SetKeyArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            set_api_key_inner(a.provider, a.key, &ctx).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("has_api_key", |_ctx, args| async move {
            let a: ProviderArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = has_api_key(a.provider).await?;
            Ok(serde_json::Value::Bool(r))
        });
        reg.register("delete_api_key", |_ctx, args| async move {
            let a: ProviderArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            delete_api_key(a.provider).await?;
            Ok(serde_json::Value::Null)
        });
        reg.register("get_provider_status", |_ctx, args| async move {
            let a: ProviderArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = get_provider_status(a.provider).await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("list_vercel_gateway_models", |_ctx, _args| async move {
            let r = list_vercel_gateway_models().await?;
            serde_json::to_value(r).map_err(|e| e.to_string())
        });
        reg.register("trigger_agent_loop", |ctx, args| async move {
            let a: TriggerLoopArgs = serde_json::from_value(args).map_err(|e| e.to_string())?;
            let r = trigger_agent_loop_inner(a.agent_id, a.goal, &ctx).await?;
            Ok(serde_json::Value::String(r))
        });
    }
}

pub use http::register as register_http;
