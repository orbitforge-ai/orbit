import { useState, useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronLeft, Check, Bot, Trash2, Plus, Calendar, Clock } from 'lucide-react';
import * as Slider from '@radix-ui/react-slider';
import * as Switch from '@radix-ui/react-switch';
import { tasksApi } from '../../api/tasks';
import { schedulesApi } from '../../api/schedules';
import { agentsApi } from '../../api/agents';
import { useUiStore } from '../../store/uiStore';
import { CollapsibleSection } from '../../components/CollapsibleSection';
import { Field } from '../../components/TaskFormFields';
import { TaskConfigForm, defaultConfigState, type TaskConfigState } from '../../components/TaskConfigForm';
import {
  KIND_OPTIONS,
  CONCURRENCY_OPTIONS,
  inputCls,
} from '../../lib/taskConstants';
import { humanSchedule } from '../../lib/humanSchedule';
import { RecurringPicker } from '../ScheduleBuilder/RecurringPicker';
import type {
  CreateTask,
  ShellCommandConfig,
  ScriptFileConfig,
  HttpRequestConfig,
  AgentStepConfig,
  AgentLoopConfig,
  RecurringConfig,
  OneShotConfig,
  Schedule,
} from '../../types';

type ScheduleKind = 'recurring' | 'one_shot';

