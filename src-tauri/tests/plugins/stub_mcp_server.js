#!/usr/bin/env node
// Stub MCP server used by plugin integration tests. Responds to the minimum
// protocol surface the Rust MCP client exercises.
// Exit with `STUB_MCP_EXIT_ON_INIT=1` to simulate crash-on-spawn.

if (process.env.STUB_MCP_EXIT_ON_INIT === '1') {
  process.exit(1);
}

let buffer = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => {
  buffer += chunk;
  const lines = buffer.split('\n');
  buffer = lines.pop() ?? '';
  for (const line of lines) {
    if (!line.trim()) continue;
    handle(JSON.parse(line));
  }
});

function handle(msg) {
  if (msg.method === 'initialize') {
    reply(msg.id, {
      protocolVersion: '2024-11-05',
      capabilities: { tools: { listChanged: false } },
      serverInfo: { name: 'stub', version: '0.0.0' },
    });
    send({ jsonrpc: '2.0', method: 'notifications/initialized' });
  } else if (msg.method === 'tools/list') {
    reply(msg.id, {
      tools: [
        {
          name: 'echo',
          description: 'Echo input back.',
          inputSchema: { type: 'object', properties: { text: { type: 'string' } } },
        },
      ],
    });
  } else if (msg.method === 'tools/call') {
    const params = msg.params ?? {};
    if (params.name === 'echo') {
      reply(msg.id, {
        content: [{ type: 'text', text: params.arguments?.text ?? '' }],
        isError: false,
      });
    } else {
      reply(msg.id, null, { code: -32601, message: `unknown tool: ${params.name}` });
    }
  } else if (msg.method === 'notifications/initialized') {
    // ignore
  } else {
    if (msg.id !== undefined) {
      reply(msg.id, null, { code: -32601, message: `unknown method: ${msg.method}` });
    }
  }
}

function reply(id, result, error) {
  if (error) {
    send({ jsonrpc: '2.0', id, error });
  } else {
    send({ jsonrpc: '2.0', id, result });
  }
}

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + '\n');
}
