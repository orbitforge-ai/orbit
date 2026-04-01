import { useEffect, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import { schedulesApi } from '../../api/schedules';
import { DAY_NAMES, humanSchedule } from '../../lib/humanSchedule';
import { parseScheduleInput } from '../../lib/parseScheduleInput';
import { RecurringConfig } from '../../types';

interface RecurringPickerProps {
  value: RecurringConfig;
  onChange: (cfg: RecurringConfig) => void;
}

type InputMode = 'text' | 'manual';

const UNIT_OPTIONS = [
  { value: 'minutes', label: 'Minutes' },
  { value: 'hours', label: 'Hours' },
  { value: 'days', label: 'Days' },
  { value: 'weeks', label: 'Weeks' },
  { value: 'months', label: 'Months' },
] as const;

export function RecurringPicker({ value, onChange }: RecurringPickerProps) {
  const [nextRuns, setNextRuns] = useState<string[]>([]);
  const [mode, setMode] = useState<InputMode>(value.expression ? 'text' : 'text');
  const [textInput, setTextInput] = useState(value.expression ?? '');
  const [parseError, setParseError] = useState(false);

  useEffect(() => {
    schedulesApi
      .previewNextRuns(value, 5)
      .then(setNextRuns)
      .catch(() => setNextRuns([]));
  }, [value]);

  function update(partial: Partial<RecurringConfig>) {
    onChange({ ...value, ...partial });
  }

  function handleTextChange(raw: string) {
    setTextInput(raw);
    const result = parseScheduleInput(raw, value.timezone);
    if (result) {
      setParseError(false);
      onChange({ ...result, expression: raw });
    } else {
      setParseError(raw.trim().length > 0);
    }
  }

  const showTimePicker = ['days', 'weeks', 'months'].includes(value.intervalUnit);
  const showDayPicker = value.intervalUnit === 'weeks';

  return (
    <div className="space-y-4">
      {/* Mode toggle */}
      <div className="flex rounded-lg bg-surface border border-edge p-0.5">
        <button
          type="button"
          onClick={() => setMode('text')}
          className={`flex-1 px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
            mode === 'text' ? 'bg-accent text-white' : 'text-muted hover:text-white'
          }`}
        >
          Natural Language / Cron
        </button>
        <button
          type="button"
          onClick={() => setMode('manual')}
          className={`flex-1 px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
            mode === 'manual' ? 'bg-accent text-white' : 'text-muted hover:text-white'
          }`}
        >
          Manual
        </button>
      </div>

      {mode === 'text' && (
        <div>
          <label className="block text-xs font-medium text-muted mb-1.5">Schedule</label>
          <input
            type="text"
            value={textInput}
            onChange={(e) => handleTextChange(e.target.value)}
            placeholder="e.g. every weekday at 9am, daily at 5pm, */30 * * * *"
            className={`w-full px-3 py-2 rounded-lg bg-surface border text-white text-sm focus:outline-none transition-colors ${
              parseError
                ? 'border-red-500/60 focus:border-red-500'
                : 'border-edge focus:border-accent'
            }`}
          />
          {parseError && (
            <p className="mt-1 text-xs text-red-400">
              Couldn't parse that schedule. Try "every 2 hours", "daily at 9am", or a cron like "0 9
              * * 1-5".
            </p>
          )}
          {!parseError && textInput.trim().length === 0 && (
            <p className="mt-1.5 text-xs text-muted leading-relaxed">
              Natural language: "every weekday at 9am", "every 30 minutes", "weekly on monday"
              <br />
              Cron: "0 9 * * 1-5", "*/15 * * * *", "0 18 * * 0,6"
            </p>
          )}
        </div>
      )}

      {mode === 'manual' && (
        <div>
          <label className="block text-xs font-medium text-muted mb-1.5">Frequency</label>
          <div className="flex gap-2">
            <input
              type="number"
              min={1}
              max={value.intervalUnit === 'minutes' ? 59 : value.intervalUnit === 'hours' ? 23 : 31}
              value={value.intervalValue}
              onChange={(e) => update({ intervalValue: Number(e.target.value) || 1 })}
              className="w-20 px-3 py-2 rounded-lg bg-surface border border-edge text-white text-sm text-center focus:outline-none focus:border-accent"
            />
            <Select.Root
              value={value.intervalUnit}
              onValueChange={(v) => update({ intervalUnit: v as RecurringConfig['intervalUnit'] })}
            >
              <Select.Trigger className="flex items-center justify-between flex-1 px-3 py-2 rounded-lg bg-surface border border-edge text-white text-sm focus:outline-none focus:border-accent">
                <Select.Value />
                <Select.Icon>
                  <ChevronDown size={14} className="text-muted" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    {UNIT_OPTIONS.map((o) => (
                      <Select.Item
                        key={o.value}
                        value={o.value}
                        className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                      >
                        <Select.ItemText>{o.label}</Select.ItemText>
                      </Select.Item>
                    ))}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          </div>
        </div>
      )}

      {mode === 'manual' && showDayPicker && (
        <div>
          <label className="block text-xs font-medium text-muted mb-1.5">Days of week</label>
          <div className="flex gap-1.5">
            {DAY_NAMES.map((name, i) => {
              const selected = value.daysOfWeek?.includes(i) ?? false;
              return (
                <button
                  key={name}
                  type="button"
                  onClick={() => {
                    const current = value.daysOfWeek ?? [];
                    const next = selected ? current.filter((d) => d !== i) : [...current, i].sort();
                    update({ daysOfWeek: next });
                  }}
                  className={`w-8 h-8 rounded-full text-xs font-medium transition-colors ${
                    selected
                      ? 'bg-accent text-white'
                      : 'bg-surface border border-edge text-muted hover:border-accent hover:text-white'
                  }`}
                >
                  {name[0]}
                </button>
              );
            })}
          </div>
        </div>
      )}

      {mode === 'manual' && showTimePicker && (
        <div>
          <label className="block text-xs font-medium text-muted mb-1.5">Time of day</label>
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
              className="w-16 px-2 py-2 rounded-lg bg-surface border border-edge text-white text-sm text-center focus:outline-none focus:border-accent"
            />
            <span className="text-muted font-medium">:</span>
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
              className="w-16 px-2 py-2 rounded-lg bg-surface border border-edge text-white text-sm text-center focus:outline-none focus:border-accent"
            />
            <span className="text-xs text-muted">local time</span>
          </div>
        </div>
      )}

      {/* Human-readable summary */}
      <div className="px-3 py-2 rounded-lg bg-accent/10 border border-accent/30">
        <p className="text-sm text-accent-hover font-medium">{humanSchedule(value)}</p>
      </div>

      {/* Next 5 runs preview */}
      {nextRuns.length > 0 && (
        <div>
          <p className="text-xs font-medium text-muted mb-1.5">Next {nextRuns.length} runs</p>
          <ul className="space-y-1">
            {nextRuns.map((iso, i) => (
              <li key={i} className="text-xs text-secondary">
                {new Date(iso).toLocaleString()}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
