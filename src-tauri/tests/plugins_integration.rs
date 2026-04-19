//! Integration tests for the plugin subsystem. Exercises the MCP stdio client
//! against a stub MCP server implemented in Node.
//!
//! Requires `node` on PATH. Skipped gracefully when missing so CI on
//! node-less runners still passes.

use std::path::PathBuf;
use std::sync::Arc;

use orbit_lib::plugins::mcp_client::{LaunchSpec, McpClient};

fn node_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn stub_server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("plugins")
        .join("stub_mcp_server.js")
}

#[tokio::test]
async fn stub_plugin_round_trips_echo_tool() {
    if !node_available() {
        eprintln!("node not installed — skipping stub plugin test");
        return;
    }

    let server = stub_server_path();
    assert!(server.is_file(), "stub server missing: {}", server.display());

    let sink: Arc<dyn Fn(&str, String) + Send + Sync> =
        Arc::new(|_id: &str, _line: String| {});
    let mut env = std::collections::BTreeMap::new();
    if let Ok(path) = std::env::var("PATH") {
        env.insert("PATH".into(), path);
    }
    let spec = LaunchSpec {
        plugin_id: "com.orbit.test".into(),
        command: "node".into(),
        args: vec![server.to_string_lossy().into_owned()],
        working_dir: std::env::temp_dir(),
        env,
    };
    let client = McpClient::spawn(spec, sink)
        .await
        .expect("client must spawn");

    let tools = client.list_tools().await.expect("tools/list must succeed");
    assert!(
        tools
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().any(|t| t.get("name").and_then(|n| n.as_str()) == Some("echo")))
            .unwrap_or(false),
        "stub must advertise echo tool"
    );

    let result = client
        .call_tool("echo", &serde_json::json!({ "text": "ping" }))
        .await
        .expect("tools/call must succeed");
    let content = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    assert_eq!(content, "ping");

    client.shutdown().await;
}

#[tokio::test]
async fn crashing_subprocess_fails_initialize_cleanly() {
    if !node_available() {
        return;
    }
    let server = stub_server_path();
    let sink: Arc<dyn Fn(&str, String) + Send + Sync> =
        Arc::new(|_: &str, _: String| {});
    let mut env = std::collections::BTreeMap::new();
    env.insert("STUB_MCP_EXIT_ON_INIT".into(), "1".into());
    // Put PATH in so `node` resolves.
    if let Ok(path) = std::env::var("PATH") {
        env.insert("PATH".into(), path);
    }
    let spec = LaunchSpec {
        plugin_id: "com.orbit.test".into(),
        command: "node".into(),
        args: vec![server.to_string_lossy().into_owned()],
        working_dir: std::env::temp_dir(),
        env,
    };
    let result = McpClient::spawn(spec, sink).await;
    assert!(result.is_err(), "crashing subprocess should fail initialize");
}
