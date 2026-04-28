//! Auth middleware for the HTTP+WS shim.
//!
//! Two modes:
//! - `LoopbackToken` — used for desktop dev. Connection must come from
//!   loopback *and* carry `Authorization: Bearer <token>` matching the
//!   per-process dev token. WebSockets accept the same token via a
//!   `?token=...` query string.
//! - `Jwt` — future cloud deployment. Validates access tokens against the
//!   existing `auth::AuthSession` machinery. Stubbed for now; Phase 5.
//!
//! The loopback peer check is defence-in-depth: if the bind address ever
//! leaks to a non-loopback interface we still refuse the request.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::{
    extract::ConnectInfo,
    http::{HeaderMap, StatusCode},
};

/// How the shim authenticates incoming requests.
#[derive(Clone, Debug)]
pub enum BindMode {
    /// Loopback + shared bearer token. `dev_token_path` is where we persist
    /// the token so the frontend's Vite plugin can read it.
    LoopbackToken {
        token: String,
        dev_token_path: PathBuf,
    },
    /// Cloud JWT auth. Not used yet.
    #[allow(dead_code)]
    Jwt,
}

impl BindMode {
    /// Reads an existing token from `dev_token_path` or generates a new one
    /// and writes it with 0600 permissions.
    pub fn loopback_with_file(dev_token_path: PathBuf) -> std::io::Result<Self> {
        let token = if dev_token_path.exists() {
            std::fs::read_to_string(&dev_token_path)?.trim().to_string()
        } else {
            // Two ULIDs = 52 chars of [0-9A-Z] — 256 bits of entropy.
            let generated = format!("{}{}", ulid::Ulid::new(), ulid::Ulid::new());
            if let Some(parent) = dev_token_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dev_token_path, &generated)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&dev_token_path)?.permissions();
                perms.set_mode(0o600);
                std::fs::set_permissions(&dev_token_path, perms)?;
            }
            generated
        };
        Ok(Self::LoopbackToken {
            token,
            dev_token_path,
        })
    }
}

/// Extract `Authorization: Bearer <tok>`.
pub fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let header = headers.get("authorization")?.to_str().ok()?;
    let rest = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))?;
    Some(rest.trim().to_string())
}

/// Authorize an incoming HTTP request. Returns `Ok(())` on success,
/// `Err((status, message))` on rejection.
pub fn check_http(
    mode: &BindMode,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, &'static str)> {
    match mode {
        BindMode::LoopbackToken { token, .. } => {
            if !peer.ip().is_loopback() {
                return Err((StatusCode::FORBIDDEN, "non-loopback peer rejected"));
            }
            let Some(presented) = extract_bearer(headers) else {
                return Err((StatusCode::UNAUTHORIZED, "missing bearer token"));
            };
            if presented != *token {
                return Err((StatusCode::UNAUTHORIZED, "invalid bearer token"));
            }
            Ok(())
        }
        BindMode::Jwt => Err((StatusCode::UNAUTHORIZED, "jwt mode not implemented")),
    }
}

/// Authorize an incoming WebSocket upgrade request. Accepts the token via
/// `Authorization: Bearer` *or* `?token=` query param (browsers can't set
/// auth headers on `new WebSocket(...)`).
pub fn check_ws(
    mode: &BindMode,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), (StatusCode, &'static str)> {
    match mode {
        BindMode::LoopbackToken { token, .. } => {
            if !peer.ip().is_loopback() {
                return Err((StatusCode::FORBIDDEN, "non-loopback peer rejected"));
            }
            let presented = extract_bearer(headers).or_else(|| query_token.map(|t| t.to_string()));
            let Some(presented) = presented else {
                return Err((StatusCode::UNAUTHORIZED, "missing token"));
            };
            if presented != *token {
                return Err((StatusCode::UNAUTHORIZED, "invalid token"));
            }
            Ok(())
        }
        BindMode::Jwt => Err((StatusCode::UNAUTHORIZED, "jwt mode not implemented")),
    }
}
