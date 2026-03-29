import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Zap,
  Save,
  Clock,
  Trash2,
  ExternalLink,
  Play,
} from "lucide-react";
import * as Switch from "@radix-ui/react-switch";
import { invoke } from "@tauri-apps/api/core";
import { tasksApi } from "../../api/tasks";
import { pulseApi, PulseConfig } from "../../api/pulse";
import { schedulesApi } from "../../api/schedules";
import { RecurringPicker } from "../ScheduleBuilder/RecurringPicker";
import { humanSchedule } from "../../lib/humanSchedule";
import { RecurringConfig, Schedule, Task } from "../../types";
import { useUiStore } from "../../store/uiStore";

const DEFAULT_SCHEDULE: RecurringConfig = {
  intervalUnit: "hours",
  intervalValue: 1,
  timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  missedRunPolicy: "skip" as const,
};

export function SchedulesTab({ agentId }: { agentId: string }) {
  return (
    <div className="p-6 space-y-8 h-full overflow-y-auto">
      <PulseSection agentId={agentId} />
      <div className="border-t border-[#2a2d3e]" />
      <AgentSchedulesList agentId={agentId} />
    </div>
  );
}

// ─── Pulse Section ──────────────────────────────────────────────────────────

function PulseSection({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();
  const { navigate } = useUiStore();
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [triggering, setTriggering] = useState(false);
  const [content, setContent] = useState("");
  const [schedule, setSchedule] = useState<RecurringConfig>(DEFAULT_SCHEDULE);
  const [enabled, setEnabled] = useState(false);

  const { data: pulseConfig } = useQuery<PulseConfig>({
    queryKey: ["pulse-config", agentId],
    queryFn: () => pulseApi.getConfig(agentId),
  });

  // Sync local state from loaded config
  useEffect(() => {
    if (pulseConfig) {
      setContent(pulseConfig.content);
      setEnabled(pulseConfig.enabled);
      if (pulseConfig.schedule) {
        setSchedule(pulseConfig.schedule);
      }
    }
  }, [pulseConfig]);

  async function handleSave() {
    setSaving(true);
    setSaved(false);
    try {
      await pulseApi.update(agentId, content, schedule, enabled);
      queryClient.invalidateQueries({ queryKey: ["pulse-config", agentId] });
      queryClient.invalidateQueries({ queryKey: ["schedules"] });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (err) {
      console.error("Failed to save pulse:", err);
    }
    setSaving(false);
  }

  return (
    <section className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Zap size={16} className="text-[#f59e0b]" />
          <h4 className="text-sm font-semibold text-white">Pulse</h4>
        </div>

        {/* Enable/disable toggle */}
        <div className="flex items-center gap-2">
          <span className={`text-xs ${enabled ? "text-emerald-400" : "text-[#64748b]"}`}>
            {enabled ? "Active" : "Inactive"}
          </span>
          <Switch.Root
            checked={enabled}
            onCheckedChange={setEnabled}
            className="w-9 h-5 rounded-full bg-[#2a2d3e] data-[state=checked]:bg-emerald-500 transition-colors outline-none"
          >
            <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
          </Switch.Root>
        </div>
      </div>

      <p className="text-xs text-[#64748b]">
        Define a recurring prompt that runs automatically on a schedule.
        All responses are logged to a dedicated Pulse chat session.
      </p>

      {/* Pulse content editor */}
      <div>
        <label className="text-xs text-[#64748b] mb-1 block">Prompt</label>
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          rows={6}
          className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm font-mono resize-y focus:outline-none focus:border-[#6366f1] leading-relaxed"
          placeholder="Describe what this agent should do on each pulse..."
        />
      </div>

      {/* Schedule picker */}
      <div>
        <label className="text-xs text-[#64748b] mb-1 block">Frequency</label>
        <RecurringPicker value={schedule} onChange={setSchedule} />
      </div>

      {/* Status info */}
      {pulseConfig?.nextRunAt && enabled && (
        <div className="flex items-center gap-2 text-xs text-[#64748b]">
          <Clock size={11} />
          <span>
            Next run:{" "}
            {new Date(pulseConfig.nextRunAt).toLocaleString()}
          </span>
        </div>
      )}
      {pulseConfig?.lastRunAt && (
        <div className="flex items-center gap-2 text-xs text-[#64748b]">
          <Clock size={11} />
          <span>
            Last run:{" "}
            {new Date(pulseConfig.lastRunAt).toLocaleString()}
          </span>
        </div>
      )}

      {/* Actions */}
      <div className="flex items-center gap-3">
        <button
          onClick={handleSave}
          disabled={saving}
          className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-sm font-medium transition-colors"
        >
          <Save size={14} />
          {saving ? "Saving..." : saved ? "Saved" : "Save Pulse"}
        </button>

        {pulseConfig?.taskId && (
          <button
            onClick={async () => {
              if (!pulseConfig?.taskId) return;
              setTriggering(true);
              try {
                await tasksApi.trigger(pulseConfig.taskId);
                queryClient.invalidateQueries({ queryKey: ["pulse-config", agentId] });
              } catch (err) {
                console.error("Failed to trigger pulse:", err);
              }
              setTriggering(false);
            }}
            disabled={triggering}
            className="flex items-center gap-1.5 px-3 py-2 rounded-lg border border-[#2a2d3e] text-[#94a3b8] hover:text-white hover:border-[#4a4d6e] disabled:opacity-50 text-xs transition-colors"
          >
            <Play size={12} />
            {triggering ? "Running..." : "Run Now"}
          </button>
        )}

        {pulseConfig?.sessionId && (
          <button
            onClick={() => navigate("chat")}
            className="flex items-center gap-1.5 px-3 py-2 rounded-lg border border-[#2a2d3e] text-[#94a3b8] hover:text-white hover:border-[#4a4d6e] text-xs transition-colors"
          >
            <ExternalLink size={12} />
            View Pulse Log
          </button>
        )}
      </div>
    </section>
  );
}

// ─── Other Agent Schedules ──────────────────────────────────────────────────

function AgentSchedulesList({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();

  const { data: tasks = [] } = useQuery<Task[]>({
    queryKey: ["tasks"],
    queryFn: () => invoke("list_tasks"),
  });

  const { data: allSchedules = [] } = useQuery<Schedule[]>({
    queryKey: ["schedules"],
    queryFn: schedulesApi.list,
    refetchInterval: 10_000,
  });

  // Filter: tasks for this agent, then schedules for those tasks, excluding pulse
  const agentTaskIds = new Set(
    tasks
      .filter((t) => t.agentId === agentId && !t.tags.includes("pulse"))
      .map((t) => t.id)
  );

  const agentSchedules = allSchedules.filter((s) => agentTaskIds.has(s.taskId));

  async function handleToggle(schedule: Schedule) {
    await schedulesApi.toggle(schedule.id, !schedule.enabled);
    queryClient.invalidateQueries({ queryKey: ["schedules"] });
  }

  async function handleDelete(schedule: Schedule) {
    await schedulesApi.delete(schedule.id);
    queryClient.invalidateQueries({ queryKey: ["schedules"] });
  }

  function getTaskName(taskId: string): string {
    return tasks.find((t) => t.id === taskId)?.name ?? "Unknown Task";
  }

  return (
    <section className="space-y-3">
      <h4 className="text-sm font-semibold text-white">Task Schedules</h4>

      {agentSchedules.length === 0 ? (
        <p className="text-xs text-[#64748b]">
          No schedules for this agent's tasks. Create one from the Schedules screen.
        </p>
      ) : (
        <div className="space-y-2">
          {agentSchedules.map((schedule) => {
            const config = schedule.config as RecurringConfig | null;
            const description = config ? humanSchedule(config) : schedule.kind;

            return (
              <div
                key={schedule.id}
                className="flex items-center gap-3 px-4 py-3 rounded-lg border border-[#2a2d3e] bg-[#1a1d27]"
              >
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-white truncate">
                    {getTaskName(schedule.taskId)}
                  </p>
                  <p className="text-xs text-[#64748b]">{description}</p>
                  {schedule.nextRunAt && (
                    <p className="text-[10px] text-[#4a4d6e] mt-0.5">
                      Next: {new Date(schedule.nextRunAt).toLocaleString()}
                    </p>
                  )}
                </div>

                <Switch.Root
                  checked={schedule.enabled}
                  onCheckedChange={() => handleToggle(schedule)}
                  className="w-9 h-5 rounded-full bg-[#2a2d3e] data-[state=checked]:bg-emerald-500 transition-colors outline-none shrink-0"
                >
                  <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
                </Switch.Root>

                <button
                  onClick={() => handleDelete(schedule)}
                  className="p-1.5 rounded text-[#64748b] hover:text-red-400 hover:bg-red-500/10 transition-colors shrink-0"
                >
                  <Trash2 size={13} />
                </button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
