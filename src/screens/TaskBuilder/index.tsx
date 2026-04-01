import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Terminal,
  Globe,
  FileCode,
  Bot,
  Cpu,
  ChevronRight,
  ChevronLeft,
  Check,
  Plus,
  Minus,
  ChevronDown,
} from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import * as Slider from '@radix-ui/react-slider';
import { tasksApi } from '../../api/tasks';
import { schedulesApi } from '../../api/schedules';
import { agentsApi } from '../../api/agents';
import { useUiStore } from '../../store/uiStore';
import { RecurringPicker } from '../ScheduleBuilder/RecurringPicker';
import { humanSchedule } from '../../lib/humanSchedule';
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

type TaskKind = 'shell_command' | 'script_file' | 'http_request' | 'agent_step' | 'agent_loop';
type ScheduleKind = 'none' | 'recurring' | 'one_shot';

const KIND_OPTIONS: {
  id: TaskKind;
  label: string;
  description: string;
  icon: React.ElementType;
}[] = [
  {
    id: 'shell_command',
    label: 'Shell Command',
    description: 'Run a bash/sh command',
    icon: Terminal,
  },
  {
    id: 'script_file',
    label: 'Script File',
    description: 'Execute a file on disk',
    icon: FileCode,
  },
  { id: 'http_request', label: 'HTTP Request', description: 'Call a URL or webhook', icon: Globe },
  { id: 'agent_step', label: 'Prompt', description: "Send a prompt to the agent's LLM", icon: Bot },
  { id: 'agent_loop', label: 'Agent Loop', description: 'Autonomous LLM-powered agent', icon: Cpu },
];

