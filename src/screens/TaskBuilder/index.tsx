import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Terminal, Globe, FileCode, Bot, ChevronRight, ChevronLeft, Check } from "lucide-react";
import { tasksApi } from "../../api/tasks";
import { schedulesApi } from "../../api/schedules";
import { useUiStore } from "../../store/uiStore";
import { RecurringPicker } from "../ScheduleBuilder/RecurringPicker";
import { humanSchedule } from "../../lib/humanSchedule";
import { CreateTask, RecurringConfig, ShellCommandConfig } from "../../types";

const STEPS = ["What", "When", "Review"] as const;
type Step = (typeof STEPS)[number];

const KIND_OPTIONS = [
  {
    id: "shell_command" as const,
    label: "Shell Command",
    description: "Run a bash/sh command or script",
    icon: Terminal,
  },
  {
    id: "http_request" as const,
    label: "HTTP Request",
    description: "Call a URL or webhook",
    icon: Globe,
    disabled: true,
    badge: "M2",
  },
  {
    id: "script_file" as const,
    label: "Script File",
    description: "Execute a file on disk",
    icon: FileCode,
    disabled: true,
    badge: "M2",
  },
  {
    id: "agent_step" as const,
    label: "Agent Step",
    description: "Delegate to another agent",
    icon: Bot,
    disabled: true,
    badge: "M2",
  },
];

