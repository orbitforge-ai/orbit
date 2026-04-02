use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::auth::{self, AuthMode, AuthSession, AuthState};
use crate::executor::keychain;

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
    access_token: String,
    refresh_token: String,
    user: SupabaseUser,
}

#[derive(Serialize)]
struct PasswordLoginRequest<'a> {
    email: &'a str,
    password: &'a str,
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
) -> Result<AuthStateDto, String> {
    let (supabase_url, anon_key) = auth::supabase_credentials()?;

    let client = reqwest::Client::new();

    // Authenticate with Supabase Auth
    let auth_url = format!("{}/auth/v1/token?grant_type=password", supabase_url);
    let response = client
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
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Login failed ({}): {}", status, body));
    }

    let auth_data: SupabaseAuthResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse auth response: {}", e))?;

    let session = AuthSession {
        user_id: auth_data.user.id,
        email: auth_data.user.email.unwrap_or_else(|| email.clone()),
        access_token: auth_data.access_token,
        refresh_token: auth_data.refresh_token,
    };

    // Sync API keys from Supabase Vault to local Keychain (best-effort)
    sync_api_keys_from_vault(&client, &supabase_url, &anon_key, &session.access_token).await;

    let email_out = session.email.clone();
    let mode = AuthMode::Cloud(session);
    auth.set(mode.clone()).await;

    let data_dir = crate::data_dir();
    auth::persist_auth_state(&data_dir, &mode);

    info!("User logged in: {}", email_out);
    Ok(AuthStateDto::Cloud { email: email_out })
}

// ---------------------------------------------------------------------------
// logout — clear session, return to auth screen
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn logout(auth: tauri::State<'_, AuthState>) -> Result<(), String> {
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
        Ok(r) if r.status().is_success() => {
            match r.json::<Vec<serde_json::Value>>().await {
                Ok(rows) => rows
                    .into_iter()
                    .filter_map(|v| v.get("provider")?.as_str().map(String::from))
                    .collect(),
                Err(e) => {
                    warn!("Could not parse API key providers from Vault: {}", e);
                    return;
                }
            }
        }
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
            Ok(r) if r.status().is_success() => {
                match r.json::<Option<String>>().await {
                    Ok(Some(key)) if !key.is_empty() => {
                        match keychain::store_api_key(&provider, &key) {
                            Ok(_) => info!("Synced API key for provider '{}' from Vault", provider),
                            Err(e) => warn!("Failed to write '{}' key to Keychain: {}", provider, e),
                        }
                    }
                    _ => {}
                }
            }
            Ok(r) => warn!("get_api_key for '{}' returned {}", provider, r.status()),
            Err(e) => warn!("Failed to fetch API key for '{}': {}", provider, e),
        }
    }
}
