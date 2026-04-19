import { CheckCircle2, XCircle, Info, X } from 'lucide-react';
import { Toast, ToastKind, useToastStore } from '../store/toastStore';

const ICONS: Record<ToastKind, typeof CheckCircle2> = {
  success: CheckCircle2,
  error: XCircle,
  info: Info,
};

const COLORS: Record<ToastKind, string> = {
  success: 'text-emerald-400',
  error: 'text-red-400',
  info: 'text-accent-hover',
};

function ToastItem({ toast }: { toast: Toast }) {
  const dismiss = useToastStore((s) => s.dismiss);
  const Icon = ICONS[toast.kind];
  return (
    <div
      role="status"
      className="flex items-start gap-2.5 px-3 py-2.5 rounded-lg bg-panel border border-edge shadow-lg min-w-[260px] max-w-[360px] pointer-events-auto"
    >
      <Icon size={14} className={`${COLORS[toast.kind]} shrink-0 mt-0.5`} />
      <div className="flex-1 min-w-0">
        <div className="text-xs text-white leading-tight">{toast.message}</div>
        {toast.detail && (
          <div className="text-[11px] text-muted mt-0.5 font-mono truncate">{toast.detail}</div>
        )}
      </div>
      <button
        onClick={() => dismiss(toast.id)}
        aria-label="Dismiss notification"
        className="p-0.5 rounded text-muted hover:text-white shrink-0"
      >
        <X size={12} />
      </button>
    </div>
  );
}

export function ToastContainer() {
  const toasts = useToastStore((s) => s.toasts);
  return (
    <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2 pointer-events-none">
      {toasts.map((t) => (
        <ToastItem key={t.id} toast={t} />
      ))}
    </div>
  );
}
