use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Supabase credentials — set these as environment variables at build time:
//   SUPABASE_URL=https://yourproject.supabase.co
//   SUPABASE_ANON_KEY=eyJ...
// ---------------------------------------------------------------------------
pub const SUPABASE_URL: Option<&str> = option_env!("SUPABASE_URL");
pub const SUPABASE_ANON_KEY: Option<&str> = option_env!("SUPABASE_ANON_KEY");

pub fn supabase_credentials() -> Result<(String, String), String> {
    let url = SUPABASE_URL
        .ok_or_else(|| "Cloud sync not configured: SUPABASE_URL missing at build time".to_string())?
        .to_string();
    let key = SUPABASE_ANON_KEY
        .ok_or_else(|| {
            "Cloud sync not configured: SUPABASE_ANON_KEY missing at build time".to_string()
        })?
        .to_string();
    Ok((url, key))
}

// ---------------------------------------------------------------------------
// Auth session (stored when user is logged in to cloud mode)
// ---------------------------------------------------------------------------
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthSession {
    pub user_id: String,
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
}

// ---------------------------------------------------------------------------
// Auth mode — three states the app can be in
// ---------------------------------------------------------------------------
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum AuthMode {
    /// First launch or just logged out — show auth screen.
    Unset,
    /// User chose "Continue offline" — skip auth screen on future launches.
    Offline,
    /// User is logged in to cloud sync.
    Cloud(AuthSession),
}

impl AuthMode {
    pub fn is_cloud(&self) -> bool {
        matches!(self, AuthMode::Cloud(_))
    }

    pub fn session(&self) -> Option<&AuthSession> {
        match self {
            AuthMode::Cloud(s) => Some(s),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Managed state
// ---------------------------------------------------------------------------
#[derive(Clone)]
pub struct AuthState(pub Arc<RwLock<AuthMode>>);

impl AuthState {
    pub fn new(mode: AuthMode) -> Self {
        Self(Arc::new(RwLock::new(mode)))
    }

    pub async fn get(&self) -> AuthMode {
        self.0.read().await.clone()
    }

    pub async fn set(&self, mode: AuthMode) {
        *self.0.write().await = mode;
    }
}

// ---------------------------------------------------------------------------
// Persistence — ~/.orbit/auth_state.json
// ---------------------------------------------------------------------------
fn auth_state_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("auth_state.json")
}

pub fn load_auth_state(data_dir: &PathBuf) -> AuthMode {
    let path = auth_state_path(data_dir);
    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str::<AuthMode>(&contents) {
            Ok(mode) => {
                info!("Loaded auth state from {:?}", path);
                mode
            }
            Err(e) => {
                warn!("Failed to parse auth_state.json ({}), defaulting to Unset", e);
                AuthMode::Unset
            }
        },
        Err(_) => AuthMode::Unset,
    }
}

pub fn persist_auth_state(data_dir: &PathBuf, mode: &AuthMode) {
    let path = auth_state_path(data_dir);
    match serde_json::to_string_pretty(mode) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                warn!("Failed to persist auth state: {}", e);
            }
        }
        Err(e) => warn!("Failed to serialize auth state: {}", e),
    }
}
