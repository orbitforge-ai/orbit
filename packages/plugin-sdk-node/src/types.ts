import type { CoreApi } from './core.js';

export type ProgressFn = (fraction: number, message?: string) => void;
export type LogFn = (line: string) => void;

export interface ToolContext {
  /** Parsed arguments. Typed from `defineSchema` when the schema is declared. */
  input: unknown;
  /** Core API client — read/write this plugin's entities, call whitelisted core entities. */
  core: CoreApi;
  /** OAuth tokens for every declared provider, keyed by provider id. */
  oauth: Record<string, { accessToken: string | undefined }>;
  /** Stream structured progress back to the agent. */
  progress: ProgressFn;
  /** Log to stderr (appears in the Plugin detail drawer's Live Log tab). */
  log: LogFn;
}

export interface HookContext {
  /** Hook event name, e.g. `entity.work_item.after_complete`. */
  event: string;
  /** Event payload. Shape depends on the event. */
  payload: unknown;
  core: CoreApi;
  log: LogFn;
}

export interface ToolHandlerOptions {
  description?: string;
  inputSchema?: Record<string, unknown>;
  run: (ctx: ToolContext) => Promise<unknown>;
}

export type ToolHandler = ToolHandlerOptions;

export type HookHandler = (ctx: HookContext) => Promise<void>;
