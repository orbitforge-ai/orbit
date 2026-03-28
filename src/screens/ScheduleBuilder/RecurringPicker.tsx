import { useEffect, useState } from "react";
import { schedulesApi } from "../../api/schedules";
import { DAY_NAMES, humanSchedule } from "../../lib/humanSchedule";
import { RecurringConfig } from "../../types";

interface RecurringPickerProps {
  value: RecurringConfig;
  onChange: (cfg: RecurringConfig) => void;
}

const UNIT_OPTIONS = [
  { value: "minutes", label: "Minutes" },
  { value: "hours", label: "Hours" },
  { value: "days", label: "Days" },
  { value: "weeks", label: "Weeks" },
  { value: "months", label: "Months" },
] as const;

export function RecurringPicker({ value, onChange }: RecurringPickerProps) {
  const [nextRuns, setNextRuns] = useState<string[]>([]);

  useEffect(() => {
    schedulesApi
      .previewNextRuns(value, 5)
      .then(setNextRuns)
      .catch(() => setNextRuns([]));
  }, [value]);

  function update(partial: Partial<RecurringConfig>) {
    onChange({ ...value, ...partial });
  }

  const showTimePicker = ["days", "weeks", "months"].includes(value.intervalUnit);
  const showDayPicker = value.intervalUnit === "weeks";

  return (
    <div className="space-y-4">
      <div>
        <label className="block text-xs font-medium text-[#64748b] mb-1.5">
          Frequency
        </label>
        <div className="flex gap-2">
          <input
            type="number"
            min={1}
            max={value.intervalUnit === "minutes" ? 59 : value.intervalUnit === "hours" ? 23 : 31}
            value={value.intervalValue}
            onChange={(e) => update({ intervalValue: Number(e.target.value) || 1 })}
            className="w-20 px-3 py-2 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] text-white text-sm text-center focus:outline-none focus:border-[#6366f1]"
          />
          <select
            value={value.intervalUnit}
            onChange={(e) =>
              update({ intervalUnit: e.target.value as RecurringConfig["intervalUnit"] })
            }
            className="flex-1 px-3 py-2 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]"
          >
            {UNIT_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </div>
      </div>

      {showDayPicker && (
        <div>
          <label className="block text-xs font-medium text-[#64748b] mb-1.5">
            Days of week
          </label>
          <div className="flex gap-1.5">
            {DAY_NAMES.map((name, i) => {
              const selected = value.daysOfWeek?.includes(i) ?? false;
              return (
                <button
                  key={name}
                  type="button"
                  onClick={() => {
                    const current = value.daysOfWeek ?? [];
                    const next = selected
                      ? current.filter((d) => d !== i)
                      : [...current, i].sort();
                    update({ daysOfWeek: next });
                  }}
                  className={`w-8 h-8 rounded-full text-xs font-medium transition-colors ${
                    selected
                      ? "bg-[#6366f1] text-white"
                      : "bg-[#1a1d27] border border-[#2a2d3e] text-[#64748b] hover:border-[#6366f1] hover:text-white"
                  }`}
                >
                  {name[0]}
                </button>
              );
            })}
          </div>
        </div>
      )}

      {showTimePicker && (
        <div>
          <label className="block text-xs font-medium text-[#64748b] mb-1.5">
            Time of day
          </label>
          <div className="flex gap-2 items-center">
            <input
              type="number"
              min={0}
              max={23}
              value={value.timeOfDay?.hour ?? 9}
              onChange={(e) =>
                update({
                  timeOfDay: {
                    hour: Number(e.target.value),
                    minute: value.timeOfDay?.minute ?? 0,
                  },
                })
              }
              className="w-16 px-2 py-2 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] text-white text-sm text-center focus:outline-none focus:border-[#6366f1]"
            />
            <span className="text-[#64748b] font-medium">:</span>
            <input
              type="number"
              min={0}
              max={59}
              value={value.timeOfDay?.minute ?? 0}
              onChange={(e) =>
                update({
                  timeOfDay: {
                    hour: value.timeOfDay?.hour ?? 9,
                    minute: Number(e.target.value),
                  },
                })
              }
              className="w-16 px-2 py-2 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] text-white text-sm text-center focus:outline-none focus:border-[#6366f1]"
            />
            <span className="text-xs text-[#64748b]">local time</span>
          </div>
        </div>
      )}

      {/* Human-readable summary */}
      <div className="px-3 py-2 rounded-lg bg-[#6366f1]/10 border border-[#6366f1]/30">
        <p className="text-sm text-[#818cf8] font-medium">{humanSchedule(value)}</p>
      </div>

      {/* Next 5 runs preview */}
      {nextRuns.length > 0 && (
        <div>
          <p className="text-xs font-medium text-[#64748b] mb-1.5">
            Next {nextRuns.length} runs
          </p>
          <ul className="space-y-1">
            {nextRuns.map((iso, i) => (
              <li key={i} className="text-xs text-[#94a3b8]">
                {new Date(iso).toLocaleString()}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
