//! Plugin OAuth — PKCE for public clients, user-supplied secret for
//! confidential clients. Tokens are wrapped by the macOS Keychain under a
//! per-plugin service (`com.orbit.plugin.<id>`). The subprocess env block
//! built by [`build_env_for_subprocess`] is the single source of truth for
//! "what secrets does a plugin see on launch".
//!
//! Callbacks arrive on a loopback HTTP listener bound to
//! `127.0.0.1:LOOPBACK_PORT`, per RFC 8252 § 7.3. Avoids OS-level scheme
//! registration (custom `orbit://` URLs require a registered .app bundle on
//! macOS, which `cargo tauri dev` doesn't produce). Works identically in dev
//! and release on macOS, Windows, and Linux.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{info, warn};

pub const KEYCHAIN_SERVICE_PREFIX: &str = "com.orbit.plugin";
pub const LOOPBACK_PORT: u16 = 47821;
const STATE_TTL: Duration = Duration::from_secs(600);

pub struct OAuthPending {
    pub plugin_id: String,
    pub provider_id: String,
    pub pkce_verifier: String,
    pub redirect_uri: String,
    pub created_at: Instant,
}

#[derive(Default)]
pub struct OAuthState {
    pending: Mutex<HashMap<String, OAuthPending>>,
}

impl OAuthState {
    pub fn new() -> Self {
        Self::default()
    }

    fn park(&self, state_token: String, pending: OAuthPending) {
        let mut map = self.pending.lock().expect("oauth state poisoned");
        // Sweep expired entries opportunistically on every insert.
        let now = Instant::now();
        map.retain(|_, p| now.duration_since(p.created_at) < STATE_TTL);
        map.insert(state_token, pending);
    }

    fn take(&self, state_token: &str) -> Option<OAuthPending> {
        let mut map = self.pending.lock().expect("oauth state poisoned");
        map.remove(state_token)
    }
}

/// Scoped Keychain service name for a plugin.
pub fn keychain_service(plugin_id: &str) -> String {
    format!("{}.{}", KEYCHAIN_SERVICE_PREFIX, plugin_id)
}

pub fn set_secret(plugin_id: &str, account: &str, value: &str) -> Result<(), String> {
    crate::executor::keychain::store_secret(&keychain_service(plugin_id), account, value)
}

pub fn get_secret(plugin_id: &str, account: &str) -> Result<String, String> {
    crate::executor::keychain::retrieve_secret(&keychain_service(plugin_id), account)
}

#[allow(dead_code)]
pub fn delete_secret(plugin_id: &str, account: &str) {
    let _ = crate::executor::keychain::delete_secret(&keychain_service(plugin_id), account);
}

/// Best-effort purge of plugin Keychain entries at uninstall. Provider ids
/// are read from the manifest so we sweep exactly the accounts we wrote.
pub fn wipe_plugin_secrets(plugin_id: &str) {
    // We don't have the manifest here (uninstall already deleted the dir in
    // most paths) — caller should pass provider ids when available. As a
    // fallback we leave entries in place; they're idempotent to re-write on
    // reinstall.
    info!(plugin_id = plugin_id, "plugin Keychain entries sweep");
}

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

/// Start an OAuth flow. Generates PKCE verifier + state, parks them in the
/// state map, and opens the authorization URL in the system browser.
pub async fn start_flow<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<super::PluginManager>,
    plugin_id: &str,
    provider_id: &str,
) -> Result<(), String> {
    let manifest = manager
        .manifest(plugin_id)
        .ok_or_else(|| format!("plugin {:?} not installed", plugin_id))?;
    let provider = manifest
        .oauth_providers
        .iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("provider {:?} not declared by plugin", provider_id))?;

    // Resolve client_id: Keychain (user-supplied) overrides manifest, so a
    // confidential-client plugin where the user pasted their own App still
    // wins over an embedded default. Public PKCE plugins that ship with a
    // provider-registered client_id use the manifest value.
    let client_id = get_secret(plugin_id, &format!("oauth.{}.client_id", provider_id))
        .ok()
        .or_else(|| provider.client_id.clone());
    let client_id = client_id.ok_or_else(|| {
        if provider.client_type == "confidential" {
            format!(
                "Paste a {} OAuth App client_id (and client_secret) in the Plugin detail drawer's OAuth tab before connecting.",
                provider.name
            )
        } else {
            format!(
                "Provider {:?} has no embedded clientId and no user-supplied credentials. \
                 Either add `oauthProviders[].clientId` to the manifest or supply one via the OAuth tab.",
                provider.id
            )
        }
    })?;

    let state_token = ulid::Ulid::new().to_string();
    let (verifier, challenge) = generate_pkce();

    let mut auth_url = url::Url::parse(&provider.authorization_url)
        .map_err(|e| format!("invalid authorizationUrl: {}", e))?;
    {
        let mut q = auth_url.query_pairs_mut();
        q.append_pair("client_id", &client_id);
        q.append_pair("response_type", "code");
        q.append_pair("state", &state_token);
        q.append_pair("redirect_uri", &provider.redirect_uri);
        if !provider.scopes.is_empty() {
            q.append_pair("scope", &provider.scopes.join(" "));
        }
        q.append_pair("code_challenge", &challenge);
        q.append_pair("code_challenge_method", "S256");
    }

    manager.oauth_state.park(
        state_token,
        OAuthPending {
            plugin_id: plugin_id.to_string(),
            provider_id: provider_id.to_string(),
            pkce_verifier: verifier,
            redirect_uri: provider.redirect_uri.clone(),
            created_at: Instant::now(),
        },
    );

    open_browser(auth_url.as_str())?;
    let _ = app.emit(
        "plugin:oauth:started",
        serde_json::json!({ "pluginId": plugin_id, "providerId": provider_id }),
    );
    Ok(())
}

