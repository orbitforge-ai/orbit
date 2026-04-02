import { useState, useRef, useEffect } from 'react';
import { Cloud, WifiOff, LogOut, LogIn } from 'lucide-react';
import { cn } from '../lib/cn';
import { useAuthStore } from '../store/authStore';

export function SyncIndicator() {
  const { state, logout } = useAuthStore();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // Close popover on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  if (!state || state.mode === 'unset') return null;

  const isCloud = state.mode === 'cloud';
  const email = isCloud ? state.email : null;

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className={cn(
          'w-full flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-colors',
          'text-secondary hover:bg-surface hover:text-primary'
        )}
      >
        {isCloud ? (
          <Cloud size={13} className="text-success shrink-0" />
        ) : (
          <WifiOff size={13} className="text-muted shrink-0" />
        )}
        <span className="flex-1 text-left truncate">
          {isCloud ? (email ?? 'Synced') : 'Offline'}
        </span>
        <span
          className={cn(
            'w-1.5 h-1.5 rounded-full shrink-0',
            isCloud ? 'bg-success' : 'bg-muted'
          )}
        />
      </button>

      {open && (
        <div className="absolute bottom-full left-0 right-0 mb-1 bg-panel border border-edge rounded-lg shadow-lg overflow-hidden text-sm">
          {isCloud ? (
            <>
              <div className="px-3 py-2 border-b border-edge">
                <p className="text-xs text-muted">Signed in as</p>
                <p className="text-primary font-medium truncate">{email}</p>
              </div>
              <button
                onClick={async () => {
                  setOpen(false);
                  await logout();
                }}
                className="w-full flex items-center gap-2 px-3 py-2 text-secondary hover:bg-surface hover:text-primary transition-colors"
              >
                <LogOut size={13} />
                Sign out
              </button>
            </>
          ) : (
            <>
              <div className="px-3 py-2 border-b border-edge">
                <p className="text-xs text-muted">Running offline</p>
                <p className="text-secondary text-xs">Data is stored locally only.</p>
              </div>
              <button
                onClick={() => {
                  setOpen(false);
                  // Trigger auth screen by resetting to unset — App.tsx handles routing
                  useAuthStore.setState({ state: { mode: 'unset' } });
                }}
                className="w-full flex items-center gap-2 px-3 py-2 text-secondary hover:bg-surface hover:text-primary transition-colors"
              >
                <LogIn size={13} />
                Sign in to sync
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
