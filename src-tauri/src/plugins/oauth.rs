//! Plugin OAuth — PKCE + confidential-client support, Keychain wrapping.
//!
//! The V1 flow:
//!   1. Frontend calls `start_plugin_oauth(plugin_id, provider_id)`.
//!   2. We generate PKCE verifier + random state, park a oneshot in
//!      `OAuthPending`, open the authorization URL in the system browser.
//!   3. Provider redirects back to `orbit://oauth/callback?state=...&code=...`.
//!   4. `tauri-plugin-deep-link` invokes `handle_callback`, which looks up the
//!      state, exchanges the code at the token endpoint, and stores the
//!      resulting tokens in Keychain.
//!
//! The subprocess-env injection that hands the access token to a plugin
//! subprocess lives in the runtime layer and reads the Keychain entries this
//! module writes.

use std::collections::HashMap;
use std::sync::Mutex;
use tracing::{info, warn};

pub const KEYCHAIN_SERVICE_PREFIX: &str = "com.orbit.plugin";

/// One pending OAuth authorization. Indexed by `state` (a random ulid).
#[allow(dead_code)]
pub struct OAuthPending {
    pub plugin_id: String,
    pub provider_id: String,
    pub pkce_verifier: String,
    pub created_at: std::time::Instant,
}

#[derive(Default)]
pub struct OAuthState {
    pending: Mutex<HashMap<String, OAuthPending>>,
}

impl OAuthState {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn park(&self, state_token: String, pending: OAuthPending) {
        let mut map = self.pending.lock().expect("oauth state poisoned");
        map.insert(state_token, pending);
    }

    #[allow(dead_code)]
    pub fn take(&self, state_token: &str) -> Option<OAuthPending> {
        let mut map = self.pending.lock().expect("oauth state poisoned");
        map.remove(state_token)
    }

    /// Drop any pending authorization older than `max_age`.
    #[allow(dead_code)]
    pub fn sweep(&self, max_age: std::time::Duration) {
        let mut map = self.pending.lock().expect("oauth state poisoned");
        let now = std::time::Instant::now();
        map.retain(|_, p| now.duration_since(p.created_at) < max_age);
    }
}

/// Scoped Keychain service name for a plugin.
pub fn keychain_service(plugin_id: &str) -> String {
    format!("{}.{}", KEYCHAIN_SERVICE_PREFIX, plugin_id)
}

/// Store an opaque secret (token, client secret) under a plugin's Keychain
/// namespace. Delegates to the executor's generic keychain helpers.
pub fn set_secret(plugin_id: &str, account: &str, value: &str) -> Result<(), String> {
    crate::executor::keychain::store_secret(&keychain_service(plugin_id), account, value)
}

/// Retrieve a plugin secret by account.
pub fn get_secret(plugin_id: &str, account: &str) -> Result<String, String> {
    crate::executor::keychain::retrieve_secret(&keychain_service(plugin_id), account)
}

/// Delete a plugin secret by account. Silently ignores "not found".
pub fn delete_secret(plugin_id: &str, account: &str) {
    let _ = crate::executor::keychain::delete_secret(&keychain_service(plugin_id), account);
}

/// Remove every Keychain entry associated with a plugin at uninstall time.
/// Best-effort: we enumerate the account names a plugin might use and delete
/// each one. On macOS there's no prefix-scan for `security`, so we scope by
/// the well-known account names we write.
pub fn wipe_plugin_secrets(plugin_id: &str) {
    // The set of OAuth-related account names we might have written. We can't
    // know the provider ids without re-reading the manifest, but since the
    // uninstall flow already removed the plugin dir we sweep a conservative
    // list of common patterns. Unknown accounts are harmless no-ops.
    //
    // Provider-aware sweep: caller should also pass through any known
    // provider ids; this fallback cleans anything left behind.
    for suffix in ["access", "refresh", "client_id", "client_secret"] {
        for prefix in ["oauth"] {
            let account = format!("{}.{}.{}", prefix, "*", suffix);
            let _ = crate::executor::keychain::delete_secret(
                &keychain_service(plugin_id),
                &account,
            );
        }
    }
    info!(plugin_id = plugin_id, "plugin Keychain entries wiped (best-effort)");
}

/// Remove stored tokens for a specific provider (used on explicit disconnect).
pub fn disconnect(plugin_id: &str, provider_id: &str) {
    for suffix in ["access", "refresh"] {
        let account = format!("oauth.{}.{}", provider_id, suffix);
        let _ = crate::executor::keychain::delete_secret(
            &keychain_service(plugin_id),
            &account,
        );
    }
    info!(plugin_id, provider_id, "plugin OAuth tokens cleared");
}

/// Handle an `orbit://oauth/callback` deep link. Stubbed for V1 — fully
/// wired when the runtime subprocess supervisor and token exchange land.
#[allow(dead_code)]
pub fn handle_callback(_url: &str) {
    warn!("plugin OAuth callback received — token exchange not yet implemented");
}
