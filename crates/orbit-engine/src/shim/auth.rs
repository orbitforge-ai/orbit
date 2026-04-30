//! Auth middleware for the HTTP+WS shim.
//!
//! Two modes:
//! - `LoopbackToken` — used for desktop dev. Connection must come from
//!   loopback *and* carry `Authorization: Bearer <token>` matching the
//!   per-process dev token. WebSockets accept the same token via a
//!   `?token=...` query string.
//! - `Jwt` — cloud/self-host deployment. Validates access tokens signed by
//!   the configured control-plane secret.
//!
//! The loopback peer check is defence-in-depth: if the bind address ever
//! leaks to a non-loopback interface we still refuse the request.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::{
    extract::ConnectInfo,
    http::{HeaderMap, StatusCode},
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

#[derive(Clone)]
pub struct JwtVerifier {
    decoding_key: DecodingKey,
    validation: Validation,
    expected_tenant_id: Option<String>,
}

impl std::fmt::Debug for JwtVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtVerifier")
            .field("validation", &self.validation)
            .field("expected_tenant_id", &self.expected_tenant_id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct JwtConfig {
    pub hs256_secret: String,
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub expected_tenant_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub tenant_id: String,
    pub email: Option<String>,
    pub exp: usize,
    pub iat: Option<usize>,
    pub plan: Option<String>,
    pub tier: Option<String>,
}

impl JwtVerifier {
    pub fn hs256(config: JwtConfig) -> Result<Self, String> {
        if config.hs256_secret.trim().is_empty() {
            return Err("JWT HS256 secret cannot be empty".to_string());
        }

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        validation.required_spec_claims.insert("sub".to_string());
        validation.required_spec_claims.insert("exp".to_string());
        if let Some(issuer) = config.issuer {
            validation.set_issuer(&[issuer]);
        }
        if let Some(audience) = config.audience {
            validation.set_audience(&[audience]);
        } else {
            validation.validate_aud = false;
        }

        Ok(Self {
            decoding_key: DecodingKey::from_secret(config.hs256_secret.as_bytes()),
            validation,
            expected_tenant_id: config.expected_tenant_id,
        })
    }

    pub fn verify(&self, token: &str) -> Result<JwtClaims, &'static str> {
        let claims = decode::<JwtClaims>(token, &self.decoding_key, &self.validation)
            .map_err(|_| "invalid bearer token")?
            .claims;

        if claims.sub.trim().is_empty() || claims.tenant_id.trim().is_empty() {
            return Err("invalid bearer token");
        }
        if let Some(expected) = &self.expected_tenant_id {
            if claims.tenant_id != *expected {
                return Err("tenant mismatch");
            }
        }
        Ok(claims)
    }
}

/// How the shim authenticates incoming requests.
#[derive(Clone, Debug)]
pub enum BindMode {
    /// Loopback + shared bearer token. `dev_token_path` is where we persist
    /// the token so the frontend's Vite plugin can read it.
    LoopbackToken {
        token: String,
        dev_token_path: PathBuf,
    },
    /// Cloud/self-host JWT auth.
    Jwt { verifier: JwtVerifier },
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
        BindMode::Jwt { verifier } => {
            let Some(presented) = extract_bearer(headers) else {
                return Err((StatusCode::UNAUTHORIZED, "missing bearer token"));
            };
            verifier
                .verify(&presented)
                .map(|_| ())
                .map_err(|message| (StatusCode::UNAUTHORIZED, message))
        }
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
        BindMode::Jwt { verifier } => {
            let presented = extract_bearer(headers).or_else(|| query_token.map(|t| t.to_string()));
            let Some(presented) = presented else {
                return Err((StatusCode::UNAUTHORIZED, "missing token"));
            };
            verifier
                .verify(&presented)
                .map(|_| ())
                .map_err(|message| (StatusCode::UNAUTHORIZED, message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::Serialize;
    use std::net::{IpAddr, Ipv4Addr};

    #[derive(Serialize)]
    struct TestClaims<'a> {
        sub: &'a str,
        tenant_id: &'a str,
        exp: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        iss: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        aud: Option<&'a str>,
    }

    fn token(secret: &str, claims: TestClaims<'_>) -> String {
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    fn jwt_mode(secret: &str) -> BindMode {
        BindMode::Jwt {
            verifier: JwtVerifier::hs256(JwtConfig {
                hs256_secret: secret.to_string(),
                issuer: Some("orbit-control".to_string()),
                audience: Some("orbit-engine".to_string()),
                expected_tenant_id: Some("tenant_a".to_string()),
            })
            .unwrap(),
        }
    }

    fn headers_with_bearer(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        headers
    }

    fn peer() -> ConnectInfo<SocketAddr> {
        ConnectInfo(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1234))
    }

    #[test]
    fn jwt_http_accepts_valid_claims() {
        let mode = jwt_mode("secret");
        let token = token(
            "secret",
            TestClaims {
                sub: "user_1",
                tenant_id: "tenant_a",
                exp: 4_102_444_800,
                iss: Some("orbit-control"),
                aud: Some("orbit-engine"),
            },
        );

        assert!(check_http(&mode, peer(), &headers_with_bearer(&token)).is_ok());
    }

    #[test]
    fn jwt_http_rejects_tenant_mismatch() {
        let mode = jwt_mode("secret");
        let token = token(
            "secret",
            TestClaims {
                sub: "user_1",
                tenant_id: "tenant_b",
                exp: 4_102_444_800,
                iss: Some("orbit-control"),
                aud: Some("orbit-engine"),
            },
        );

        let err = check_http(&mode, peer(), &headers_with_bearer(&token)).unwrap_err();
        assert_eq!(err, (StatusCode::UNAUTHORIZED, "tenant mismatch"));
    }

    #[test]
    fn jwt_ws_accepts_query_token() {
        let mode = jwt_mode("secret");
        let token = token(
            "secret",
            TestClaims {
                sub: "user_1",
                tenant_id: "tenant_a",
                exp: 4_102_444_800,
                iss: Some("orbit-control"),
                aud: Some("orbit-engine"),
            },
        );

        assert!(check_ws(&mode, peer(), &HeaderMap::new(), Some(&token)).is_ok());
    }
}
