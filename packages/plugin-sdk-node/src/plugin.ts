import { createCoreApi, type CoreApi } from './core.js';
import type { HookHandler, ToolContext, ToolHandler } from './types.js';

interface JsonRpcMessage {
  jsonrpc: '2.0';
  id?: number | string | null;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

export interface PluginOptions {
  /** Must match manifest.id. */
  id: string;
}

/**
 * Orchestrates the MCP stdio server for a single plugin. Authors register
 * tools and hook handlers, then call `plugin.run()`.
 */
export class Plugin {
  private tools: Map<string, ToolHandler> = new Map();
  private hooks: Map<string, HookHandler[]> = new Map();
  private core: CoreApi | null = null;
  private oauth: Record<string, { accessToken: string | undefined }> = {};

  constructor(private readonly options: PluginOptions) {
    // Lazy-build the core API + oauth map on first use so test code can
    // construct a Plugin without the env vars being set.
  }

  tool(name: string, handler: ToolHandler): void {
    if (name.includes('__')) {
      throw new Error(
        `tool name ${name} must not contain '__' (reserved for namespacing)`
      );
    }
    this.tools.set(name, handler);
  }

  on(event: string, handler: HookHandler): void {
    const arr = this.hooks.get(event) ?? [];
    arr.push(handler);
    this.hooks.set(event, arr);
  }

  /** Kick off the MCP stdio loop. Resolves when stdin closes. */
  async run(): Promise<void> {
    const stdin = process.stdin;
    const stdout = process.stdout;
    const write = (msg: unknown) => {
      stdout.write(JSON.stringify(msg) + '\n');
    };

    stdin.setEncoding('utf8');
    let buffer = '';
    stdin.on('data', (chunk) => {
      buffer += chunk;
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';
      for (const line of lines) {
        if (!line.trim()) continue;
        let msg: JsonRpcMessage;
        try {
          msg = JSON.parse(line) as JsonRpcMessage;
        } catch {
          continue;
        }
        void this.handle(msg, write);
      }
    });

    await new Promise<void>((resolve) => {
      stdin.on('end', () => resolve());
    });
  }

  private async handle(
    msg: JsonRpcMessage,
    write: (m: unknown) => void
  ): Promise<void> {
    if (!msg.method) return;
    const respond = (result: unknown) => {
      if (msg.id === undefined) return;
      write({ jsonrpc: '2.0', id: msg.id, result });
    };
    const respondError = (code: number, message: string) => {
      if (msg.id === undefined) return;
      write({ jsonrpc: '2.0', id: msg.id, error: { code, message } });
    };

    try {
      switch (msg.method) {
        case 'initialize':
          respond({
            protocolVersion: '2024-11-05',
            capabilities: { tools: { listChanged: false } },
            serverInfo: { name: this.options.id, version: '0.0.0' },
          });
          return;

        case 'notifications/initialized':
          return;

        case 'tools/list':
          respond({
            tools: [...this.tools.entries()].map(([name, handler]) => ({
              name,
              description: handler.description ?? '',
              inputSchema: handler.inputSchema ?? { type: 'object' },
            })),
          });
          return;

        case 'tools/call': {
          const params = msg.params as { name?: string; arguments?: unknown };
          const name = params?.name ?? '';
          const args = (params?.arguments ?? {}) as unknown;
          const handler = this.tools.get(name);
          if (!handler) {
            respondError(-32601, `tool not found: ${name}`);
            return;
          }
          const ctx = this.buildContext(args, msg.id, write);
          const result = await handler.run(ctx);
          respond({
            content: [
              {
                type: 'text',
                text:
                  typeof result === 'string' ? result : JSON.stringify(result),
              },
            ],
            isError: false,
          });
          return;
        }

        case 'hooks/fire': {
          const params = msg.params as { event?: string; payload?: unknown };
          const handlers = this.hooks.get(params?.event ?? '') ?? [];
          for (const fn of handlers) {
            try {
              await fn({
                event: params?.event ?? '',
                payload: params?.payload,
                core: this.coreApi(),
                log: (line) => process.stderr.write(line + '\n'),
              });
            } catch (e) {
              process.stderr.write(`hook error: ${(e as Error).message}\n`);
            }
          }
          respond({ ok: true });
          return;
        }

        default:
          respondError(-32601, `method not found: ${msg.method}`);
      }
    } catch (e) {
      respondError(-32000, (e as Error).message);
    }
  }

  private buildContext(
    input: unknown,
    _id: JsonRpcMessage['id'],
    _write: (m: unknown) => void
  ): ToolContext {
    return {
      input,
      core: this.coreApi(),
      oauth: this.oauthMap(),
      progress: (_fraction: number, _message?: string) => {
        // Progress notifications require streaming support; V1 SDK elides them.
      },
      log: (line: string) => process.stderr.write(line + '\n'),
    };
  }

  private coreApi(): CoreApi {
    if (this.core) return this.core;
    const socket = process.env.ORBIT_CORE_API_SOCKET;
    if (!socket) {
      throw new Error('ORBIT_CORE_API_SOCKET not set — core API unavailable');
    }
    this.core = createCoreApi(socket);
    return this.core;
  }

  private oauthMap(): Record<string, { accessToken: string | undefined }> {
    const prefix = 'ORBIT_OAUTH_';
    const suffix = '_ACCESS_TOKEN';
    if (Object.keys(this.oauth).length === 0) {
      for (const key of Object.keys(process.env)) {
        if (key.startsWith(prefix) && key.endsWith(suffix)) {
          const providerId = key
            .slice(prefix.length, key.length - suffix.length)
            .toLowerCase();
          this.oauth[providerId] = { accessToken: process.env[key] };
        }
      }
    }
    return this.oauth;
  }
}