export function TaskEdit() {
  const { editingTaskId, navigate } = useUiStore();
  const queryClient = useQueryClient();

  // ── Data loading ──
  const { data: task, isLoading } = useQuery({
    queryKey: ['tasks', editingTaskId],
    queryFn: () => tasksApi.get(editingTaskId!),
    enabled: !!editingTaskId,
  });

  const { data: agents = [] } = useQuery({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  const { data: schedules = [], refetch: refetchSchedules } = useQuery({
    queryKey: ['schedules', editingTaskId],
    queryFn: () => schedulesApi.listForTask(editingTaskId!),
    enabled: !!editingTaskId,
  });

  // ── Form state ──
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [configState, setConfigState] = useState<TaskConfigState>({ ...defaultConfigState });
  const [agentId, setAgentId] = useState('default');
  const [concurrencyPolicy, setConcurrencyPolicy] =
    useState<CreateTask['concurrencyPolicy']>('allow');
  const [maxDurationMinutes, setMaxDurationMinutes] = useState(60);
  const [maxRetries, setMaxRetries] = useState(0);
  const [retryDelaySecs, setRetryDelaySecs] = useState(60);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // ── Schedule add form ──
  const [showAddSchedule, setShowAddSchedule] = useState(false);
  const [newScheduleKind, setNewScheduleKind] = useState<ScheduleKind>('recurring');
  const [newRecurringConfig, setNewRecurringConfig] = useState<RecurringConfig>({
    intervalUnit: 'hours',
    intervalValue: 1,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    missedRunPolicy: 'skip',
  });
  const [newOneShotDate, setNewOneShotDate] = useState('');
  const [newOneShotTime, setNewOneShotTime] = useState('09:00');
  const [addingSchedule, setAddingSchedule] = useState(false);

  // ── Hydrate form from loaded task ──
  useEffect(() => {
    if (!task) return;
    setName(task.name);
    setDescription(task.description ?? '');
    setAgentId(task.agentId ?? 'default');
    setConcurrencyPolicy((task.concurrencyPolicy as CreateTask['concurrencyPolicy']) ?? 'allow');
    setMaxDurationMinutes(Math.round(task.maxDurationSeconds / 60));
    setMaxRetries(task.maxRetries);
    setRetryDelaySecs(task.retryDelaySeconds);

    // Hydrate kind-specific config
    const s: TaskConfigState = { ...defaultConfigState };
    switch (task.kind) {
      case 'shell_command': {
        const cfg = task.config as ShellCommandConfig;
        s.command = cfg.command ?? '';
        s.workingDir = cfg.workingDirectory ?? '';
        break;
      }
      case 'script_file': {
        const cfg = task.config as ScriptFileConfig;
        s.scriptPath = cfg.scriptPath ?? '';
        s.interpreter = cfg.interpreter ?? '/bin/sh';
        s.workingDir = cfg.workingDirectory ?? '';
        break;
      }
      case 'http_request': {
        const cfg = task.config as HttpRequestConfig;
        s.httpUrl = cfg.url ?? '';
        s.httpMethod = cfg.method ?? 'GET';
        s.httpHeaders = cfg.headers
          ? Object.entries(cfg.headers).map(([k, v]) => ({ k, v }))
          : [];
        s.httpBody = cfg.body ?? '';
        s.httpExpectedCodes = cfg.expectedStatusCodes
          ? cfg.expectedStatusCodes.join(', ')
          : '';
        break;
      }
      case 'agent_step': {
        const cfg = task.config as AgentStepConfig;
        s.prompt = cfg.prompt ?? '';
        break;
      }
      case 'agent_loop': {
        const cfg = task.config as AgentLoopConfig;
        s.goal = cfg.goal ?? '';
        s.loopMaxIterations = cfg.maxIterations ?? 25;
        s.loopMaxTokens = cfg.maxTotalTokens ?? 200000;
        break;
      }
    }
    setConfigState(s);
  }, [task]);

  function handleConfigChange<K extends keyof TaskConfigState>(key: K, value: TaskConfigState[K]) {
    setConfigState((prev) => ({ ...prev, [key]: value }));
  }

  // ── Build config from form state ──
  function buildConfig() {
    if (!task) return {};
    const cs = configState;
    switch (task.kind) {
      case 'shell_command': {
        const cfg: ShellCommandConfig = { command: cs.command };
        if (cs.workingDir.trim()) cfg.workingDirectory = cs.workingDir.trim();
        return cfg as ShellCommandConfig;
      }
      case 'script_file': {
        const cfg: ScriptFileConfig = { scriptPath: cs.scriptPath };
        if (cs.interpreter.trim()) cfg.interpreter = cs.interpreter.trim();
        if (cs.workingDir.trim()) cfg.workingDirectory = cs.workingDir.trim();
        return cfg as ScriptFileConfig;
      }
      case 'http_request': {
        const cfg: HttpRequestConfig = { url: cs.httpUrl, method: cs.httpMethod };
        if (cs.httpHeaders.length > 0) {
          cfg.headers = Object.fromEntries(
            cs.httpHeaders.filter((h) => h.k).map((h) => [h.k, h.v])
          );
        }
        if (cs.httpBody.trim()) cfg.body = cs.httpBody.trim();
        if (cs.httpExpectedCodes.trim()) {
          cfg.expectedStatusCodes = cs.httpExpectedCodes
            .split(',')
            .map((s) => parseInt(s.trim(), 10))
            .filter(Boolean);
        }
        return cfg as HttpRequestConfig;
      }
      case 'agent_step':
        return { prompt: cs.prompt } as AgentStepConfig;
      case 'agent_loop': {
        const cfg: AgentLoopConfig = { goal: cs.goal };
        if (cs.loopMaxIterations !== 25) cfg.maxIterations = cs.loopMaxIterations;
        if (cs.loopMaxTokens !== 200000) cfg.maxTotalTokens = cs.loopMaxTokens;
        return cfg as AgentLoopConfig;
      }
      default:
        return {};
    }
  }

  // ── Validation ──
  function canSave(): boolean {
    if (!name.trim()) return false;
    if (!task) return false;
    switch (task.kind) {
      case 'shell_command': return configState.command.trim().length > 0;
      case 'script_file': return configState.scriptPath.trim().length > 0;
      case 'http_request': return configState.httpUrl.trim().length > 0;
      case 'agent_step': return configState.prompt.trim().length > 0;
      case 'agent_loop': return configState.goal.trim().length > 0;
      default: return true;
    }
  }

  // ── Save ──
  async function handleSave() {
    if (!editingTaskId) return;
    setSaving(true);
    setError(null);
    try {
      await tasksApi.update(editingTaskId, {
        name,
        description: description.trim() || null,
        config: buildConfig(),
        agentId,
        concurrencyPolicy,
        maxDurationSeconds: maxDurationMinutes * 60,
        maxRetries,
        retryDelaySeconds: retryDelaySecs,
      });
      queryClient.invalidateQueries({ queryKey: ['tasks'] });
      navigate('tasks');
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  // ── Enabled toggle (saves immediately) ──
  async function handleToggleEnabled(enabled: boolean) {
    if (!editingTaskId) return;
    await tasksApi.update(editingTaskId, { enabled });
    queryClient.invalidateQueries({ queryKey: ['tasks'] });
    queryClient.invalidateQueries({ queryKey: ['tasks', editingTaskId] });
  }

  // ── Schedule actions ──
  async function handleToggleSchedule(id: string, enabled: boolean) {
    await schedulesApi.toggle(id, enabled);
    refetchSchedules();
  }

  async function handleDeleteSchedule(id: string) {
    await schedulesApi.delete(id);
    refetchSchedules();
    queryClient.invalidateQueries({ queryKey: ['schedules'] });
  }

  async function handleAddSchedule() {
    if (!editingTaskId) return;
    setAddingSchedule(true);
    try {
      if (newScheduleKind === 'recurring') {
        await schedulesApi.create({
          taskId: editingTaskId,
          kind: 'recurring',
          config: newRecurringConfig,
        });
      } else {
        const runAt = new Date(`${newOneShotDate}T${newOneShotTime}`).toISOString();
        const config: OneShotConfig = {
          runAt,
          timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        };
        await schedulesApi.create({ taskId: editingTaskId, kind: 'one_shot', config });
      }
      refetchSchedules();
      queryClient.invalidateQueries({ queryKey: ['schedules'] });
      setShowAddSchedule(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setAddingSchedule(false);
    }
  }

  // ── Loading / not found ──
  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">Loading…</div>
    );
  }
  if (!task) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">
        Task not found.
      </div>
    );
  }

  const kindInfo = KIND_OPTIONS.find((k) => k.id === task.kind);
  const KindIcon = kindInfo?.icon;

  return (
    <div className="flex flex-col h-full max-w-2xl mx-auto p-6">
      {/* ── Header ── */}
      <div className="mb-6">
        <button
          onClick={() => navigate('tasks')}
          className="flex items-center gap-1.5 text-sm text-muted hover:text-white mb-4 transition-colors"
        >
          <ChevronLeft size={14} />
          Back to Tasks
        </button>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <h2 className="text-xl font-semibold text-white">Edit Task</h2>
            {kindInfo && (
              <span className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-accent/10 border border-accent/30 text-accent-hover text-xs font-medium">
                {KindIcon && <KindIcon size={12} />}
                {kindInfo.label}
              </span>
            )}
          </div>
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted">
              {task.enabled ? 'Enabled' : 'Disabled'}
            </span>
            <Switch.Root
              checked={task.enabled}
              onCheckedChange={handleToggleEnabled}
              className="w-9 h-5 rounded-full bg-edge data-[state=checked]:bg-accent transition-colors"
            >
              <Switch.Thumb className="block w-4 h-4 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
            </Switch.Root>
          </div>
        </div>
      </div>

      {/* ── Scrollable form ── */}
      <div className="flex-1 overflow-y-auto space-y-5 pr-1">
        {/* ── General ── */}
        <div className="space-y-4">
          <Field label="Task name">
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className={inputCls}
            />
          </Field>
          <Field label="Description">
            <input
              type="text"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="What does this task do?"
              className={inputCls}
            />
          </Field>
        </div>

        {/* ── Configuration (kind-specific) ── */}
        <div className="rounded-xl border border-edge bg-surface p-4 space-y-4">
          <h3 className="text-sm font-semibold text-white">Configuration</h3>
          <TaskConfigForm
            kind={task.kind}
            state={configState}
            onChange={handleConfigChange}
          />
        </div>

        {/* ── Execution settings ── */}
        <CollapsibleSection title="Execution Settings" description="Agent, concurrency, timeout, retries">
          <div className="space-y-5">
            <Field label="Agent">
              <div className="space-y-2">
                {agents.map((agent) => (
                  <button
                    key={agent.id}
                    type="button"
                    onClick={() => setAgentId(agent.id)}
                    className={`w-full flex items-center gap-3 px-4 py-3 rounded-xl border transition-colors text-left ${
                      agent.id === agentId
                        ? 'border-accent bg-accent/10'
                        : 'border-edge bg-background hover:border-edge-hover'
                    }`}
                  >
                    <Bot
                      size={16}
                      className={agent.id === agentId ? 'text-accent-hover' : 'text-muted'}
                    />
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium text-white">{agent.name}</p>
                      <p className="text-xs text-muted">Max {agent.maxConcurrentRuns} concurrent</p>
                    </div>
                    {agent.id === agentId && <Check size={14} className="text-accent" />}
                  </button>
                ))}
              </div>
            </Field>

            <Field label="Concurrency policy">
              <div className="space-y-2">
                {CONCURRENCY_OPTIONS.map((opt) => (
                  <button
                    key={opt.value}
                    type="button"
                    onClick={() =>
                      setConcurrencyPolicy(opt.value as CreateTask['concurrencyPolicy'])
                    }
                    className={`w-full flex items-center gap-3 px-4 py-2.5 rounded-lg border transition-colors text-left ${
                      opt.value === concurrencyPolicy
                        ? 'border-accent bg-accent/10'
                        : 'border-edge bg-background hover:border-edge-hover'
                    }`}
                  >
                    <div
                      className={`w-3.5 h-3.5 rounded-full border-2 flex-shrink-0 ${
                        opt.value === concurrencyPolicy
                          ? 'border-accent bg-accent'
                          : 'border-edge-hover'
                      }`}
                    />
                    <div>
                      <p className="text-sm font-medium text-white">{opt.label}</p>
                      <p className="text-xs text-muted">{opt.hint}</p>
                    </div>
                  </button>
                ))}
              </div>
            </Field>

            <Field label={`Timeout: ${maxDurationMinutes} min`}>
              <Slider.Root
                min={1}
                max={360}
                step={1}
                value={[maxDurationMinutes]}
                onValueChange={([v]) => setMaxDurationMinutes(v)}
                className="relative flex items-center w-full h-5 select-none touch-none"
              >
                <Slider.Track className="relative grow h-1 rounded-full bg-edge">
                  <Slider.Range className="absolute h-full rounded-full bg-accent" />
                </Slider.Track>
                <Slider.Thumb className="block w-4 h-4 rounded-full bg-white shadow-md border-2 border-accent focus:outline-none focus:ring-2 focus:ring-accent/40" />
              </Slider.Root>
              <div className="flex justify-between text-xs text-muted mt-1">
                <span>1 min</span>
                <span>1 hr</span>
                <span>6 hrs</span>
              </div>
            </Field>

            <div className="grid grid-cols-2 gap-4">
              <Field label="Max retries">
                <input
                  type="number"
                  min={0}
                  max={10}
                  value={maxRetries}
                  onChange={(e) => setMaxRetries(Number(e.target.value))}
                  className={inputCls}
                />
              </Field>
              <Field label="Retry delay (sec)">
                <input
                  type="number"
                  min={10}
                  value={retryDelaySecs}
                  onChange={(e) => setRetryDelaySecs(Number(e.target.value))}
                  className={inputCls}
                />
              </Field>
            </div>
          </div>
        </CollapsibleSection>

        {/* ── Schedules ── */}
        <CollapsibleSection
          title="Schedules"
          description="When this task runs automatically"
          badge={
            schedules.length > 0 ? (
              <span className="px-2 py-0.5 rounded-full bg-accent/20 text-accent-hover text-xs font-medium">
                {schedules.length}
              </span>
            ) : undefined
          }
        >
          <div className="space-y-3">
            {schedules.length === 0 && !showAddSchedule && (
              <p className="text-sm text-muted py-2">No schedules. Task runs manually only.</p>
            )}

            {schedules.map((sched) => (
              <ScheduleRow
                key={sched.id}
                schedule={sched}
                onToggle={(enabled) => handleToggleSchedule(sched.id, enabled)}
                onDelete={() => handleDeleteSchedule(sched.id)}
              />
            ))}

            {showAddSchedule ? (
              <div className="rounded-lg border border-edge bg-background p-4 space-y-4">
                <div className="flex gap-2">
                  {(['recurring', 'one_shot'] as const).map((sk) => (
                    <button
                      key={sk}
                      type="button"
                      onClick={() => setNewScheduleKind(sk)}
                      className={`flex-1 py-2 rounded-lg border text-sm font-medium transition-colors ${
                        sk === newScheduleKind
                          ? 'border-accent bg-accent/10 text-accent-hover'
                          : 'border-edge bg-surface text-muted hover:border-edge-hover'
                      }`}
                    >
                      {sk === 'recurring' ? 'Recurring' : 'One time'}
                    </button>
                  ))}
                </div>

                {newScheduleKind === 'recurring' && (
                  <RecurringPicker value={newRecurringConfig} onChange={setNewRecurringConfig} />
                )}

                {newScheduleKind === 'one_shot' && (
                  <div className="space-y-3">
                    <Field label="Date">
                      <input
                        type="date"
                        value={newOneShotDate}
                        onChange={(e) => setNewOneShotDate(e.target.value)}
                        className={inputCls}
                      />
                    </Field>
                    <Field label="Time">
                      <input
                        type="time"
                        value={newOneShotTime}
                        onChange={(e) => setNewOneShotTime(e.target.value)}
                        className={inputCls}
                      />
                    </Field>
                  </div>
                )}

                <div className="flex items-center justify-end gap-2 pt-1">
                  <button
                    onClick={() => setShowAddSchedule(false)}
                    className="px-3 py-1.5 rounded-lg text-sm text-muted hover:text-white transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    disabled={addingSchedule || (newScheduleKind === 'one_shot' && !newOneShotDate)}
                    onClick={handleAddSchedule}
                    className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-sm font-medium transition-colors"
                  >
                    {addingSchedule ? 'Adding…' : 'Add Schedule'}
                  </button>
                </div>
              </div>
            ) : (
              <button
                onClick={() => setShowAddSchedule(true)}
                className="flex items-center gap-1.5 text-sm text-accent hover:text-accent-hover transition-colors"
              >
                <Plus size={14} /> Add schedule
              </button>
            )}
          </div>
        </CollapsibleSection>

        {error && (
          <div className="px-4 py-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">
            {error}
          </div>
        )}
      </div>

      {/* ── Footer ── */}
      <div className="flex items-center justify-end pt-6 border-t border-edge mt-6">
        <button
          disabled={!canSave() || saving}
          onClick={handleSave}
          className="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
        >
          {saving ? 'Saving…' : 'Save Changes'}
          <Check size={14} />
        </button>
      </div>
    </div>
  );
}

// ── Schedule row sub-component ──
function ScheduleRow({
  schedule,
  onToggle,
  onDelete,
}: {
  schedule: Schedule;
  onToggle: (enabled: boolean) => void;
  onDelete: () => void;
}) {
  const label =
    schedule.kind === 'recurring'
      ? humanSchedule(schedule.config as RecurringConfig)
      : schedule.kind === 'one_shot'
        ? `Once on ${new Date((schedule.config as OneShotConfig).runAt).toLocaleString()}`
        : 'Triggered';

  return (
    <div className="flex items-center gap-3 px-3 py-2.5 rounded-lg border border-edge bg-background">
      <div className="flex items-center gap-2 flex-1 min-w-0">
        {schedule.kind === 'recurring' ? (
          <Clock size={14} className="text-muted shrink-0" />
        ) : (
          <Calendar size={14} className="text-muted shrink-0" />
        )}
        <div className="flex-1 min-w-0">
          <p className="text-sm text-white truncate">{label}</p>
          {schedule.nextRunAt && (
            <p className="text-xs text-muted">
              Next: {new Date(schedule.nextRunAt).toLocaleString()}
            </p>
          )}
        </div>
      </div>
      <Switch.Root
        checked={schedule.enabled}
        onCheckedChange={onToggle}
        className="w-8 h-[18px] rounded-full bg-edge data-[state=checked]:bg-accent transition-colors shrink-0"
      >
        <Switch.Thumb className="block w-3.5 h-3.5 rounded-full bg-white shadow translate-x-0.5 data-[state=checked]:translate-x-[14px] transition-transform" />
      </Switch.Root>
      <button
        onClick={onDelete}
        className="p-1 rounded text-muted hover:text-red-400 hover:bg-red-500/10 transition-colors shrink-0"
      >
        <Trash2 size={13} />
      </button>
    </div>
  );
}
