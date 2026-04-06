# Plan: MCP Integration — Model Context Protocol Tools

> **Source**: Claude Code's `MCPTool`, `ListMcpResources`, `ReadMcpResource`, `McpAuthTool` tools. Not present in OpenClaw.
> **Approach**: New capability — MCP is the standard protocol for extending AI agents with external tools and data sources.

## Context

Claude Code has deep MCP integration: connect to MCP servers (stdio, SSE, HTTP, WebSocket), discover their tools and resources, call tools, read resources, and handle OAuth auth. MCP is becoming the standard way to extend AI agent capabilities. For Orbit, MCP support means agents can connect to databases, APIs, IDEs, and any MCP-compatible service.

## What It Does

A unified `mcp` tool with actions: `list_servers`, `list_tools`, `call_tool`, `list_resources`, `read_resource`. Plus configuration for adding/removing MCP server connections.

## Backend Changes

### `Cargo.toml`

```toml
mcp-client = "0.1"  # or implement raw JSON-RPC over stdio/HTTP
serde_json = "1"     # already present
```

### New module: `src-tauri/src/executor/mcp.rs`

MCP client implementation:

```rust
pub struct McpManager {
    servers: HashMap<String, McpServer>,
}

pub struct McpServer {
    pub name: String,
    pub transport: McpTransport,
    pub status: ServerStatus,  // Connected, Failed, Pending
    pub tools: Vec<McpToolDef>,
    pub resources: Vec<McpResource>,
}

pub enum McpTransport {
    Stdio { command: String, args: Vec<String> },
    Http { url: String },
    Sse { url: String },
}

pub enum ServerStatus {
    Connected,
    Failed(String),
    Pending,
}

impl McpManager {
    pub async fn connect(&mut self, name: &str, config: &McpServerConfig) -> Result<(), String>;
    pub async fn disconnect(&mut self, name: &str) -> Result<(), String>;
    pub async fn list_tools(&self, server: Option<&str>) -> Vec<McpToolDef>;
    pub async fn call_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value, String>;
    pub async fn list_resources(&self, server: Option<&str>) -> Vec<McpResource>;
    pub async fn read_resource(&self, server: &str, uri: &str) -> Result<String, String>;
}
```

### New file: `src-tauri/src/executor/tools/mcp.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod mcp;` and `Box::new(mcp::McpTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):

```rust
ToolDefinition {
    name: "mcp".to_string(),
    description: "Interact with MCP (Model Context Protocol) servers. List available tools and resources from connected servers, call tools, and read resources.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["list_servers", "list_tools", "call_tool", "list_resources", "read_resource"],
                "description": "Action to perform"
            },
            "server": {
                "type": "string",
                "description": "Server name (required for call_tool, read_resource; optional filter for list_*)"
            },
            "tool": {
                "type": "string",
                "description": "Tool name to call (for call_tool)"
            },
            "arguments": {
                "type": "object",
                "description": "Arguments to pass to the MCP tool (for call_tool)"
            },
            "uri": {
                "type": "string",
                "description": "Resource URI to read (for read_resource)"
            }
        },
        "required": ["action"]
    }),
}
```

### MCP Server Configuration

Store in agent workspace config or a dedicated `mcp.json`:

```json
{
    "mcpServers": {
        "filesystem": {
            "transport": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
        },
        "github": {
            "transport": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-github"],
            "env": { "GITHUB_TOKEN": "..." }
        }
    }
}
```

### `src-tauri/src/executor/permissions.rs`

```rust
"mcp" => {
    let action = input["action"].as_str().unwrap_or("list_servers");
    match action {
        "list_servers" | "list_tools" | "list_resources" => (RiskLevel::AutoAllow, String::new()),
        "read_resource" => (RiskLevel::AutoAllow, String::new()),
        "call_tool" => {
            let server = input["server"].as_str().unwrap_or("<unknown>");
            let tool = input["tool"].as_str().unwrap_or("<unknown>");
            (RiskLevel::Prompt, format!("MCP call: {}/{}", server, tool))
        }
        _ => (RiskLevel::Prompt, "MCP action".to_string()),
    }
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`

```ts
import { Plug } from 'lucide-react';
mcp: { Icon: Plug, colorClass: 'text-purple-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`

Add new "Integrations" category:

```ts
{
    label: 'Integrations',
    tools: [{ id: 'mcp', label: 'MCP Servers' }],
},
```

### New UI: MCP Server Configuration

Add an "MCP Servers" section in global settings:
- List connected servers with status indicators
- Add/remove server configurations
- Test connectivity
- View available tools and resources per server

Tool-call result presentation for `mcp` should build on the shared tool presentation foundation from plan [26-tool-use-ui](26-tool-use-ui.md), so list/read/call actions all use the same human-readable tool-detail system instead of bespoke raw JSON panels.

## Permission Level

- `list_*`, `read_resource`: **AutoAllow** (discovery/read-only)
- `call_tool`: **Prompt** (executes arbitrary external tool)

## Dependencies

- MCP client implementation (JSON-RPC 2.0 over stdio or HTTP)
- `tokio::process` for stdio transport (already available)
- `reqwest` for HTTP/SSE transport (already available)

## Key Design Decisions

- **Single `mcp` tool** with actions (vs. Claude Code's 4 separate tools)
- **Server lifecycle**: Auto-connect on first use, keep alive for session
- **Security**: Tool calls always require permission prompt (unknown external side effects)
- **Config storage**: Per-agent MCP config so different agents can have different integrations
- **Dynamic tool discovery**: MCP server tools become available to the agent after connecting

## Verification

1. Configure an MCP server (e.g., filesystem server) in agent config
2. `mcp { action: "list_servers" }` -> shows connected server
3. `mcp { action: "list_tools", server: "filesystem" }` -> shows available tools
4. `mcp { action: "call_tool", server: "filesystem", tool: "read_file", arguments: {"path": "/tmp/test"} }` -> calls tool
5. `mcp { action: "list_resources" }` -> shows available resources
6. Test with disconnected server -> clear error
7. Confirm permission prompt for call_tool
