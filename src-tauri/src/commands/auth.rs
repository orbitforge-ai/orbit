use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::auth::{self, AuthMode, AuthSession, AuthState};
use crate::db::cloud::{CloudClientState, SupabaseClient};
use crate::db::DbPool;
use crate::executor::keychain;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Supabase REST response types
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
struct SupabaseUser {
    id: String,
    email: Option<String>,
}

#[derive(Deserialize)]
struct SupabaseAuthResponse {
    // Optional: absent when Supabase requires email confirmation before issuing a session
    access_token: Option<String>,
    refresh_token: Option<String>,
    user: SupabaseUser,
}

#[derive(Serialize)]
struct PasswordLoginRequest<'a> {
    email: &'a str,
    password: &'a str,
}

/// Extract the human-readable message from a Supabase error body.
/// Supabase returns `{"code":400,"error_code":"...","msg":"..."}`.
/// Falls back to the raw body if parsing fails.
fn supabase_error_msg(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("msg")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| body.to_owned())
}

// ---------------------------------------------------------------------------
// Frontend-facing auth state DTO
// ---------------------------------------------------------------------------
#[derive(Serialize, Clone)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum AuthStateDto {
    Unset,
    Offline,
    Cloud { email: String },
}

// ---------------------------------------------------------------------------
// get_auth_state — called on app start to determine which screen to show
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn get_auth_state(auth: tauri::State<'_, AuthState>) -> Result<AuthStateDto, String> {
    let mode = auth.get().await;
    let dto = match mode {
        AuthMode::Unset => AuthStateDto::Unset,
        AuthMode::Offline => AuthStateDto::Offline,
        AuthMode::Cloud(session) => AuthStateDto::Cloud {
            email: session.email,
        },
    };
    Ok(dto)
}

// ---------------------------------------------------------------------------
// set_offline_mode — user chose "Continue offline" on the auth screen
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn set_offline_mode(auth: tauri::State<'_, AuthState>) -> Result<(), String> {
    auth.set(AuthMode::Offline).await;
    let data_dir = crate::data_dir();
    auth::persist_auth_state(&data_dir, &AuthMode::Offline);
    info!("Auth mode set to Offline");
    Ok(())
}

// ---------------------------------------------------------------------------
// login — authenticate with Supabase and populate Keychain with API keys
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn login(
    email: String,
    password: String,
    auth: tauri::State<'_, AuthState>,
    cloud_state: tauri::State<'_, CloudClientState>,
    db: tauri::State<'_, DbPool>,
) -> Result<AuthStateDto, String> {
    let (supabase_url, anon_key) = auth::supabase_credentials()?;

    let http = reqwest::Client::new();

    // Authenticate with Supabase Auth
    let auth_url = format!("{}/auth/v1/token?grant_type=password", supabase_url);
    let response = http
        .post(&auth_url)
        .header("apikey", &anon_key)
        .header("Content-Type", "application/json")
        .json(&PasswordLoginRequest {
            email: &email,
            password: &password,
        })
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(supabase_error_msg(&body));
    }

    let auth_data: SupabaseAuthResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse auth response: {}", e))?;

    let access_token = auth_data
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "Login failed: no session returned. Check your credentials.".to_string())?;

    let session = AuthSession {
        user_id: auth_data.user.id.clone(),
        email: auth_data.user.email.unwrap_or_else(|| email.clone()),
        access_token: access_token.clone(),
        refresh_token: auth_data.refresh_token.unwrap_or_default(),
    };

    // Build the cloud client and store it in managed state
    let cloud_client = Arc::new(SupabaseClient::new(
        supabase_url.clone(),
        anon_key.clone(),
        access_token,
        session.refresh_token.clone(),
        session.user_id.clone(),
        session.email.clone(),
    ));
    cloud_state.set(Some(cloud_client.clone()));

    // Sync API keys from Supabase Vault to local Keychain (best-effort)
    sync_api_keys_from_vault(&http, &supabase_url, &anon_key, &session.access_token).await;

    if !crate::db::cloud::cloud_sync_disabled() {
        // Push local → cloud in background (best-effort, non-blocking)
        let pool_for_push = db.0.clone();
        let client_for_push = cloud_client.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = client_for_push.push_local_data(&pool_for_push).await {
                warn!("Login push failed: {}", e);
            }
        });

        // Pull cloud → local blocking: data must be in SQLite before the frontend
        // receives the auth state and starts querying.
        let pool = db.0.clone();
        if let Err(e) = cloud_client.pull_all_data(&pool).await {
            warn!("Login pull failed: {}", e);
        }
    } else {
        info!("Cloud sync disabled — skipping login push/pull");
    }

    let email_out = session.email.clone();
    let mode = AuthMode::Cloud(session);
    auth.set(mode.clone()).await;

    let data_dir = crate::data_dir();
    auth::persist_auth_state(&data_dir, &mode);

    info!("User logged in: {}", email_out);
    Ok(AuthStateDto::Cloud { email: email_out })
}