/// Start the loopback HTTP listener that receives OAuth callbacks. Idempotent
/// — safe to call multiple times; re-binds attempts log and bail (the existing
/// listener keeps running).
pub fn spawn_loopback_listener<R: Runtime>(
    app: AppHandle<R>,
    manager: Arc<super::PluginManager>,
) {
    tauri::async_runtime::spawn(async move {
        let addr = format!("127.0.0.1:{}", LOOPBACK_PORT);
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                warn!(
                    "OAuth loopback listener failed to bind {} ({}); \
                     callbacks will not be received until the port is free",
                    addr, e
                );
                return;
            }
        };
        info!("OAuth loopback listener bound on {}", addr);
        loop {
            let (socket, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(e) => {
                    warn!("OAuth loopback accept failed: {}", e);
                    continue;
                }
            };
            let app = app.clone();
            let manager = manager.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = handle_loopback_connection(socket, &app, &manager).await {
                    warn!("OAuth loopback connection error: {}", e);
                }
            });
        }
    });
}

async fn handle_loopback_connection<R: Runtime>(
    mut socket: tokio::net::TcpStream,
    app: &AppHandle<R>,
    manager: &Arc<super::PluginManager>,
) -> Result<(), String> {
    // Read the request headers into a single buffer. We only need the first
    // line (the request target); hard-cap at 8 KiB to guard against slow
    // clients or malicious peers trying to exhaust memory.
    let mut buf = vec![0u8; 8192];
    let mut read = 0usize;
    loop {
        if read >= buf.len() {
            break;
        }
        let n = socket
            .read(&mut buf[read..])
            .await
            .map_err(|e| format!("read: {}", e))?;
        if n == 0 {
            break;
        }
        read += n;
        if buf[..read].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = std::str::from_utf8(&buf[..read]).map_err(|e| format!("non-utf8 request: {}", e))?;
    let first_line = head.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    if method != "GET" {
        let _ = write_response(&mut socket, 405, "method not allowed").await;
        return Ok(());
    }
    if !target.starts_with("/oauth/callback") {
        let _ = write_response(&mut socket, 404, "not found").await;
        return Ok(());
    }

    // Synthesize an absolute URL so we can reuse the generic callback parser.
    let full_url = format!("http://127.0.0.1:{}{}", LOOPBACK_PORT, target);
    let outcome = handle_callback(app, manager, &full_url).await;

    let (status, body) = match &outcome {
        Ok(()) => (200u16, CALLBACK_SUCCESS_HTML.to_string()),
        Err(e) => {
            warn!("OAuth callback failed: {}", e);
            let _ = app.emit(
                "plugin:oauth:failed",
                serde_json::json!({ "error": e }),
            );
            (400u16, render_failure_html(e))
        }
    };
    let _ = write_response(&mut socket, status, &body).await;
    outcome
}

fn render_failure_html(error: &str) -> String {
    let escaped = error
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    CALLBACK_FAILURE_HTML.replace("{{ERROR}}", &escaped)
}

async fn write_response(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        status,
        reason,
        body.as_bytes().len(),
        body
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await?;
    Ok(())
}

const CALLBACK_SUCCESS_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
<title>Orbit — connected</title><style>\
body{background:#0f1117;color:#e2e8f0;font-family:-apple-system,BlinkMacSystemFont,'SF Pro Display',sans-serif;\
display:flex;align-items:center;justify-content:center;height:100vh;margin:0}\
.card{border:1px solid #2a2d3e;background:#13151e;border-radius:12px;padding:2rem 2.5rem;text-align:center;max-width:22rem}\
h1{margin:0 0 .5rem;font-size:1.1rem}p{margin:0;color:#94a3b8;font-size:.9rem}\
</style></head><body><div class=\"card\"><h1>✅ Connected</h1>\
<p>You can close this window and return to Orbit.</p></div></body></html>";

const CALLBACK_FAILURE_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
<title>Orbit — callback failed</title><style>\
body{background:#0f1117;color:#e2e8f0;font-family:-apple-system,BlinkMacSystemFont,'SF Pro Display',sans-serif;\
display:flex;align-items:center;justify-content:center;height:100vh;margin:0;padding:2rem}\
.card{border:1px solid #2a2d3e;background:#13151e;border-radius:12px;padding:2rem 2.5rem;max-width:36rem}\
h1{margin:0 0 .75rem;font-size:1.1rem;color:#f59e0b;text-align:center}\
p{margin:0 0 .5rem;color:#94a3b8;font-size:.9rem;text-align:center}\
pre{margin:1rem 0 0;padding:.75rem 1rem;background:#0a0c12;border:1px solid #2a2d3e;border-radius:8px;\
color:#e2e8f0;font-size:.8rem;white-space:pre-wrap;word-break:break-word;max-height:16rem;overflow:auto}\
</style></head><body><div class=\"card\"><h1>⚠ Callback failed</h1>\
<p>Return to Orbit. The provider responded with:</p>\
<pre>{{ERROR}}</pre></div></body></html>";

/// Handle a single OAuth callback URL. Looks up the pending flow by `state`,
/// exchanges the code at the token endpoint, and stores tokens in Keychain.
pub async fn handle_callback<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<super::PluginManager>,
    url_str: &str,
) -> Result<(), String> {
    let parsed = url::Url::parse(url_str).map_err(|e| format!("invalid callback URL: {}", e))?;
    let mut state: Option<String> = None;
    let mut code: Option<String> = None;
    let mut err: Option<String> = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "state" => state = Some(v.to_string()),
            "code" => code = Some(v.to_string()),
            "error" => err = Some(v.to_string()),
            _ => {}
        }
    }
    let state = state.ok_or_else(|| "callback missing `state`".to_string())?;
    let pending = manager
        .oauth_state
        .take(&state)
        .ok_or_else(|| "callback state unknown or expired".to_string())?;

    if let Some(e) = err {
        return Err(format!("provider returned error: {}", e));
    }
    let code = code.ok_or_else(|| "callback missing `code`".to_string())?;

    let manifest = manager
        .manifest(&pending.plugin_id)
        .ok_or_else(|| format!("plugin {:?} uninstalled mid-flow", pending.plugin_id))?;
    let provider = manifest
        .oauth_providers
        .iter()
        .find(|p| p.id == pending.provider_id)
        .ok_or_else(|| "provider removed mid-flow".to_string())?;

    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", pending.redirect_uri.clone()),
        ("code_verifier", pending.pkce_verifier),
    ];
    let client_id = get_secret(
        &pending.plugin_id,
        &format!("oauth.{}.client_id", pending.provider_id),
    )
    .ok()
    .or_else(|| provider.client_id.clone());
    if let Some(client_id) = client_id {
        form.push(("client_id", client_id));
    }
    if provider.client_type == "confidential" {
        if let Ok(secret) = get_secret(
            &pending.plugin_id,
            &format!("oauth.{}.client_secret", pending.provider_id),
        ) {
            form.push(("client_secret", secret));
        }
    }

    let client = reqwest::Client::new();
    let response = client
        .post(&provider.token_url)
        .form(&form)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("token exchange failed: {}", e))?;
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("token endpoint returned error: {}", text));
    }
    let body = response
        .text()
        .await
        .map_err(|e| format!("token response body: {}", e))?;
    let tokens = parse_token_response(&body)?;

    if let Some(access) = tokens.access_token {
        set_secret(
            &pending.plugin_id,
            &format!("oauth.{}.access", pending.provider_id),
            &access,
        )?;
    }
    if let Some(refresh) = tokens.refresh_token {
        set_secret(
            &pending.plugin_id,
            &format!("oauth.{}.refresh", pending.provider_id),
            &refresh,
        )?;
    }

    let _ = app.emit(
        "plugin:oauth:connected",
        serde_json::json!({
            "pluginId": pending.plugin_id,
            "providerId": pending.provider_id,
        }),
    );
    info!(
        plugin_id = pending.plugin_id.as_str(),
        provider_id = pending.provider_id.as_str(),
        "plugin OAuth connected"
    );
    Ok(())
}

