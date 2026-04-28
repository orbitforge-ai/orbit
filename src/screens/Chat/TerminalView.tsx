import { useEffect, useRef, useState } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import '@xterm/xterm/css/xterm.css';
import { RotateCw } from 'lucide-react';
import { terminalsApi, encodeBase64, type CliKind } from '../../api/terminals';
import type { UnlistenFn } from '../../api/transport';

const CLI_OPTIONS: { value: CliKind; label: string; description: string }[] = [
  { value: 'claude', label: 'Claude Code', description: 'claude' },
  { value: 'codex', label: 'Codex', description: 'codex' },
  { value: 'gemini', label: 'Gemini', description: 'gemini' },
  { value: 'shell', label: 'Shell', description: '$SHELL' },
];

interface TerminalViewProps {
  /** Optional chat session id; when set the PTY uses that agent's workspace + system prompt. */
  sessionId?: string | null;
  /** Notified when the running app sets its xterm title (OSC 0/2). */
  onTitleChange?: (title: string) => void;
}

export function TerminalView({ sessionId, onTitleChange }: TerminalViewProps) {
  const [kind, setKind] = useState<CliKind>('claude');
  const [restartKey, setRestartKey] = useState(0);

  return (
    <div className="flex h-full flex-col bg-[#0b0b0e]">
      <div className="flex items-center justify-between border-b border-edge px-3 py-1.5 text-[11px]">
        <div className="flex items-center gap-2">
          <span className="text-muted">CLI:</span>
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as CliKind)}
            className="rounded border border-edge bg-background px-2 py-0.5 text-[11px] text-white"
          >
            {CLI_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </div>
        <button
          type="button"
          onClick={() => setRestartKey((k) => k + 1)}
          className="inline-flex items-center gap-1 rounded border border-edge px-2 py-0.5 text-muted hover:text-white"
          title="Restart terminal"
        >
          <RotateCw size={11} />
          Restart
        </button>
      </div>
      <div className="min-h-0 flex-1">
        <PtyHost
          key={`${sessionId ?? 'global'}:${kind}:${restartKey}`}
          sessionId={sessionId ?? null}
          kind={kind}
          onTitleChange={onTitleChange}
        />
      </div>
    </div>
  );
}

interface PtyHostProps {
  sessionId: string | null;
  kind: CliKind;
  onTitleChange?: (title: string) => void;
}

function PtyHost({ sessionId, kind, onTitleChange }: PtyHostProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let terminalId: string | null = null;
    let unlistenChunk: UnlistenFn | null = null;
    let unlistenExit: UnlistenFn | null = null;
    let resizeObs: ResizeObserver | null = null;
    let cancelled = false;

    const term = new Terminal({
      fontFamily:
        'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      theme: {
        background: '#0b0b0e',
        foreground: '#e6e6e6',
        cursor: '#e6e6e6',
      },
    });
    const fit = new FitAddon();
    const links = new WebLinksAddon();
    term.loadAddon(fit);
    term.loadAddon(links);
    term.open(container);

    if (onTitleChange) {
      term.onTitleChange((t) => onTitleChange(t));
    }

    try {
      fit.fit();
    } catch {
      // ignore
    }
    const initialRows = term.rows || 24;
    const initialCols = term.cols || 80;

    (async () => {
      try {
        const { terminalId: id } = await terminalsApi.open(
          sessionId,
          kind,
          initialRows,
          initialCols
        );
        if (cancelled) {
          await terminalsApi.close(id);
          return;
        }
        terminalId = id;

        unlistenChunk = await terminalsApi.onChunk(id, (bytes) => {
          term.write(bytes);
        });
        unlistenExit = await terminalsApi.onExit(id, (code) => {
          term.write(`\r\n\x1b[2m[process exited with code ${code}]\x1b[0m\r\n`);
        });

        term.onData((data) => {
          terminalsApi.write(id, encodeBase64(data)).catch(() => {});
        });
        term.onResize(({ rows, cols }) => {
          terminalsApi.resize(id, rows, cols).catch(() => {});
        });

        try {
          fit.fit();
        } catch {
          // ignore
        }
        terminalsApi.resize(id, term.rows, term.cols).catch(() => {});

        resizeObs = new ResizeObserver(() => {
          try {
            fit.fit();
          } catch {
            // ignore
          }
        });
        resizeObs.observe(container);

        term.focus();
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    })();

    return () => {
      cancelled = true;
      if (resizeObs) resizeObs.disconnect();
      if (unlistenChunk) unlistenChunk();
      if (unlistenExit) unlistenExit();
      if (terminalId) {
        terminalsApi.close(terminalId).catch(() => {});
      }
      term.dispose();
    };
  }, [sessionId, kind, onTitleChange]);

  if (error) {
    return (
      <div className="flex h-full items-center justify-center px-6 text-center">
        <div className="max-w-sm text-sm text-red-400">
          Failed to start terminal: {error}
        </div>
      </div>
    );
  }

  return <div ref={containerRef} className="h-full w-full p-2" />;
}
