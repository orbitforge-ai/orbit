# @orbit/plugin-sdk

SDK for building Orbit plugins in Node.js.

## Install

```bash
npm install @orbit/plugin-sdk
```

Or scaffold a new plugin with:

```bash
npx create-orbit-plugin my-plugin --template=node-tool-only
```

## Minimal example

```ts
import { Plugin } from '@orbit/plugin-sdk';

const plugin = new Plugin({ id: 'com.example.hello' });

plugin.tool('greet', {
  description: 'Say hello.',
  inputSchema: { type: 'object', properties: { name: { type: 'string' } } },
  run: async ({ input, log }) => {
    log(`greeting ${JSON.stringify(input)}`);
    return `hello, ${(input as any).name ?? 'world'}`;
  },
});

plugin.run();
```

## Authoring guide

See `docs/plugins/` in the Orbit repo:

- `quickstart.md` — build and install a hello-world plugin in 10 minutes
- `manifest-reference.md` — every `plugin.json` field
- `mcp-server.md` — protocol details
- `entity-types.md` — declaring custom entity types
- `core-api.md` — methods plugins can call over `ORBIT_CORE_API_SOCKET`
- `oauth.md` — OAuth flow details
- `packaging.md` — zip packaging
- `security.md` — what Orbit guarantees vs what plugins are trusted with
