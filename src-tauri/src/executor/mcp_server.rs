//! Embedded MCP (Model Context Protocol) server used as a bridge when Orbit
//! uses a local CLI as an LLM provider (e.g. `claude-cli`, `codex-cli`).
//!
//! Design invariants:
//!  - binds only to 127.0.0.1 — rejects non-loopback peers defensively
//!  - every request must carry a short-lived bearer token minted per run
//!  - tool execution always flows through `permissions::execute_tool_with_permissions`
//!  - `tools/list` is filtered to the run-scoped tool catalog
//!
//! Transport: a simplified subset of MCP's Streamable HTTP — a single POST
//! endpoint that accepts JSON-RPC 2.0 requests and returns JSON-RPC responses.
//! This covers what `claude -p --mcp-config` and `codex exec` need for
//! `initialize`, `tools/list`, and `tools/call`.
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

use crate::db::DbPool;
use crate::executor::agent_tools::ToolExecutionContext;
use crate::executor::llm_provider::ToolDefinition;
use crate::executor::permissions::{self, PermissionRegistry};

/// Authorization scope tied to a single agent run / chat turn.
/// A per-run token is minted in `issue_token` and revoked in `revoke_token`.
#[derive(Clone)]
#[allow(dead_code)]
pub struct McpSession {
    pub run_id: String,
    /// Kept for future per-agent routing/telemetry in the bridge handler.
    pub agent_id: String,
    pub tool_ctx: Arc<ToolExecutionContext>,
    pub tools: Vec<ToolDefinition>,
    pub permission_registry: PermissionRegistry,
    pub app: tauri::AppHandle,
    /// Kept for tools that need to open a DB connection during execution.
    pub db: DbPool,
}

/// Handle stored as Tauri managed state so call sites can issue/revoke
/// per-run MCP tokens without knowing how the server is implemented.
#[derive(Clone)]
pub struct McpServerHandle {
    inner: Arc<McpServerInner>,
}

struct McpServerInner {
    addr: SocketAddr,
    tokens: RwLock<HashMap<String, McpSession>>,
    /// Reserved so future server shutdown logic can own the bind lifecycle.
    _bind_guard: Mutex<()>,
}

impl McpServerHandle {
    #[allow(dead_code)]
    pub fn addr(&self) -> SocketAddr {
        self.inner.addr
    }

    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.inner.addr)
    }

    /// Mint a fresh token bound to this run's tool catalog + execution ctx.
    /// Call `revoke_token` when the run ends.
    pub async fn issue_token(&self, session: McpSession) -> String {
        let token = format!("orbit-{}", ulid::Ulid::new());
        let mut tokens = self.inner.tokens.write().await;
        tokens.insert(token.clone(), session);
        token
    }

    pub async fn revoke_token(&self, token: &str) {
        let mut tokens = self.inner.tokens.write().await;
        tokens.remove(token);
    }

    async fn lookup(&self, token: &str) -> Option<McpSession> {
        let tokens = self.inner.tokens.read().await;
        tokens.get(token).cloned()
    }
}

/// Start the MCP server on a random loopback port. Returns a handle the app
/// state can store. The server task runs for the lifetime of the app.
pub async fn start() -> Result<McpServerHandle, String> {
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("mcp: failed to bind loopback listener: {}", e))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("mcp: failed to read local addr: {}", e))?;

    let inner = Arc::new(McpServerInner {
        addr: local_addr,
        tokens: RwLock::new(HashMap::new()),
        _bind_guard: Mutex::new(()),
    });

    let handle = McpServerHandle {
        inner: inner.clone(),
    };
    let handle_for_router = handle.clone();

    let app = Router::new()
        .route("/mcp", post(mcp_post))
        .with_state(handle_for_router);

    tokio::spawn(async move {
        info!("MCP bridge listening on http://{}/mcp", local_addr);
        if let Err(e) = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            warn!("MCP bridge server exited: {}", e);
        }
    });

    Ok(handle)
}

/// Extract a bearer token from `Authorization: Bearer <token>`.
fn extract_token(headers: &HeaderMap) -> Option<String> {
    let header = headers.get("authorization")?.to_str().ok()?;
    let rest = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))?;
    Some(rest.trim().to_string())
}

async fn mcp_post(
    State(handle): State<McpServerHandle>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Defense in depth: reject anything that slipped in over a non-loopback peer.
    if !peer.ip().is_loopback() {
        warn!("MCP bridge rejecting non-loopback peer: {}", peer);
        return (StatusCode::FORBIDDEN, Json(json!({"error": "forbidden"}))).into_response();
    }

    let Some(token) = extract_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "missing bearer token"})),
        )
            .into_response();
    };

    let Some(session) = handle.lookup(&token).await else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid or expired token"})),
        )
            .into_response();
    };

    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = body.get("params").cloned().unwrap_or(Value::Null);

    let result: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "orbit-mcp-bridge",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "notifications/initialized" | "notifications/cancelled" => {
            // Notifications carry no id and expect no response. Axum still expects
            // a body, so return an empty success object — the CLI will ignore it.
            return Json(json!({})).into_response();
        }
        "tools/list" => Ok(list_tools(&session.tools)),
        "tools/call" => call_tool(&session, &params).await,
        other => {
            debug!("MCP method not implemented: {}", other);
            Err((-32601, format!("method not found: {}", other)))
        }
    };

    let response = match result {
        Ok(v) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": v,
        }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        }),
    };
    Json(response).into_response()
}

fn list_tools(tools: &[ToolDefinition]) -> Value {
    let items: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        })
        .collect();
    json!({ "tools": items })
}

async fn call_tool(session: &McpSession, params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (-32602, "tools/call: missing 'name'".to_string()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // The CLI may have flagged a tool as "not in catalog" client-side already,
    // but we enforce the run's allow-list here too.
    if !session.tools.iter().any(|t| t.name == name) {
        return Err((-32602, format!("tool not allowed for this run: {}", name)));
    }

    let exec_result = permissions::execute_tool_with_permissions(
        session.tool_ctx.as_ref(),
        name,
        &arguments,
        &session.app,
        &session.run_id,
        &session.permission_registry,
    )
    .await;

    match exec_result {
        Ok((output, is_error)) => Ok(json!({
            "content": [{
                "type": "text",
                "text": output
            }],
            "isError": is_error
        })),
        Err(e) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("tool error: {}", e)
            }],
            "isError": true
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_header_parsing_is_case_insensitive() {
        let mut h = HeaderMap::new();
        h.insert("authorization", "Bearer abc123".parse().unwrap());
        assert_eq!(extract_token(&h).as_deref(), Some("abc123"));

        let mut h2 = HeaderMap::new();
        h2.insert("authorization", "bearer xyz".parse().unwrap());
        assert_eq!(extract_token(&h2).as_deref(), Some("xyz"));
    }

    #[test]
    fn bearer_missing_returns_none() {
        let h = HeaderMap::new();
        assert!(extract_token(&h).is_none());
    }

    #[test]
    fn list_tools_shapes_input_schema() {
        let tools = vec![ToolDefinition {
            name: "echo".into(),
            description: "echoes input".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        }];
        let v = list_tools(&tools);
        let items = v["tools"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "echo");
        assert_eq!(items[0]["inputSchema"]["type"], "object");
    }
}
