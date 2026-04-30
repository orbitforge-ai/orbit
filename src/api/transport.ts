/**
 * Single injection point for IPC. All `invoke` and `listen` calls in the
 * frontend should import from here, not directly from `@tauri-apps/api/*`.
 *
 * Modes (selected via `VITE_TRANSPORT_MODE`, default `tauri`):
 *   - `tauri` — passthrough to `@tauri-apps/api/core` and `@tauri-apps/api/event`
 *   - `http`  — POST `/rpc/:cmd` and a singleton WebSocket on `/ws`
 *
 * In `http` mode the Vite proxy (vite.config.ts) forwards both to the shim
 * running inside the Tauri process at 127.0.0.1:8765. The dev token comes
 * from `VITE_DEV_TOKEN` injected by the `orbit-dev-token` Vite plugin.
 *
 * The shim emits one WS message shape per event:
 *   { channel: string, payload: unknown }
 * plus an out-of-band `{ type: "lagged", missed: N }` notice if the broadcast
 * channel drops messages for slow consumers.
 */

import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import {
  listen as tauriListen,
  type EventCallback,
  type UnlistenFn,
} from '@tauri-apps/api/event';

type Mode = 'tauri' | 'http';

const MODE: Mode =
  (import.meta.env.VITE_TRANSPORT_MODE as Mode | undefined) ?? 'tauri';

const API_BASE: string =
  (import.meta.env.VITE_ORBIT_API_URL as string | undefined) ?? '';

const DEV_TOKEN: string | undefined = import.meta.env.VITE_DEV_TOKEN as
  | string
  | undefined;

// ─── HTTP invoke ────────────────────────────────────────────────────────────

async function httpInvoke<T>(cmd: string, args?: object): Promise<T> {
  const url = `${API_BASE}/rpc/${cmd}`;
  const headers: Record<string, string> = {
    'content-type': 'application/json',
  };
  const token = currentToken();
  if (token) headers.authorization = `Bearer ${token}`;

  const res = await fetch(url, {
    method: 'POST',
    headers,
    body: JSON.stringify(args ?? {}),
  });

  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const body = (await res.json()) as { error?: string };
      if (body?.error) message = body.error;
    } catch {
      // ignore
    }
    throw new Error(message);
  }

  return (await res.json()) as T;
}

function currentToken(): string | undefined {
  if (DEV_TOKEN) return DEV_TOKEN;
  if (typeof window !== 'undefined') {
    const stored = window.localStorage?.getItem('orbit_access_token');
    if (stored) return stored;
  }
  return undefined;
}

// ─── WebSocket listen ───────────────────────────────────────────────────────

type Handler = (payload: unknown) => void;

class WsClient {
  private socket: WebSocket | null = null;
  private connecting = false;
  private listeners: Map<string, Set<Handler>> = new Map();
  private backoffMs = 500;
  private destroyed = false;
  private statusListeners: Set<(s: 'connected' | 'disconnected' | 'lagged') => void> =
    new Set();

  ensureConnected() {
    if (this.socket || this.connecting || this.destroyed) return;
    this.connecting = true;
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    // When API_BASE is set, point the WS there too; otherwise rely on the
    // Vite proxy at the current origin.
    const base = API_BASE
      ? API_BASE.replace(/^http/, 'ws')
      : `${proto}//${location.host}`;
    const token = currentToken();
    const url = `${base}/ws${token ? `?token=${encodeURIComponent(token)}` : ''}`;
    const ws = new WebSocket(url);

    ws.onopen = () => {
      this.connecting = false;
      this.backoffMs = 500;
      this.notifyStatus('connected');
    };
    ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data as string) as
          | { channel: string; payload: unknown }
          | { type: 'lagged'; missed: number }
          | { type: 'error'; error: string };
        if ('type' in msg) {
          if (msg.type === 'lagged') this.notifyStatus('lagged');
          return;
        }
        const set = this.listeners.get(msg.channel);
        if (!set) return;
        for (const h of set) h(msg.payload);
      } catch (err) {
        console.warn('shim ws: bad message', err);
      }
    };
    ws.onclose = () => {
      this.socket = null;
      this.connecting = false;
      this.notifyStatus('disconnected');
      if (this.destroyed) return;
      const delay = Math.min(this.backoffMs, 30_000);
      this.backoffMs = Math.min(this.backoffMs * 2, 30_000);
      setTimeout(() => this.ensureConnected(), delay + Math.random() * 250);
    };
    ws.onerror = () => {
      // Let onclose handle reconnection.
    };
    this.socket = ws;
  }

  addListener(channel: string, h: Handler): () => void {
    let set = this.listeners.get(channel);
    if (!set) {
      set = new Set();
      this.listeners.set(channel, set);
    }
    set.add(h);
    this.ensureConnected();
    return () => {
      const s = this.listeners.get(channel);
      if (!s) return;
      s.delete(h);
      if (s.size === 0) this.listeners.delete(channel);
    };
  }

  onStatus(cb: (s: 'connected' | 'disconnected' | 'lagged') => void): () => void {
    this.statusListeners.add(cb);
    return () => this.statusListeners.delete(cb);
  }

  private notifyStatus(s: 'connected' | 'disconnected' | 'lagged') {
    for (const cb of this.statusListeners) {
      try {
        cb(s);
      } catch (e) {
        console.warn('shim ws status callback threw', e);
      }
    }
  }
}

let wsClient: WsClient | null = null;
function ws(): WsClient {
  if (!wsClient) wsClient = new WsClient();
  return wsClient;
}

async function httpListen<T>(
  channel: string,
  cb: EventCallback<T>,
): Promise<UnlistenFn> {
  const dispose = ws().addListener(channel, (payload) => {
    // Tauri's EventCallback expects an `Event<T>` shape — match it loosely.
    cb({
      event: channel,
      id: 0,
      payload: payload as T,
    } as Parameters<EventCallback<T>>[0]);
  });
  return () => {
    dispose();
  };
}

// ─── Public API ─────────────────────────────────────────────────────────────

export const TRANSPORT_MODE = MODE;

export const invoke: <T = unknown>(cmd: string, args?: object) => Promise<T> =
  MODE === 'http' ? httpInvoke : (tauriInvoke as never);

export const listen: <T>(
  channel: string,
  cb: EventCallback<T>,
) => Promise<UnlistenFn> = MODE === 'http' ? httpListen : tauriListen;

export type { EventCallback, UnlistenFn };

/**
 * Subscribe to shim WS connection-state transitions (browser mode only).
 * Returns a no-op unsubscribe in tauri mode.
 */
export function onTransportStatus(
  cb: (s: 'connected' | 'disconnected' | 'lagged') => void,
): () => void {
  if (MODE !== 'http') return () => {};
  return ws().onStatus(cb);
}
