import { cn } from "../lib/cn";
import { RunState } from "../types";

const STATE_CONFIG: Record<
  RunState | "idle" | "busy",
  { label: string; className: string; dot?: boolean }
> = {
  pending: { label: "Pending", className: "bg-yellow-500/15 text-yellow-400 border-yellow-500/30", dot: true },
  queued: { label: "Queued", className: "bg-blue-500/15 text-blue-400 border-blue-500/30", dot: true },
  running: { label: "Running", className: "bg-blue-500/15 text-blue-400 border-blue-500/30", dot: true },
  success: { label: "Success", className: "bg-green-500/15 text-green-400 border-green-500/30" },
  failure: { label: "Failed", className: "bg-red-500/15 text-red-400 border-red-500/30" },
  cancelled: { label: "Cancelled", className: "bg-slate-500/15 text-slate-400 border-slate-500/30" },
  timed_out: { label: "Timed Out", className: "bg-orange-500/15 text-orange-400 border-orange-500/30" },
  idle: { label: "Idle", className: "bg-slate-500/15 text-slate-400 border-slate-500/30" },
  busy: { label: "Busy", className: "bg-blue-500/15 text-blue-400 border-blue-500/30", dot: true },
};

interface StatusBadgeProps {
  state: string;
  className?: string;
}

export function StatusBadge({ state, className }: StatusBadgeProps) {
  const cfg = STATE_CONFIG[state as keyof typeof STATE_CONFIG] ?? {
    label: state,
    className: "bg-slate-500/15 text-slate-400 border-slate-500/30",
  };

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium border",
        cfg.className,
        className
      )}
    >
      {cfg.dot && (
        <span className="w-1.5 h-1.5 rounded-full bg-current animate-pulse" />
      )}
      {cfg.label}
    </span>
  );
}
