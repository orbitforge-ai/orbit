/**
 * @orbit/plugin-sdk — author-facing entry points for building Orbit plugins
 * in Node.js. Wraps the MCP stdio server, the core-API unix socket, and
 * OAuth token reading so plugin authors only think about their logic.
 */

export { Plugin } from './plugin.js';
export type {
  ToolHandler,
  ToolContext,
  HookHandler,
  HookContext,
  ProgressFn,
  LogFn,
} from './types.js';
export { defineSchema } from './schema.js';
export type { CoreApi, CoreEntity } from './core.js';
