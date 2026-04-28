//! Axum router for the HTTP+WS shim.
//!
//! Routes:
//!   - `GET  /healthz`            — liveness + build info
//!   - `POST /rpc/:command`       — dispatch to the registered adapter
//!   - `GET  /ws`                 — upgrade to WebSocket, forward events
//!
//! Binding strategy: spawn on 127.0.0.1 + configurable port (default 8765)
//! and spawn the serve loop as a tokio task so it runs for the lifetime of
//! the app. Modelled on the existing MCP bridge in
//! `crate::executor::mcp_server`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::app_context::AppContext;
use crate::shim::auth::{self, BindMode};
use crate::shim::registry::Registry;
use crate::shim::ws;

/// Shared state passed to every handler.
#[derive(Clone)]
pub struct ShimState {
    pub ctx: Arc<AppContext>,
    pub registry: Arc<Registry>,
    pub mode: Arc<BindMode>,
}

pub async fn start(
    ctx: Arc<AppContext>,
    registry: Registry,
    mode: BindMode,
    port: u16,
) -> std::io::Result<SocketAddr> {
    // Initialise the event bus so `emit_*` helpers stop being no-ops.
    ws::init_bus();

    let state = ShimState {
        ctx,
        registry: Arc::new(registry),
        mode: Arc::new(mode),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/rpc/:command", post(rpc))
        .route("/ws", get(ws_upgrade))
        .route("/oauth/callback", get(oauth_callback))
        .with_state(state)
        // Static UI fallback. Catches every other GET — `/`, `/projects`, etc.
        // — and serves the embedded bundle. In dev (Vite serving on a separate
        // port) this is dormant because the browser hits Vite, not 8765. In
        // release / cloud, this is how the UI gets to the user.
        .fallback(crate::shim::static_files::handler);

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    info!("shim: listening on http://{}", bound);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            warn!("shim server exited: {}", e);
        }
    });
    Ok(bound)
}

async fn healthz() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn rpc(
    State(state): State<ShimState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(command): Path<String>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Response {
    if let Err((status, msg)) = auth::check_http(&state.mode, ConnectInfo(peer), &headers) {
        return (status, Json(json!({ "error": msg }))).into_response();
    }

    let Some(adapter) = state.registry.get(&command) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("unknown command: {}", command) })),
        )
            .into_response();
    };

    let args = body.map(|Json(v)| v).unwrap_or(Value::Null);
    match adapter(state.ctx.clone(), args).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": message })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

/// OAuth provider callback. Mirrors the loopback listener in
/// `plugins::oauth::spawn_loopback_listener` — same `handle_callback`
/// implementation, different transport. Cloud deployments will configure
/// `redirect_uri` to point here.
async fn oauth_callback(
    State(state): State<ShimState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let app = match state.ctx.app() {
        Ok(a) => a,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    // `handle_callback` only reads query params, so a synthetic origin is fine.
    let qs: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("http://shim/oauth/callback?{}", qs);
    match crate::plugins::oauth::handle_callback(app, &state.ctx.plugins, &url).await {
        Ok(()) => (
            StatusCode::OK,
            [("content-type", "text/html; charset=utf-8")],
            "<!doctype html><meta charset=utf-8><title>Connected</title>\
             <body style=\"background:#0f1117;color:#e2e8f0;font-family:-apple-system;\
             display:flex;align-items:center;justify-content:center;height:100vh\">\
             <div style=\"border:1px solid #2a2d3e;background:#13151e;border-radius:12px;\
             padding:2rem 2.5rem\">\u{2705} Connected. You can close this tab.</div></body>",
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            [("content-type", "text/plain")],
            format!("OAuth callback failed: {}", e),
        )
            .into_response(),
    }
}

async fn ws_upgrade(
    State(state): State<ShimState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<WsQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if let Err((status, msg)) =
        auth::check_ws(&state.mode, ConnectInfo(peer), &headers, q.token.as_deref())
    {
        return (status, Json(json!({ "error": msg }))).into_response();
    }
    upgrade.on_upgrade(ws::handle_socket)
}
