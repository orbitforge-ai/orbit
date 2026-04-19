# MCP server

Plugins speak MCP (Model Context Protocol) over **line-delimited JSON-RPC 2.0** on stdin/stdout. The `@orbit/plugin-sdk` handles the protocol for you; this page is the reference if you're writing a server by hand or in another language.

## Methods Orbit calls

- `initialize` — sent once on spawn. Respond with `{ protocolVersion, capabilities, serverInfo }`.
- `tools/list` — return the array of declared tools. Must match the manifest.
- `tools/call` — execute a tool. Params: `{ name, arguments }`. Response: `{ content: [{ type: "text", text }], isError }`.
- `hooks/fire` — notify of a hook event. Params: `{ event, payload }`.

A plugin can also **send** notifications (no id):

- `notifications/progress` — stream updates during a long `tools/call`.
- `notifications/initialized` — must be sent after responding to `initialize`.

## Env vars every subprocess receives

| Variable | Notes |
|---|---|
| `ORBIT_PLUGIN_ID` | The manifest id. |
| `ORBIT_PLUGIN_DATA_DIR` | Absolute path to the plugin's source directory. |
| `ORBIT_CORE_API_SOCKET` | Path to the unix socket the plugin can call back into. |
| `ORBIT_OAUTH_<PROVIDER>_ACCESS_TOKEN` | One per connected OAuth provider (uppercase id). |
| `PATH`, `HOME` | Re-injected. The user's shell rc files are **not** sourced. |

## Minimal server (no SDK)

```js
process.stdin.setEncoding('utf8');
let buffer = '';
process.stdin.on('data', chunk => {
  buffer += chunk;
  const lines = buffer.split('\n');
  buffer = lines.pop() || '';
  for (const line of lines) {
    if (!line.trim()) continue;
    const msg = JSON.parse(line);
    handle(msg);
  }
});

function handle(msg) {
  if (msg.method === 'initialize') {
    reply(msg.id, { protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'com.me.hi', version: '0.0.0' } });
    send({ jsonrpc: '2.0', method: 'notifications/initialized' });
  } else if (msg.method === 'tools/list') {
    reply(msg.id, { tools: [{ name: 'greet', description: 'hi', inputSchema: { type: 'object' } }] });
  } else if (msg.method === 'tools/call') {
    reply(msg.id, { content: [{ type: 'text', text: 'hello' }], isError: false });
  }
}

function reply(id, result) { send({ jsonrpc: '2.0', id, result }); }
function send(msg) { process.stdout.write(JSON.stringify(msg) + '\n'); }
```

## Shutdown

The subprocess is killed on: disable, reload, uninstall, Orbit quit. `kill_on_drop` is enabled, so orphaned processes should not survive a crash. Register a `SIGTERM` handler if you need to flush state.
