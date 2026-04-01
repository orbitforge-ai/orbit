import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronRight, ChevronLeft, Check, Bot } from 'lucide-react';
import * as Slider from '@radix-ui/react-slider';
import { tasksApi } from '../../api/tasks';
import { schedulesApi } from '../../api/schedules';
import { agentsApi } from '../../api/agents';
import { useUiStore } from '../../store/uiStore';
import { RecurringPicker } from '../ScheduleBuilder/RecurringPicker';
import { humanSchedule } from '../../lib/humanSchedule';
import { Field, Row } from '../../components/TaskFormFields';
import { TaskConfigForm, defaultConfigState, type TaskConfigState } from '../../components/TaskConfigForm';
import {
  KIND_OPTIONS,
  CONCURRENCY_OPTIONS,
  inputCls,
  type TaskKind,
  type ScheduleKind,
} from '../../lib/taskConstants';
import {
  AgentLoopConfig,
  AgentStepConfig,
  CreateTask,
  HttpRequestConfig,
  OneShotConfig,
  RecurringConfig,
  ScriptFileConfig,
  ShellCommandConfig,
} from '../../types';

const STEPS = ['What', 'Who', 'When', 'Review'] as const;
type Step = (typeof STEPS)[number];

export function TaskBuilder() {
  const { navigate } = useUiStore();
  const queryClient = useQueryClient();

  // Step
  const [step, setStep] = useState<Step>('What');
  const stepIndex = STEPS.indexOf(step);

  // Step 1 — What
  const [kind, setKind] = useState<TaskKind>('shell_command');
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [configState, setConfigState] = useState<TaskConfigState>({ ...defaultConfigState });

  function handleConfigChange<K extends keyof TaskConfigState>(key: K, value: TaskConfigState[K]) {
    setConfigState((prev) => ({ ...prev, [key]: value }));
  }

  // Convenience aliases for buildConfig / canProceed
  const { command, workingDir, scriptPath, interpreter, prompt, goal,
    loopMaxIterations, loopMaxTokens, httpUrl, httpMethod, httpHeaders,
    httpBody, httpExpectedCodes } = configState;

  // Step 2 — Who
  const [agentId, setAgentId] = useState('default');
  const [concurrencyPolicy, setConcurrencyPolicy] =
    useState<CreateTask['concurrencyPolicy']>('allow');
  const [maxDurationMinutes, setMaxDurationMinutes] = useState(60);
  const [maxRetries, setMaxRetries] = useState(0);
  const [retryDelaySecs, setRetryDelaySecs] = useState(60);

  // Step 3 — When
  const [scheduleKind, setScheduleKind] = useState<ScheduleKind>('none');
  const [recurringConfig, setRecurringConfig] = useState<RecurringConfig>({
    intervalUnit: 'hours',
    intervalValue: 1,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    missedRunPolicy: 'skip',
  });
  const [oneShotDate, setOneShotDate] = useState('');
  const [oneShotTime, setOneShotTime] = useState('09:00');

  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { data: agents = [] } = useQuery({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  // Validation per step
  const canProceed = (): boolean => {
    if (step === 'What') {
      if (!name.trim()) return false;
      if (kind === 'shell_command') return command.trim().length > 0;
      if (kind === 'agent_step') return prompt.trim().length > 0;
      if (kind === 'script_file') return scriptPath.trim().length > 0;
      if (kind === 'http_request') return httpUrl.trim().length > 0;
      if (kind === 'agent_loop') return goal.trim().length > 0;
    }
    if (step === 'When' && scheduleKind === 'one_shot') {
      return oneShotDate.trim().length > 0;
    }
    return true;
  };

  function buildConfig() {
    if (kind === 'shell_command') {
      const cfg: ShellCommandConfig = { command };
      if (workingDir.trim()) cfg.workingDirectory = workingDir.trim();
      return cfg;
    }
    if (kind === 'script_file') {
      const cfg: ScriptFileConfig = { scriptPath };
      if (interpreter.trim()) cfg.interpreter = interpreter.trim();
      if (workingDir.trim()) cfg.workingDirectory = workingDir.trim();
      return cfg;
    }
    if (kind === 'http_request') {
      const cfg: HttpRequestConfig = { url: httpUrl, method: httpMethod };
      if (httpHeaders.length > 0) {
        cfg.headers = Object.fromEntries(httpHeaders.filter((h) => h.k).map((h) => [h.k, h.v]));
      }
      if (httpBody.trim()) cfg.body = httpBody.trim();
      if (httpExpectedCodes.trim()) {
        cfg.expectedStatusCodes = httpExpectedCodes
          .split(',')
          .map((s) => parseInt(s.trim(), 10))
          .filter(Boolean);
      }
      return cfg;
    }
    if (kind === 'agent_step') {
      const cfg: AgentStepConfig = { prompt };
      return cfg;
    }
    if (kind === 'agent_loop') {
      const cfg: AgentLoopConfig = { goal };
      if (loopMaxIterations !== 25) cfg.maxIterations = loopMaxIterations;
      if (loopMaxTokens !== 200000) cfg.maxTotalTokens = loopMaxTokens;
      return cfg;
    }
    return {};
  }

  async function handleCreate() {
    setCreating(true);
    setError(null);
    try {
      const payload: CreateTask = {
        name,
        description: description.trim() || undefined,
        kind,
        config: buildConfig(),
        agentId,
        concurrencyPolicy,
        maxDurationSeconds: maxDurationMinutes * 60,
        maxRetries,
        retryDelaySeconds: retryDelaySecs,
      };
      const task = await tasksApi.create(payload);

      if (scheduleKind === 'recurring') {
        await schedulesApi.create({ taskId: task.id, kind: 'recurring', config: recurringConfig });
      } else if (scheduleKind === 'one_shot') {
        const runAt = new Date(`${oneShotDate}T${oneShotTime}`).toISOString();
        const config: OneShotConfig = {
          runAt,
          timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        };
        await schedulesApi.create({ taskId: task.id, kind: 'one_shot', config });
      }

      queryClient.invalidateQueries({ queryKey: ['tasks'] });
      queryClient.invalidateQueries({ queryKey: ['schedules'] });
      navigate('dashboard');
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  }

  function scheduleLabel(): string {
    if (scheduleKind === 'none') return 'Manual only';
    if (scheduleKind === 'recurring') return humanSchedule(recurringConfig);
    if (scheduleKind === 'one_shot' && oneShotDate) {
      return `Once on ${new Date(`${oneShotDate}T${oneShotTime}`).toLocaleString()}`;
    }
    return 'One time (date not set)';
  }

  return (
    <div className="flex flex-col h-full max-w-2xl mx-auto p-6">
      {/* Header */}
      <div className="mb-8">
        <h2 className="text-xl font-semibold text-white">New Task</h2>
        <div className="flex items-center gap-2 mt-4">
          {STEPS.map((s, i) => (
            <div key={s} className="flex items-center gap-2">
              <div
                className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-semibold ${
                  i < stepIndex
                    ? 'bg-green-500 text-white'
                    : i === stepIndex
                      ? 'bg-accent text-white'
                      : 'bg-edge text-muted'
                }`}
              >
                {i < stepIndex ? <Check size={12} /> : i + 1}
              </div>
              <span
                className={`text-sm ${i === stepIndex ? 'text-white font-medium' : 'text-muted'}`}
              >
                {s}
              </span>
              {i < STEPS.length - 1 && <div className="w-8 h-px bg-edge mx-1" />}
            </div>
          ))}
        </div>
      </div>

      {/* Step content */}
      <div className="flex-1 overflow-y-auto space-y-5">
        {/* ── Step 1: What ── */}
        {step === 'What' && (
          <>
            <Field label="Task name">
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Daily database backup"
                className={inputCls}
              />
            </Field>

            <Field label="Description (optional)">
              <input
                type="text"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="What does this task do?"
                className={inputCls}
              />
            </Field>

            <Field label="What should it do?">
              <div className="grid grid-cols-2 gap-2">
                {KIND_OPTIONS.map(({ id, label, description: desc, icon: Icon }) => (
                  <button
                    key={id}
                    type="button"
                    onClick={() => setKind(id)}
                    className={`flex items-start gap-3 px-4 py-3 rounded-xl border transition-colors text-left ${
                      id === kind
                        ? 'border-accent bg-accent/10'
                        : 'border-edge bg-surface hover:border-edge-hover'
                    }`}
                  >
                    <Icon
                      size={18}
                      className={id === kind ? 'text-accent-hover mt-0.5' : 'text-muted mt-0.5'}
                    />
                    <div>
                      <p className="text-sm font-medium text-white">{label}</p>
                      <p className="text-xs text-muted">{desc}</p>
                    </div>
                  </button>
                ))}
              </div>
            </Field>

            {/* Config panel for the selected kind */}
            <TaskConfigForm
              kind={kind}
              state={configState}
              onChange={handleConfigChange}
            />
          </>
        )}

        {/* ── Step 2: Who ── */}
        {step === 'Who' && (
          <>
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
                        : 'border-edge bg-surface hover:border-edge-hover'
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
                        : 'border-edge bg-surface hover:border-edge-hover'
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
          </>
        )}

        {/* ── Step 3: When ── */}
        {step === 'When' && (
          <>
            <Field label="Schedule type">
              <div className="flex gap-2">
                {(['none', 'recurring', 'one_shot'] as ScheduleKind[]).map((sk) => (
                  <button
                    key={sk}
                    type="button"
                    onClick={() => setScheduleKind(sk)}
                    className={`flex-1 py-2 rounded-lg border text-sm font-medium transition-colors ${
                      sk === scheduleKind
                        ? 'border-accent bg-accent/10 text-accent-hover'
                        : 'border-edge bg-surface text-muted hover:border-edge-hover'
                    }`}
                  >
                    {sk === 'none' ? 'Manual' : sk === 'recurring' ? 'Recurring' : 'One time'}
                  </button>
                ))}
              </div>
            </Field>

            {scheduleKind === 'none' && (
              <p className="text-center py-8 text-muted text-sm">
                Run manually from the Dashboard or Tasks screen.
              </p>
            )}

            {scheduleKind === 'recurring' && (
              <RecurringPicker value={recurringConfig} onChange={setRecurringConfig} />
            )}

            {scheduleKind === 'one_shot' && (
              <div className="space-y-4">
                <Field label="Date">
                  <input
                    type="date"
                    value={oneShotDate}
                    onChange={(e) => setOneShotDate(e.target.value)}
                    className={inputCls}
                  />
                </Field>
                <Field label="Time">
                  <input
                    type="time"
                    value={oneShotTime}
                    onChange={(e) => setOneShotTime(e.target.value)}
                    className={inputCls}
                  />
                </Field>
                {oneShotDate && (
                  <p className="text-sm text-secondary">
                    Will run on {new Date(`${oneShotDate}T${oneShotTime}`).toLocaleString()}
                  </p>
                )}
              </div>
            )}
          </>
        )}

        {/* ── Step 4: Review ── */}
        {step === 'Review' && (
          <div className="space-y-4">
            <div className="rounded-xl border border-edge bg-surface p-5">
              <h3 className="text-sm font-semibold text-white mb-4">Summary</h3>
              <dl className="space-y-3">
                <Row label="Name" value={name} />
                {description && <Row label="Description" value={description} />}
                <Row label="Type" value={KIND_OPTIONS.find((k) => k.id === kind)?.label ?? kind} />
                {kind === 'shell_command' && <Row label="Command" value={command} mono />}
                {kind === 'agent_step' && <Row label="Prompt" value={prompt} />}
                {kind === 'script_file' && <Row label="Script" value={scriptPath} mono />}
                {kind === 'http_request' && (
                  <Row label="Request" value={`${httpMethod} ${httpUrl}`} mono />
                )}
                {kind === 'agent_loop' && (
                  <>
                    <Row label="Goal" value={goal} />
                    <Row label="Max iterations" value={String(loopMaxIterations)} />
                    <Row label="Token budget" value={loopMaxTokens.toLocaleString()} />
                  </>
                )}
                <Row label="Agent" value={agents.find((a) => a.id === agentId)?.name ?? agentId} />
                <Row
                  label="Concurrency"
                  value={
                    CONCURRENCY_OPTIONS.find((o) => o.value === concurrencyPolicy)?.label ??
                    concurrencyPolicy ??
                    'allow'
                  }
                />
                <Row label="Timeout" value={`${maxDurationMinutes} minutes`} />
                {maxRetries > 0 && (
                  <Row label="Retries" value={`${maxRetries}× (${retryDelaySecs}s base delay)`} />
                )}
                <Row label="Schedule" value={scheduleLabel()} />
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
      <div className="flex items-center justify-between pt-6 border-t border-edge mt-6">
        {stepIndex > 0 ? (
          <button
            onClick={() => setStep(STEPS[stepIndex - 1])}
            className="flex items-center gap-2 px-4 py-2 rounded-lg text-muted hover:text-white hover:bg-edge text-sm transition-colors"
          >
            <ChevronLeft size={14} /> Back
          </button>
        ) : (
          <button
            onClick={() => navigate('dashboard')}
            className="px-4 py-2 rounded-lg text-muted hover:text-white text-sm transition-colors"
          >
            Cancel
          </button>
        )}

        {stepIndex < STEPS.length - 1 ? (
          <button
            disabled={!canProceed()}
            onClick={() => setStep(STEPS[stepIndex + 1])}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium transition-colors"
          >
            Continue <ChevronRight size={14} />
          </button>
        ) : (
          <button
            disabled={creating}
            onClick={handleCreate}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-sm font-medium transition-colors"
          >
            {creating ? 'Creating…' : 'Create Task'} <Check size={14} />
          </button>
        )}
      </div>
    </div>
  );
}

