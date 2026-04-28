import { invoke, listen, type UnlistenFn } from './transport';

export type CliKind = 'claude' | 'codex' | 'gemini' | 'shell';

export interface OpenTerminalArgs {
  sessionId: string;
  kind: { kind: CliKind };
  rows: number;
  cols: number;
}

export interface OpenTerminalResponse {
  terminalId: string;
}

export interface TerminalChunkPayload {
  terminalId: string;
  data: string; // base64
  timestamp: string;
}

export interface TerminalExitPayload {
  terminalId: string;
  code: number;
  timestamp: string;
}

export const terminalsApi = {
  open: (sessionId: string | null, kind: CliKind, rows: number, cols: number) =>
    invoke<OpenTerminalResponse>('open_terminal', {
      args: { sessionId, kind: { kind }, rows, cols },
    }),

  write: (terminalId: string, data: string) =>
    invoke<void>('write_terminal', { args: { terminalId, data } }),

  resize: (terminalId: string, rows: number, cols: number) =>
    invoke<void>('resize_terminal', { args: { terminalId, rows, cols } }),

  close: (terminalId: string) =>
    invoke<void>('close_terminal', { args: { terminalId } }),

  onChunk: (terminalId: string, cb: (bytes: Uint8Array) => void): Promise<UnlistenFn> =>
    listen<TerminalChunkPayload>('terminal:output_chunk', (event) => {
      if (event.payload.terminalId !== terminalId) return;
      cb(decodeBase64(event.payload.data));
    }),

  onExit: (terminalId: string, cb: (code: number) => void): Promise<UnlistenFn> =>
    listen<TerminalExitPayload>('terminal:exit', (event) => {
      if (event.payload.terminalId !== terminalId) return;
      cb(event.payload.code);
    }),
};

function decodeBase64(b64: string): Uint8Array {
  const binary = atob(b64);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

export function encodeBase64(bytes: Uint8Array | string): string {
  const buf = typeof bytes === 'string' ? new TextEncoder().encode(bytes) : bytes;
  let binary = '';
  for (let i = 0; i < buf.length; i++) binary += String.fromCharCode(buf[i]);
  return btoa(binary);
}
