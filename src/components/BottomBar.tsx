import { useCallback, useEffect, useRef } from 'react';
import { TerminalSquare, X } from 'lucide-react';
import { TerminalView } from '../screens/Chat/TerminalView';
import { useTerminalStore } from '../store/terminalStore';

const DRAWER_HEIGHT = 360;

export function BottomBar() {
  const open = useTerminalStore((s) => s.open);
  const title = useTerminalStore((s) => s.title);
  const toggle = useTerminalStore((s) => s.toggle);
  const setOpen = useTerminalStore((s) => s.setOpen);
  const setTitle = useTerminalStore((s) => s.setTitle);

  // Mount the terminal once it's been opened, but keep it mounted thereafter
  // so opening/closing the drawer doesn't restart the PTY.
  const everOpenedRef = useRef(false);
  if (open) everOpenedRef.current = true;

  // Reset the title on close so a future open doesn't show the stale name.
  useEffect(() => {
    if (!open) setTitle('');
  }, [open, setTitle]);

  const handleTitleChange = useCallback(
    (t: string) => setTitle(t),
    [setTitle]
  );

  return (
    <>
      {/* Slide-in drawer above the bottom bar. */}
      <div
        aria-hidden={!open}
        className="pointer-events-none fixed inset-x-0 z-40 transition-transform duration-200 ease-out"
        style={{
          bottom: 28, // sits directly on top of the bottom bar
          height: DRAWER_HEIGHT,
          transform: open ? 'translateY(0)' : `translateY(${DRAWER_HEIGHT + 28}px)`,
        }}
      >
        <div
          className={`pointer-events-auto flex h-full flex-col border-t border-edge bg-surface shadow-2xl ${
            open ? '' : 'invisible'
          }`}
        >
          <div className="flex items-center justify-between border-b border-edge bg-background px-3 py-1 text-[11px]">
            <div className="truncate text-secondary" title={title}>
              {title || 'Terminal'}
            </div>
            <button
              type="button"
              onClick={() => setOpen(false)}
              className="rounded p-0.5 text-muted hover:bg-edge/50 hover:text-white"
              aria-label="Close terminal"
            >
              <X size={12} />
            </button>
          </div>
          <div className="min-h-0 flex-1">
            {everOpenedRef.current && (
              <TerminalView onTitleChange={handleTitleChange} />
            )}
          </div>
        </div>
      </div>

      {/* Slim, always-visible bottom bar. */}
      <div className="z-50 flex h-7 items-center border-t border-edge bg-background px-2 text-[11px]">
        <button
          type="button"
          onClick={toggle}
          className={`inline-flex items-center gap-1.5 rounded px-2 py-0.5 transition-colors ${
            open
              ? 'bg-accent/15 text-accent-hover'
              : 'text-muted hover:bg-edge/40 hover:text-white'
          }`}
          aria-pressed={open}
        >
          <TerminalSquare size={12} />
          Terminal
        </button>
      </div>
    </>
  );
}