const CONCURRENCY_OPTIONS: { value: string; label: string; hint: string }[] = [
  { value: 'allow', label: 'Allow', hint: 'Start a new run even if one is active' },
  { value: 'skip', label: 'Skip', hint: 'Drop the new run if agent is busy' },
  { value: 'queue', label: 'Queue', hint: 'Wait for a free slot before starting' },
  {
    value: 'cancel_previous',
    label: 'Cancel previous',
    hint: 'Stop the active run and start the new one',
  },
];

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

  // Shell / AgentStep config
  const [command, setCommand] = useState('');
  const [workingDir, setWorkingDir] = useState('');

  // Script file config
  const [scriptPath, setScriptPath] = useState('');
  const [interpreter, setInterpreter] = useState('/bin/sh');

  // Agent step (prompt) config
  const [prompt, setPrompt] = useState('');

  // Agent loop config
  const [goal, setGoal] = useState('');
  const [loopMaxIterations, setLoopMaxIterations] = useState(25);
  const [loopMaxTokens, setLoopMaxTokens] = useState(200000);

  // HTTP config
  const [httpUrl, setHttpUrl] = useState('');
  const [httpMethod, setHttpMethod] = useState<HttpRequestConfig['method']>('GET');
  const [httpHeaders, setHttpHeaders] = useState<{ k: string; v: string }[]>([]);
  const [httpBody, setHttpBody] = useState('');
  const [httpExpectedCodes, setHttpExpectedCodes] = useState('');

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

            {/* Config panel for each kind */}
            {kind === 'shell_command' && (
              <>
                <Field label="Command">
                  <textarea
                    value={command}
                    onChange={(e) => setCommand(e.target.value)}
                    rows={6}
                    placeholder={"#!/bin/bash\necho 'Hello from Orbit!'"}
                    className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-green-400 text-sm font-mono placeholder-border focus:outline-none focus:border-accent resize-none"
                  />
                </Field>
                <Field label="Working directory (optional)">
                  <input
                    type="text"
                    value={workingDir}
                    onChange={(e) => setWorkingDir(e.target.value)}
                    placeholder="~/scripts"
                    className={inputCls}
                  />
                </Field>
              </>
            )}

            {kind === 'agent_step' && (
              <Field label="Prompt">
                <textarea
                  value={prompt}
                  onChange={(e) => setPrompt(e.target.value)}
                  rows={6}
                  placeholder="e.g., Summarize the key trends in our latest sales data and suggest three action items."
                  className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-white text-sm placeholder-border focus:outline-none focus:border-accent resize-none leading-relaxed"
                />
              </Field>
            )}

            {kind === 'script_file' && (
              <>
                <Field label="Script path">
                  <input
                    type="text"
                    value={scriptPath}
                    onChange={(e) => setScriptPath(e.target.value)}
                    placeholder="/Users/you/scripts/backup.sh"
                    className={inputCls}
                  />
                </Field>
                <Field label="Interpreter">
                  <input
                    type="text"
                    value={interpreter}
                    onChange={(e) => setInterpreter(e.target.value)}
                    placeholder="/bin/sh"
                    className={inputCls}
                  />
                </Field>
                <Field label="Working directory (optional)">
                  <input
                    type="text"
                    value={workingDir}
                    onChange={(e) => setWorkingDir(e.target.value)}
                    placeholder="~/scripts"
                    className={inputCls}
                  />
                </Field>
              </>
            )}

            {kind === 'agent_loop' && (
              <>
                <Field label="Goal">
                  <textarea
                    value={goal}
                    onChange={(e) => setGoal(e.target.value)}
                    rows={5}
                    placeholder="e.g., Create a Python script that scrapes weather data and saves it to a CSV file"
                    className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-white text-sm placeholder-border focus:outline-none focus:border-accent resize-none leading-relaxed"
                  />
                </Field>
                <div className="grid grid-cols-2 gap-4">
                  <Field label="Max iterations">
                    <input
                      type="number"
                      min={1}
                      max={100}
                      value={loopMaxIterations}
                      onChange={(e) => setLoopMaxIterations(Number(e.target.value))}
                      className={inputCls}
                    />
                  </Field>
                  <Field label="Max total tokens">
                    <input
                      type="number"
                      min={1000}
                      step={10000}
                      value={loopMaxTokens}
                      onChange={(e) => setLoopMaxTokens(Number(e.target.value))}
                      className={inputCls}
                    />
                  </Field>
                </div>
              </>
            )}

            {kind === 'http_request' && (
              <>
                <Field label="URL">
                  <div className="flex gap-2">
                    <Select.Root
                      value={httpMethod}
                      onValueChange={(v) => setHttpMethod(v as HttpRequestConfig['method'])}
                    >
                      <Select.Trigger className="flex items-center gap-2 px-3 py-2.5 rounded-lg bg-surface border border-edge text-white text-sm focus:outline-none focus:border-accent">
                        <Select.Value />
                        <Select.Icon>
                          <ChevronDown size={14} className="text-muted" />
                        </Select.Icon>
                      </Select.Trigger>
                      <Select.Portal>
                        <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                          <Select.Viewport className="p-1">
                            {['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].map((m) => (
                              <Select.Item
                                key={m}
                                value={m}
                                className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                              >
                                <Select.ItemText>{m}</Select.ItemText>
                              </Select.Item>
                            ))}
                          </Select.Viewport>
                        </Select.Content>
                      </Select.Portal>
                    </Select.Root>
                    <input
                      type="url"
                      value={httpUrl}
                      onChange={(e) => setHttpUrl(e.target.value)}
                      placeholder="https://api.example.com/endpoint"
                      className={`${inputCls} flex-1`}
                    />
                  </div>
                </Field>

                <Field label="Headers">
                  <div className="space-y-2">
                    {httpHeaders.map((h, i) => (
                      <div key={i} className="flex gap-2 items-center">
                        <input
                          type="text"
                          placeholder="Header-Name"
                          value={h.k}
                          onChange={(e) =>
                            setHttpHeaders((prev) =>
                              prev.map((x, j) => (j === i ? { ...x, k: e.target.value } : x))
                            )
                          }
                          className={`${inputCls} flex-1`}
                        />
                        <input
                          type="text"
                          placeholder="value"
                          value={h.v}
                          onChange={(e) =>
                            setHttpHeaders((prev) =>
                              prev.map((x, j) => (j === i ? { ...x, v: e.target.value } : x))
                            )
                          }
                          className={`${inputCls} flex-1`}
                        />
                        <button
                          onClick={() => setHttpHeaders((prev) => prev.filter((_, j) => j !== i))}
                          className="p-1.5 text-muted hover:text-red-400"
                        >
                          <Minus size={14} />
                        </button>
                      </div>
                    ))}
                    <button
                      onClick={() => setHttpHeaders((prev) => [...prev, { k: '', v: '' }])}
                      className="flex items-center gap-1.5 text-xs text-accent hover:text-accent-hover"
                    >
                      <Plus size={12} /> Add header
                    </button>
                  </div>
                </Field>

                {['POST', 'PUT', 'PATCH'].includes(httpMethod) && (
                  <Field label="Body">
                    <textarea
                      value={httpBody}
                      onChange={(e) => setHttpBody(e.target.value)}
                      rows={4}
                      placeholder='{"key": "value"}'
                      className="w-full px-4 py-3 rounded-lg bg-inset border border-edge text-green-400 text-sm font-mono placeholder-border focus:outline-none focus:border-accent resize-none"
                    />
                  </Field>
                )}

                <Field label="Expected status codes (optional)">
                  <input
                    type="text"
                    value={httpExpectedCodes}
                    onChange={(e) => setHttpExpectedCodes(e.target.value)}
                    placeholder="200, 201, 204"
                    className={inputCls}
                  />
                  <p className="text-xs text-muted mt-1">
                    Comma-separated. Leave blank for any 2xx.
                  </p>
                </Field>
              </>
            )}
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

const inputCls =
  'w-full px-4 py-2.5 rounded-lg bg-surface border border-edge text-white text-sm placeholder-border-hover focus:outline-none focus:border-accent';

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-sm font-medium text-secondary mb-1.5">{label}</label>
      {children}
    </div>
  );
}

function Row({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex gap-3">
      <dt className="w-28 flex-shrink-0 text-xs text-muted pt-0.5">{label}</dt>
      <dd
        className={`flex-1 text-sm text-white break-all ${mono ? 'font-mono text-green-400' : ''}`}
      >
        {value}
      </dd>
    </div>
  );
}