// ---------------------------------------------------------------------------
// register — create a new Supabase account and sign in
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn register(
    email: String,
    password: String,
    auth: tauri::State<'_, AuthState>,
    cloud_state: tauri::State<'_, CloudClientState>,
    db: tauri::State<'_, DbPool>,
) -> Result<AuthStateDto, String> {
    let (supabase_url, anon_key) = auth::supabase_credentials()?;

    let http = reqwest::Client::new();

    let signup_url = format!("{}/auth/v1/signup", supabase_url);
    let response = http
        .post(&signup_url)
        .header("apikey", &anon_key)
        .header("Content-Type", "application/json")
        .json(&PasswordLoginRequest {
            email: &email,
            password: &password,
        })
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(supabase_error_msg(&body));
    }

    let auth_data: SupabaseAuthResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse signup response: {}", e))?;

    // If no access_token, Supabase requires email confirmation before issuing a session
    let access_token = auth_data
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            "Account created! Check your email to confirm your address before signing in."
                .to_string()
        })?;

    let session = AuthSession {
        user_id: auth_data.user.id.clone(),
        email: auth_data.user.email.unwrap_or_else(|| email.clone()),
        access_token: access_token.clone(),
        refresh_token: auth_data.refresh_token.unwrap_or_default(),
    };

    let cloud_client = Arc::new(SupabaseClient::new(
        supabase_url.clone(),
        anon_key.clone(),
        access_token,
        session.refresh_token.clone(),
        session.user_id.clone(),
        session.email.clone(),
    ));
    cloud_state.set(Some(cloud_client.clone()));

    if !crate::db::cloud::cloud_sync_disabled() {
        // Push local → cloud in background (best-effort, non-blocking)
        let pool_for_push = db.0.clone();
        let client_for_push = cloud_client.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = client_for_push.push_local_data(&pool_for_push).await {
                warn!("Registration push failed: {}", e);
            }
        });

        // Pull cloud → local blocking so data is ready before the frontend renders
        let pool = db.0.clone();
        if let Err(e) = cloud_client.pull_all_data(&pool).await {
            warn!("Registration pull failed: {}", e);
        }
    } else {
        info!("Cloud sync disabled — skipping registration push/pull");
    }

    let email_out = session.email.clone();
    let mode = AuthMode::Cloud(session);
    auth.set(mode.clone()).await;

    let data_dir = crate::data_dir();
    auth::persist_auth_state(&data_dir, &mode);

    info!("User registered: {}", email_out);
    Ok(AuthStateDto::Cloud { email: email_out })
}

// ---------------------------------------------------------------------------
// logout — clear session, return to auth screen
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn logout(
    auth: tauri::State<'_, AuthState>,
    cloud_state: tauri::State<'_, CloudClientState>,
) -> Result<(), String> {
    // Best-effort: call Supabase signout endpoint
    if let AuthMode::Cloud(session) = auth.get().await {
        if let Ok((supabase_url, anon_key)) = auth::supabase_credentials() {
            let client = reqwest::Client::new();
            let _ = client
                .post(format!("{}/auth/v1/logout", supabase_url))
                .header("apikey", &anon_key)
                .header("Authorization", format!("Bearer {}", session.access_token))
                .send()
                .await;
        }
    }

    // Clear the in-memory cloud client
    cloud_state.set(None);

    auth.set(AuthMode::Unset).await;
    let data_dir = crate::data_dir();
    auth::persist_auth_state(&data_dir, &AuthMode::Unset);

    info!("User logged out");
    Ok(())
}

// ---------------------------------------------------------------------------
// sync_api_keys_from_vault — fetch all user API keys from Supabase Vault
// and write to local macOS Keychain. Graceful: logs warnings, never errors.
// ---------------------------------------------------------------------------
async fn sync_api_keys_from_vault(
    client: &reqwest::Client,
    supabase_url: &str,
    anon_key: &str,
    access_token: &str,
) {
    // First, list which providers have keys stored
    let list_url = format!("{}/rest/v1/rpc/list_api_key_providers", supabase_url);
    let providers_response = client
        .post(&list_url)
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({}))
        .send()
        .await;

    let providers: Vec<String> = match providers_response {
        Ok(r) if r.status().is_success() => match r.json::<Vec<serde_json::Value>>().await {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|v| v.get("provider")?.as_str().map(String::from))
                .collect(),
            Err(e) => {
                warn!("Could not parse API key providers from Vault: {}", e);
                return;
            }
        },
        Ok(r) => {
            warn!(
                "list_api_key_providers returned {} — Vault may not be configured yet",
                r.status()
            );
            return;
        }
        Err(e) => {
            warn!("Failed to list API key providers: {}", e);
            return;
        }
    };

    // Fetch and write each key to Keychain
    let get_url = format!("{}/rest/v1/rpc/get_api_key", supabase_url);
    for provider in providers {
        let result = client
            .post(&get_url)
            .header("apikey", anon_key)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "p_provider": provider }))
            .send()
            .await;

        match result {
            Ok(r) if r.status().is_success() => match r.json::<Option<String>>().await {
                Ok(Some(key)) if !key.is_empty() => {
                    match keychain::store_api_key(&provider, &key) {
                        Ok(_) => info!("Synced API key for provider '{}' from Vault", provider),
                        Err(e) => warn!("Failed to write '{}' key to Keychain: {}", provider, e),
                    }
                }
                _ => {}
            },
            Ok(r) => warn!("get_api_key for '{}' returned {}", provider, r.status()),
            Err(e) => warn!("Failed to fetch API key for '{}': {}", provider, e),
        }
    }
}

// ---------------------------------------------------------------------------
// force_cloud_sync — manually trigger a full pull from Supabase
// Returns row counts per table so the caller can show diagnostic info.
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn force_cloud_sync(
    cloud: tauri::State<'_, CloudClientState>,
    db: tauri::State<'_, DbPool>,
) -> Result<std::collections::HashMap<String, usize>, String> {
    let client = cloud
        .get()
        .ok_or_else(|| "Not signed in to cloud".to_string())?;
    let pool = db.0.clone();
    client.pull_all_data_with_counts(&pool).await
}
