import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown } from "lucide-react";
import * as Select from "@radix-ui/react-select";
import { schedulesApi } from "../../api/schedules";
import { tasksApi } from "../../api/tasks";
import { StatusBadge } from "../../components/StatusBadge";
import { humanSchedule } from "../../lib/humanSchedule";
import { RecurringConfig, Schedule, Task } from "../../types";
import { RecurringPicker } from "./RecurringPicker";

export function ScheduleBuilderScreen() {
  const [creating, setCreating] = useState(false);
  const [selectedTaskId, setSelectedTaskId] = useState("");
  const [config, setConfig] = useState<RecurringConfig>({
    intervalUnit: "hours",
    intervalValue: 1,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    missedRunPolicy: "skip",
  });

  const { data: tasks = [] } = useQuery({
    queryKey: ["tasks"],
    queryFn: tasksApi.list,
    select: (all: Task[]) => all.filter((t) => !t.tags.includes("pulse")),
  });

  const pulseTaskIds = useQuery({
    queryKey: ["tasks"],
    queryFn: tasksApi.list,
    select: (all: Task[]) => new Set(all.filter((t) => t.tags.includes("pulse")).map((t) => t.id)),
  }).data ?? new Set<string>();

  const { data: schedules = [], refetch } = useQuery({
    queryKey: ["schedules"],
    queryFn: schedulesApi.list,
    select: (all: Schedule[]) => all.filter((s) => !pulseTaskIds.has(s.taskId)),
  });

  async function handleCreate() {
    if (!selectedTaskId) return;
    await schedulesApi.create({
      taskId: selectedTaskId,
      kind: "recurring",
      config,
    });
    setCreating(false);
    refetch();
  }

  return (
    <div className="flex h-full">
      {/* Schedule list */}
      <div className="flex-1 flex flex-col h-full overflow-hidden">
        <div className="flex items-center justify-between px-6 py-4 border-b border-[#2a2d3e]">
          <h2 className="text-lg font-semibold text-white">Schedules</h2>
          <button
            onClick={() => setCreating(true)}
            className="px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] text-white text-sm font-medium transition-colors"
          >
            + New Schedule
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-2">
          {schedules.length === 0 && (
            <div className="text-center py-16 text-[#64748b] text-sm">
              No schedules yet. Create one to automate your tasks.
            </div>
          )}
          {schedules.map((s) => (
            <ScheduleCard
              key={s.id}
              schedule={s}
              taskName={tasks.find((t) => t.id === s.taskId)?.name ?? "Unknown task"}
              onToggle={() =>
                schedulesApi.toggle(s.id, !s.enabled).then(() => refetch())
              }
              onDelete={() =>
                schedulesApi.delete(s.id).then(() => refetch())
              }
            />
          ))}
        </div>
      </div>

      {/* Create panel */}
      {creating && (
        <div className="w-[400px] border-l border-[#2a2d3e] flex flex-col bg-[#13151e]">
          <div className="flex items-center justify-between px-4 py-4 border-b border-[#2a2d3e]">
            <h3 className="font-semibold text-white">New Schedule</h3>
            <button
              onClick={() => setCreating(false)}
              className="text-[#64748b] hover:text-white text-lg leading-none"
            >
              ×
            </button>
          </div>

          <div className="flex-1 overflow-y-auto p-4 space-y-4">
            {/* Task selector */}
            <div>
              <label className="block text-xs font-medium text-[#64748b] mb-1.5">
                Task
              </label>
              <Select.Root value={selectedTaskId} onValueChange={setSelectedTaskId}>
                <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]">
                  <Select.Value placeholder="Select a task…" />
                  <Select.Icon><ChevronDown size={14} className="text-[#64748b]" /></Select.Icon>
                </Select.Trigger>
                <Select.Portal>
                  <Select.Content className="rounded-lg bg-[#1a1d27] border border-[#2a2d3e] shadow-xl overflow-hidden z-50">
                    <Select.Viewport className="p-1">
                      {tasks.map((t) => (
                        <Select.Item key={t.id} value={t.id} className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-[#6366f1]/20">
                          <Select.ItemText>{t.name}</Select.ItemText>
                        </Select.Item>
                      ))}
                    </Select.Viewport>
                  </Select.Content>
                </Select.Portal>
              </Select.Root>
            </div>

            <RecurringPicker value={config} onChange={setConfig} />
          </div>

          <div className="px-4 py-3 border-t border-[#2a2d3e]">
            <button
              disabled={!selectedTaskId}
              onClick={handleCreate}
              className="w-full py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
            >
              Create Schedule
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function ScheduleCard({
  schedule,
  taskName,
  onToggle,
  onDelete,
}: {
  schedule: Schedule;
  taskName: string;
  onToggle: () => void;
  onDelete: () => void;
}) {
  const cfg = schedule.config as RecurringConfig;
  const label = schedule.kind === "recurring" ? humanSchedule(cfg) : schedule.kind;

  return (
    <div className="flex items-center gap-3 px-4 py-3 rounded-xl border border-[#2a2d3e] bg-[#1a1d27]">
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium text-white truncate">{taskName}</p>
        <p className="text-xs text-[#64748b]">{label}</p>
        {schedule.nextRunAt && (
          <p className="text-xs text-[#6366f1] mt-0.5">
            Next: {new Date(schedule.nextRunAt).toLocaleString()}
          </p>
        )}
      </div>
      <StatusBadge state={schedule.enabled ? "idle" : "cancelled"} />
      <button
        onClick={onToggle}
        className="px-2 py-1 rounded text-xs text-[#64748b] hover:text-white hover:bg-[#2a2d3e] transition-colors"
      >
        {schedule.enabled ? "Pause" : "Resume"}
      </button>
      <button
        onClick={onDelete}
        className="px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10 transition-colors"
      >
        Delete
      </button>
    </div>
  );
}
