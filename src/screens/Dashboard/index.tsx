import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Play, Clock, CheckCircle, XCircle, Plus } from "lucide-react";
import { runsApi } from "../../api/runs";
import { tasksApi } from "../../api/tasks";
import { schedulesApi } from "../../api/schedules";
import { StatusBadge } from "../../components/StatusBadge";
import { onRunStateChanged } from "../../events/runEvents";
import { useLiveRunStore } from "../../store/liveRunStore";
import { useUiStore } from "../../store/uiStore";
import { formatDuration, formatElapsed } from "../../lib/formatDuration";
import { humanSchedule } from "../../lib/humanSchedule";
import { RecurringConfig, RunState } from "../../types";

export function Dashboard() {
  const queryClient = useQueryClient();
  const { navigate, selectRun } = useUiStore();
  const { activeRuns, upsertRun, updateRunState } = useLiveRunStore();

  const { data: runs = [] } = useQuery({
    queryKey: ["runs", "recent"],
    queryFn: () => runsApi.list({ limit: 20 }),
    refetchInterval: 10_000,
  });

  const { data: tasks = [] } = useQuery({
    queryKey: ["tasks"],
    queryFn: tasksApi.list,
    refetchInterval: 30_000,
  });

  const { data: schedules = [] } = useQuery({
    queryKey: ["schedules"],
    queryFn: schedulesApi.list,
    refetchInterval: 30_000,
  });

  const { data: active = [] } = useQuery({
    queryKey: ["runs", "active"],
    queryFn: runsApi.getActive,
    refetchInterval: 5_000,
  });

  // Seed live store with active runs on load
  useEffect(() => {
    for (const run of active) {
      upsertRun(run);
    }
  }, [active, upsertRun]);

  // Subscribe to run state change events
  useEffect(() => {
    const unlisten = onRunStateChanged((payload) => {
      updateRunState(payload.runId, payload.newState);
      queryClient.invalidateQueries({ queryKey: ["runs"] });
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [queryClient, updateRunState]);

  const activeRunList = Object.values(activeRuns);
  const recentRuns = runs.filter(
    (r) => !["pending", "queued", "running"].includes(r.state)
  ).slice(0, 10);

  // Upcoming schedules: enabled schedules with a nextRunAt
  const upcoming = schedules
    .filter((s) => s.enabled && s.nextRunAt)
    .sort((a, b) => a.nextRunAt!.localeCompare(b.nextRunAt!))
    .slice(0, 5);

  return (
    <div className="flex flex-col h-full p-6 overflow-y-auto gap-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold text-white">Dashboard</h2>
          <p className="text-sm text-[#64748b] mt-0.5">
            {tasks.length} tasks · {schedules.filter((s) => s.enabled).length} active schedules
          </p>
        </div>
        <button
          onClick={() => navigate("task-builder")}
          className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] text-white text-sm font-medium transition-colors"
        >
          <Plus size={14} />
          New Task
        </button>
      </div>

      {/* Active Now */}
      <section>
        <h3 className="text-sm font-medium text-[#64748b] uppercase tracking-wider mb-3">
          Active Now
          {activeRunList.length > 0 && (
            <span className="ml-2 px-1.5 py-0.5 rounded bg-blue-500/20 text-blue-400 text-xs normal-case font-normal">
              {activeRunList.length}
            </span>
          )}
        </h3>

        {activeRunList.length === 0 ? (
          <div className="rounded-xl border border-[#2a2d3e] bg-[#1a1d27] p-8 text-center">
            <Play size={24} className="text-[#2a2d3e] mx-auto mb-2" />
            <p className="text-sm text-[#64748b]">No runs in progress</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3">
            {activeRunList.map((run) => (
              <ActiveRunCard
                key={run.runId}
                run={run}
                onClick={() => {
                  selectRun(run.runId);
                  navigate("history");
                }}
              />
            ))}
          </div>
        )}
      </section>

      {/* Upcoming */}
      {upcoming.length > 0 && (
        <section>
          <h3 className="text-sm font-medium text-[#64748b] uppercase tracking-wider mb-3">
            Upcoming
          </h3>
          <div className="rounded-xl border border-[#2a2d3e] bg-[#1a1d27] divide-y divide-[#2a2d3e] overflow-hidden">
            {upcoming.map((sched) => {
              const task = tasks.find((t) => t.id === sched.taskId);
              const nextRunDate = new Date(sched.nextRunAt!);
              const diffMs = nextRunDate.getTime() - Date.now();
              const diffMins = Math.round(diffMs / 60000);

              return (
                <div key={sched.id} className="flex items-center gap-3 px-4 py-3">
                  <Clock size={14} className="text-[#6366f1] flex-shrink-0" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm text-white truncate">
                      {task?.name ?? "Unknown task"}
                    </p>
                    {sched.kind === "recurring" && (
                      <p className="text-xs text-[#64748b]">
                        {humanSchedule(sched.config as RecurringConfig)}
                      </p>
                    )}
                  </div>
                  <span className="text-xs text-[#64748b] flex-shrink-0">
                    {diffMins <= 0
                      ? "Now"
                      : diffMins < 60
                      ? `in ${diffMins}m`
                      : `in ${Math.round(diffMins / 60)}h`}
                  </span>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* Recent Activity */}
      <section>
        <h3 className="text-sm font-medium text-[#64748b] uppercase tracking-wider mb-3">
          Recent Activity
        </h3>

        {recentRuns.length === 0 ? (
          <div className="rounded-xl border border-[#2a2d3e] bg-[#1a1d27] p-8 text-center">
            <CheckCircle size={24} className="text-[#2a2d3e] mx-auto mb-2" />
            <p className="text-sm text-[#64748b]">No runs yet</p>
            <p className="text-xs text-[#64748b] mt-1">
              Create a task and it will appear here once it runs
            </p>
          </div>
        ) : (
          <div className="rounded-xl border border-[#2a2d3e] bg-[#1a1d27] overflow-hidden">
            {recentRuns.map((run, i) => (
              <button
                key={run.id}
                onClick={() => {
                  selectRun(run.id);
                  navigate("history");
                }}
                className={`w-full flex items-center gap-3 px-4 py-3 hover:bg-[#222533] text-left transition-colors ${
                  i > 0 ? "border-t border-[#2a2d3e]" : ""
                }`}
              >
                <StatusBadge state={run.state} className="flex-shrink-0" />
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-white truncate">{run.taskName}</p>
                  <p className="text-xs text-[#64748b]">
                    {run.trigger === "scheduled" ? "Scheduled" : "Manual"} ·{" "}
                    {run.startedAt
                      ? new Date(run.startedAt).toLocaleString()
                      : "—"}
                  </p>
                </div>
                <span className="text-xs text-[#64748b] flex-shrink-0">
                  {formatDuration(run.durationMs)}
                </span>
              </button>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function ActiveRunCard({
  run,
  onClick,
}: {
  run: { id?: string; runId?: string; taskName?: string; state: RunState; startedAt: string | null };
  onClick: () => void;
}) {
  const liveRun = useLiveRunStore(
    (s) => s.activeRuns[run.id ?? run.runId ?? ""]
  );
  const state = liveRun?.state ?? run.state;
  const taskName = liveRun?.taskName ?? run.taskName;
  const startedAt = liveRun?.startedAt ?? run.startedAt;

  return (
    <button
      onClick={onClick}
      className="flex items-center gap-3 px-4 py-3 rounded-xl border border-blue-500/30 bg-blue-500/5 hover:bg-blue-500/10 text-left transition-colors"
    >
      <div className="w-2 h-2 rounded-full bg-blue-400 animate-pulse flex-shrink-0" />
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium text-white truncate">{taskName}</p>
        <p className="text-xs text-[#64748b]">
          <StatusBadge state={state} className="mr-1" />
          {formatElapsed(startedAt)}
        </p>
      </div>
      <XCircle size={14} className="text-[#64748b] flex-shrink-0 hover:text-red-400" />
    </button>
  );
}