export function TaskBuilder() {
  const { navigate } = useUiStore();
  const queryClient = useQueryClient();

  const [step, setStep] = useState<Step>("What");
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [kind] = useState<"shell_command">("shell_command");
  const [scheduleEnabled, setScheduleEnabled] = useState(false);
  const [scheduleConfig, setScheduleConfig] = useState<RecurringConfig>({
    intervalUnit: "hours",
    intervalValue: 1,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    missedRunPolicy: "skip",
  });
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const stepIndex = STEPS.indexOf(step);
  const canProceed =
    step === "What"
      ? name.trim().length > 0 && command.trim().length > 0
      : step === "When"
      ? true
      : true;

  async function handleCreate() {
    setCreating(true);
    setError(null);
    try {
      const config: ShellCommandConfig = { command };
      const payload: CreateTask = { name, kind, config };
      const task = await tasksApi.create(payload);

      if (scheduleEnabled) {
        await schedulesApi.create({
          taskId: task.id,
          kind: "recurring",
          config: scheduleConfig,
        });
      }

      queryClient.invalidateQueries({ queryKey: ["tasks"] });
      queryClient.invalidateQueries({ queryKey: ["schedules"] });
      navigate("dashboard");
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  }

  return (
    <div className="flex flex-col h-full max-w-2xl mx-auto p-6">
      {/* Header */}
      <div className="mb-8">
        <h2 className="text-xl font-semibold text-white">New Task</h2>
        {/* Step indicator */}
        <div className="flex items-center gap-2 mt-4">
          {STEPS.map((s, i) => (
            <div key={s} className="flex items-center gap-2">
              <div
                className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-semibold ${
                  i < stepIndex
                    ? "bg-green-500 text-white"
                    : i === stepIndex
                    ? "bg-[#6366f1] text-white"
                    : "bg-[#2a2d3e] text-[#64748b]"
                }`}
              >
                {i < stepIndex ? <Check size={12} /> : i + 1}
              </div>
              <span
                className={`text-sm ${
                  i === stepIndex ? "text-white font-medium" : "text-[#64748b]"
                }`}
              >
                {s}
              </span>
              {i < STEPS.length - 1 && (
                <div className="w-8 h-px bg-[#2a2d3e] mx-1" />
              )}
            </div>
          ))}
        </div>
      </div>

      {/* Step content */}
      <div className="flex-1 overflow-y-auto">
        {step === "What" && (
          <div className="space-y-5">
            {/* Task name */}
            <div>
              <label className="block text-sm font-medium text-[#94a3b8] mb-1.5">
                Task name
              </label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Daily database backup"
                className="w-full px-4 py-2.5 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] text-white text-sm placeholder-[#4a5568] focus:outline-none focus:border-[#6366f1]"
              />
            </div>

            {/* Task kind */}
            <div>
              <label className="block text-sm font-medium text-[#94a3b8] mb-2">
                What should it do?
              </label>
              <div className="grid grid-cols-2 gap-2">
                {KIND_OPTIONS.map(({ id, label, description, icon: Icon, disabled, badge }) => (
                  <div
                    key={id}
                    className={`relative flex items-start gap-3 px-4 py-3 rounded-xl border transition-colors ${
                      disabled
                        ? "border-[#2a2d3e] bg-[#13151e] opacity-40 cursor-not-allowed"
                        : id === kind
                        ? "border-[#6366f1] bg-[#6366f1]/10 cursor-pointer"
                        : "border-[#2a2d3e] bg-[#1a1d27] cursor-pointer hover:border-[#4a4d6e]"
                    }`}
                  >
                    <Icon size={18} className={id === kind ? "text-[#818cf8]" : "text-[#64748b]"} />
                    <div>
                      <p className="text-sm font-medium text-white">{label}</p>
                      <p className="text-xs text-[#64748b]">{description}</p>
                    </div>
                    {badge && (
                      <span className="absolute top-2 right-2 text-[10px] px-1.5 py-0.5 rounded bg-[#2a2d3e] text-[#64748b]">
                        {badge}
                      </span>
                    )}
                  </div>
                ))}
              </div>
            </div>

            {/* Command */}
            <div>
              <label className="block text-sm font-medium text-[#94a3b8] mb-1.5">
                Command
              </label>
              <textarea
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                rows={6}
                placeholder={"#!/bin/bash\necho 'Hello from Orbit!'"}
                className="w-full px-4 py-3 rounded-lg bg-[#0a0c12] border border-[#2a2d3e] text-green-400 text-sm font-mono placeholder-[#2a2d3e] focus:outline-none focus:border-[#6366f1] resize-none"
              />
            </div>
          </div>
        )}

        {step === "When" && (
          <div className="space-y-5">
            <div className="flex items-center gap-3 p-4 rounded-xl border border-[#2a2d3e] bg-[#1a1d27]">
              <input
                type="checkbox"
                id="schedule-enabled"
                checked={scheduleEnabled}
                onChange={(e) => setScheduleEnabled(e.target.checked)}
                className="w-4 h-4 accent-[#6366f1]"
              />
              <label htmlFor="schedule-enabled" className="text-sm text-white cursor-pointer">
                Run on a schedule
              </label>
            </div>

            {scheduleEnabled && (
              <RecurringPicker value={scheduleConfig} onChange={setScheduleConfig} />
            )}

            {!scheduleEnabled && (
              <div className="text-center py-8 text-[#64748b] text-sm">
                No schedule — run manually from the Dashboard
              </div>
            )}
          </div>
        )}

        {step === "Review" && (
          <div className="space-y-4">
            <div className="rounded-xl border border-[#2a2d3e] bg-[#1a1d27] p-5">
              <h3 className="text-sm font-semibold text-white mb-4">Summary</h3>
              <dl className="space-y-3">
                <Row label="Name" value={name} />
                <Row label="Type" value="Shell command" />
                <Row label="Command" value={command} mono />
                <Row
                  label="Schedule"
                  value={
                    scheduleEnabled
                      ? humanSchedule(scheduleConfig)
                      : "Manual only"
                  }
                />
              </dl>
            </div>

            {error && (
              <div className="px-4 py-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">
                {error}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Navigation */}
      <div className="flex items-center justify-between pt-6 border-t border-[#2a2d3e] mt-6">
        {stepIndex > 0 ? (
          <button
            onClick={() => setStep(STEPS[stepIndex - 1])}
            className="flex items-center gap-2 px-4 py-2 rounded-lg text-[#64748b] hover:text-white hover:bg-[#2a2d3e] text-sm transition-colors"
          >
            <ChevronLeft size={14} />
            Back
          </button>
        ) : (
          <button
            onClick={() => navigate("dashboard")}
            className="px-4 py-2 rounded-lg text-[#64748b] hover:text-white text-sm transition-colors"
          >
            Cancel
          </button>
        )}

        {stepIndex < STEPS.length - 1 ? (
          <button
            disabled={!canProceed}
            onClick={() => setStep(STEPS[stepIndex + 1])}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
          >
            Continue
            <ChevronRight size={14} />
          </button>
        ) : (
          <button
            disabled={creating}
            onClick={handleCreate}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-sm font-medium transition-colors"
          >
            {creating ? "Creating…" : "Create Task"}
            <Check size={14} />
          </button>
        )}
      </div>
    </div>
  );
}

function Row({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex gap-3">
      <dt className="w-24 flex-shrink-0 text-xs text-[#64748b] pt-0.5">{label}</dt>
      <dd
        className={`flex-1 text-sm text-white break-all ${mono ? "font-mono text-green-400" : ""}`}
      >
        {value}
      </dd>
    </div>
  );
}
