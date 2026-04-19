# Quickstart — build a hello-world plugin

Goal: install a Node plugin that exposes a `greet` tool an agent can call. End-to-end in < 10 minutes.

## 1. Scaffold

```bash
npx create-orbit-plugin hello --template=node-tool-only
cd hello
npm install
```

This writes:

```
hello/
├── plugin.json       # manifest declaring the `greet` tool
├── server.js         # MCP stdio server using @orbit/plugin-sdk
├── package.json
└── README.md
```

## 2. Enable dev mode

Open Orbit → Settings → Developer and flip `Plugin dev mode` on. (Or edit `~/.orbit/settings.json` and set `developer.pluginDevMode = true`.)

## 3. Install from directory

In Orbit: **Plugins** screen → **Install from directory** → pick the `hello/` folder. The plugin card appears, disabled by default.

## 4. Enable and call

Toggle **Enabled** on the card. In an agent chat, ask the agent to call the tool:

> Use the `com_example_hello__greet` tool with name "world".

Orbit spawns the subprocess lazily, the agent gets back `hello, world`. The status dot on the plugin card flips green.

## 5. Iterate

Edit `server.js`:

```js
plugin.tool('greet', {
  description: 'Say hello (excited edition).',
  run: async ({ input }) => {
    const who = (input).name ?? 'world';
    return `hello, ${who}!!!`;
  },
});
```

Click **Reload** on the plugin card. Next tool call respawns with the new code.

## Next steps

- Add a JSON Schema to your tool so agents validate arguments automatically — see [`manifest-reference.md`](manifest-reference.md).
- Add an entity type so your plugin can store structured data — see [`entity-types.md`](entity-types.md).
- Add an OAuth provider so your plugin can call third-party APIs — see [`oauth.md`](oauth.md).
- Package for distribution — see [`packaging.md`](packaging.md).
