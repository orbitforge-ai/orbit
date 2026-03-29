// ─── Core domain types — mirror Rust structs (camelCase from serde) ──────────

export interface Agent {
  id: string;
  name: string;
  description: string | null;
  state: "idle" | "busy" | "paused" | "error" | "offline";
  maxConcurrentRuns: number;
  heartbeatAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface Task {
  id: string;
  name: string;
  description: string | null;
  kind: "shell_command" | "script_file" | "http_request" | "agent_step" | "agent_loop";
  config: ShellCommandConfig | ScriptFileConfig | HttpRequestConfig | AgentStepConfig | AgentLoopConfig | Record<string, unknown>;
  maxDurationSeconds: number;
  maxRetries: number;
  retryDelaySeconds: number;
  concurrencyPolicy: "allow" | "skip" | "queue" | "cancel_previous";
  tags: string[];
  agentId: string | null;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface ShellCommandConfig {
  command: string;
  workingDirectory?: string;
  environment?: Record<string, string>;
  shell?: string;
}

export interface ScriptFileConfig {
  scriptPath: string;
  interpreter?: string;
  workingDirectory?: string;
  environment?: Record<string, string>;
}

export interface HttpRequestConfig {
  url: string;
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  headers?: Record<string, string>;
  body?: string;
  timeoutSeconds?: number;
  expectedStatusCodes?: number[];
}

export interface AgentStepConfig {
  prompt: string;
}

export interface AgentLoopConfig {
  goal: string;
  model?: string;
  maxIterations?: number;
  maxTotalTokens?: number;
  templateVars?: Record<string, string>;
}

export type RunState =
  | "pending"
  | "queued"
  | "running"
  | "success"
  | "failure"
  | "cancelled"
  | "timed_out";

export interface Run {
  id: string;
  taskId: string;
  scheduleId: string | null;
  agentId: string | null;
  state: RunState;
  trigger: "scheduled" | "manual" | "channel" | "retry";
  exitCode: number | null;
  pid: number | null;
  logPath: string;
  startedAt: string | null;
  finishedAt: string | null;
  durationMs: number | null;
  retryCount: number;
  parentRunId: string | null;
  metadata: Record<string, unknown>;
  createdAt: string;
}

export interface RunSummary {
  id: string;
  taskId: string;
  taskName: string;
  scheduleId: string | null;
  agentId: string | null;
  agentName: string | null;
  state: RunState;
  trigger: string;
  exitCode: number | null;
  startedAt: string | null;
  finishedAt: string | null;
  durationMs: number | null;
  retryCount: number;
  createdAt: string;
}

export interface Schedule {
  id: string;
  taskId: string;
  kind: "recurring" | "one_shot" | "triggered";
  config: RecurringConfig | OneShotConfig | Record<string, unknown>;
  enabled: boolean;
  nextRunAt: string | null;
  lastRunAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface RecurringConfig {
  intervalUnit: "minutes" | "hours" | "days" | "weeks" | "months";
  intervalValue: number;
  daysOfWeek?: number[]; // 0=Sun … 6=Sat
  timeOfDay?: { hour: number; minute: number };
  timezone: string;
  missedRunPolicy: "run_once" | "skip";
}

export interface OneShotConfig {
  runAt: string; // ISO 8601
  timezone: string;
}

// ─── Chat types ─────────────────────────────────────────────────────────────

export interface ChatSession {
  id: string;
  agentId: string;
  title: string;
  archived: boolean;
  createdAt: string;
  updatedAt: string;
}

// ─── IPC event payloads ───────────────────────────────────────────────────────

export interface LogLine {
  stream: "stdout" | "stderr";
  line: string;
}

export interface RunLogChunkPayload {
  runId: string;
  lines: LogLine[];
  timestamp: string;
}

export interface RunStateChangedPayload {
  runId: string;
  previousState: RunState;
  newState: RunState;
  timestamp: string;
}

// ─── Command payloads ─────────────────────────────────────────────────────────

export interface CreateTask {
  name: string;
  description?: string;
  kind: Task["kind"];
  config: ShellCommandConfig | ScriptFileConfig | HttpRequestConfig | AgentStepConfig | AgentLoopConfig | Record<string, unknown>;
  maxDurationSeconds?: number;
  maxRetries?: number;
  retryDelaySeconds?: number;
  concurrencyPolicy?: Task["concurrencyPolicy"];
  tags?: string[];
  agentId?: string;
}

export interface CreateSchedule {
  taskId: string;
  kind: Schedule["kind"];
  config: RecurringConfig | OneShotConfig | Record<string, unknown>;
}

export interface CreateAgent {
  name: string;
  description?: string;
  maxConcurrentRuns?: number;
}

export interface UpdateAgent {
  name?: string;
  description?: string;
  maxConcurrentRuns?: number;
}

// ─── Agent workspace types ───────────────────────────────────────────────────

export interface FileEntry {
  name: string;
  isDir: boolean;
  sizeBytes: number;
  modifiedAt: string;
}

export interface AgentWorkspaceConfig {
  provider: string;
  model: string;
  temperature: number;
  maxIterations: number;
  maxTotalTokens: number;
  allowedTools: string[];
}

// ─── LLM content types ──────────────────────────────────────────────────────

export type ContentBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; thinking: string }
  | { type: "tool_use"; id: string; name: string; input: Record<string, unknown> }
  | { type: "tool_result"; tool_use_id: string; content: string; is_error: boolean }
  | { type: "image"; media_type: string; data: string };

export interface ChatMessage {
  role: "user" | "assistant";
  content: ContentBlock[];
}

// ─── Agent loop event payloads ───────────────────────────────────────────────

export interface AgentLlmChunkPayload {
  runId: string;
  delta: string;
  iteration: number;
  timestamp: string;
}

export interface AgentIterationPayload {
  runId: string;
  iteration: number;
  action: "llm_call" | "tool_exec" | "finished";
  toolName: string | null;
  totalTokens: number;
  timestamp: string;
}

export interface AgentContentBlockPayload {
  runId: string;
  iteration: number;
  blockType: string;
  block: ContentBlock;
  timestamp: string;
}

export interface AgentToolResultPayload {
  runId: string;
  iteration: number;
  toolUseId: string;
  content: string;
  isError: boolean;
  timestamp: string;
}