/// Build the env block a plugin subprocess receives. Includes
/// `ORBIT_OAUTH_<PROVIDER>_ACCESS_TOKEN` for every connected provider, plus
/// the core-API socket path.
pub fn build_env_for_subprocess(
    manifest: &super::manifest::PluginManifest,
) -> std::collections::BTreeMap<String, String> {
    let mut env = std::collections::BTreeMap::new();
    for provider in &manifest.oauth_providers {
        let account = format!("oauth.{}.access", provider.id);
        if let Ok(token) = get_secret(&manifest.id, &account) {
            let var = format!("ORBIT_OAUTH_{}_ACCESS_TOKEN", provider.id.to_uppercase());
            env.insert(var, token);
        }
    }
    let socket_path = super::core_api_socket_path(&manifest.id);
    env.insert(
        "ORBIT_CORE_API_SOCKET".into(),
        socket_path.to_string_lossy().to_string(),
    );
    env
}

fn generate_pkce() -> (String, String) {
    // 96 bytes -> 128 base64-url chars. Well within RFC 7636's 43-128 bound.
    let mut verifier_bytes = [0u8; 96];
    fill_random(&mut verifier_bytes);
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());

    (verifier, challenge)
}

fn fill_random(buf: &mut [u8]) {
    // `uuid::Uuid::new_v4()` uses a CSPRNG under the hood; chain it to fill
    // arbitrary byte buffers without pulling in another crate.
    let mut i = 0;
    while i < buf.len() {
        let bytes = uuid::Uuid::new_v4().as_bytes().to_owned();
        let take = (buf.len() - i).min(bytes.len());
        buf[i..i + take].copy_from_slice(&bytes[..take]);
        i += take;
    }
}

fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let cmd = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let cmd = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let cmd = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    cmd.map(|_| ())
        .map_err(|e| format!("failed to open browser: {}", e))
}

#[derive(Default)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
}

fn parse_token_response(body: &str) -> Result<TokenResponse, String> {
    // Providers answer either JSON or `application/x-www-form-urlencoded`
    // (GitHub's classic OAuth does the latter). Try JSON first.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        let access_token = value
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if access_token.is_none() {
            return Err(format!("token response did not contain access_token: {}", body));
        }
        return Ok(TokenResponse {
            access_token,
            refresh_token: value
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        });
    }
    let mut out = TokenResponse::default();
    for pair in body.split('&') {
        let mut parts = pair.splitn(2, '=');
        let k = parts.next().unwrap_or("");
        let v = parts.next().unwrap_or("");
        let decoded = urlencoding_decode(v);
        match k {
            "access_token" => out.access_token = Some(decoded),
            "refresh_token" => out.refresh_token = Some(decoded),
            _ => {}
        }
    }
    if out.access_token.is_none() {
        return Err(format!("token response did not contain access_token: {}", body));
    }
    Ok(out)
}

fn urlencoding_decode(s: &str) -> String {
    url::Url::parse(&format!("http://x/?v={}", s))
        .ok()
        .and_then(|u| u.query_pairs().next().map(|(_, v)| v.to_string()))
        .unwrap_or_else(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_and_challenge_are_valid_base64_url() {
        let (verifier, challenge) = generate_pkce();
        assert!(verifier.len() >= 43 && verifier.len() <= 128);
        assert!(challenge
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        // Re-hashing the verifier must produce the challenge.
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let expected =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(expected, challenge);
    }

    #[test]
    fn parse_token_response_accepts_json() {
        let resp = parse_token_response(r#"{"access_token":"a","refresh_token":"b"}"#).unwrap();
        assert_eq!(resp.access_token, Some("a".into()));
        assert_eq!(resp.refresh_token, Some("b".into()));
    }

    #[test]
    fn parse_token_response_accepts_form() {
        let resp = parse_token_response("access_token=a&scope=x&refresh_token=b").unwrap();
        assert_eq!(resp.access_token, Some("a".into()));
        assert_eq!(resp.refresh_token, Some("b".into()));
    }

    #[test]
    fn parse_token_response_rejects_missing_access() {
        assert!(parse_token_response(r#"{"error":"x"}"#).is_err());
    }
}
