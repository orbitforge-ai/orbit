export interface TemplateCtx {
  pluginName: string;
  pluginId: string;
  humanName: string;
}

export interface TemplateFile {
  path: string;
  contents: string;
}

export type TemplateName = 'node-tool-only' | 'node-entity' | 'node-oauth';

export const templates: Record<TemplateName, (ctx: TemplateCtx) => TemplateFile[]> = {
  'node-tool-only': (ctx) => [
    { path: 'plugin.json', contents: manifest(ctx, { tool: true }) },
    { path: 'server.js', contents: serverToolOnly(ctx) },
    { path: 'package.json', contents: packageJson(ctx) },
    { path: 'README.md', contents: readme(ctx, 'tool-only') },
  ],
  'node-entity': (ctx) => [
    { path: 'plugin.json', contents: manifest(ctx, { tool: true, entity: true }) },
    { path: 'server.js', contents: serverEntity(ctx) },
    { path: 'package.json', contents: packageJson(ctx) },
    { path: 'README.md', contents: readme(ctx, 'entity') },
  ],
  'node-oauth': (ctx) => [
    { path: 'plugin.json', contents: manifest(ctx, { tool: true, oauth: true }) },
    { path: 'server.js', contents: serverOauth(ctx) },
    { path: 'package.json', contents: packageJson(ctx) },
    { path: 'README.md', contents: readme(ctx, 'oauth') },
  ],
};

function manifest(
  ctx: TemplateCtx,
  opts: { tool?: boolean; entity?: boolean; oauth?: boolean }
) {
  const obj: Record<string, unknown> = {
    schemaVersion: 1,
    hostApiVersion: '^1.0.0',
    id: ctx.pluginId,
    name: ctx.humanName,
    version: '0.1.0',
    description: `${ctx.humanName} plugin for Orbit.`,
    runtime: { type: 'mcp-stdio', command: 'node', args: ['server.js'] },
    permissions: { network: [], oauth: [], coreEntities: [] },
  };
  if (opts.tool) {
    (obj as Record<string, unknown>).tools = [
      { name: 'greet', description: 'Say hello.', riskLevel: 'safe' },
    ];
  }
  if (opts.entity) {
    (obj as Record<string, unknown>).entityTypes = [
      {
        name: 'note',
        displayName: 'Note',
        icon: 'file-text',
        schema: {
          type: 'object',
          required: ['title'],
          properties: {
            title: { type: 'string', maxLength: 200 },
            body: { type: 'string' },
          },
        },
        relations: [],
        listFields: ['title'],
        titleField: 'title',
        indexedFields: [],
      },
    ];
  }
  if (opts.oauth) {
    (obj.permissions as Record<string, unknown>).oauth = ['example'];
    (obj as Record<string, unknown>).oauthProviders = [
      {
        id: 'example',
        name: 'Example',
        authorizationUrl: 'https://example.com/oauth/authorize',
        tokenUrl: 'https://example.com/oauth/token',
        scopes: [],
        clientType: 'public',
        redirectUri: 'orbit://oauth/callback',
      },
    ];
  }
  return JSON.stringify(obj, null, 2) + '\n';
}

function serverToolOnly(_ctx: TemplateCtx) {
  return `import { Plugin } from '@orbit/plugin-sdk';

const plugin = new Plugin({ id: '${_ctx.pluginId}' });

plugin.tool('greet', {
  description: 'Say hello.',
  inputSchema: {
    type: 'object',
    properties: { name: { type: 'string' } },
  },
  run: async ({ input }) => {
    const who = (input as { name?: string }).name ?? 'world';
    return \`hello, \${who}!\`;
  },
});

plugin.run();
`;
}

function serverEntity(_ctx: TemplateCtx) {
  return `import { Plugin } from '@orbit/plugin-sdk';

const plugin = new Plugin({ id: '${_ctx.pluginId}' });

plugin.tool('greet', {
  description: 'Say hello.',
  run: async () => 'hello',
});

plugin.tool('add_note', {
  description: 'Create a note entity.',
  inputSchema: {
    type: 'object',
    required: ['title'],
    properties: {
      title: { type: 'string' },
      body: { type: 'string' },
    },
  },
  run: async ({ input, core }) => {
    const entity = await core.entity.create('note', input);
    return { id: entity.id };
  },
});

plugin.run();
`;
}

function serverOauth(_ctx: TemplateCtx) {
  return `import { Plugin } from '@orbit/plugin-sdk';

const plugin = new Plugin({ id: '${_ctx.pluginId}' });

plugin.tool('whoami', {
  description: 'Check that the OAuth token is present.',
  run: async ({ oauth }) => {
    const token = oauth.example?.accessToken;
    if (!token) return 'not connected — connect via the Plugins screen';
    return 'connected';
  },
});

plugin.run();
`;
}

function packageJson(ctx: TemplateCtx) {
  return (
    JSON.stringify(
      {
        name: ctx.pluginName,
        version: '0.1.0',
        type: 'module',
        main: 'server.js',
        dependencies: {
          '@orbit/plugin-sdk': '^1.0.0',
        },
      },
      null,
      2
    ) + '\n'
  );
}

function readme(ctx: TemplateCtx, variant: string) {
  return `# ${ctx.humanName}

Template: \`${variant}\`.

## Develop

1. \`npm install\`
2. Enable \`developer.pluginDevMode\` in Orbit settings.
3. In Orbit: Plugins screen → Install from directory → pick this folder.
4. Enable the plugin.
5. Edit \`server.js\`, then click Reload on the plugin card.

## Tools

See \`plugin.json\` for the full list. Namespaced tool names the agent sees:

- \`${ctx.pluginId.replace(/[.-]/g, '_')}__<tool-name>\`
`;
}
