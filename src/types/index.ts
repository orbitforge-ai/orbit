// ─── Core domain types — mirror Rust structs (camelCase from serde) ──────────

// ─── Projects ────────────────────────────────────────────────────────────────

export interface Project {
  id: string;
  name: string;
  description: string | null;
  createdAt: string;
  updatedAt: string;
}

/**
 * Response shape for `list_projects` — same fields as `Project` plus an
 * aggregated `agentCount` computed server-side. Kept as a separate interface
 * so `get_project` / single-entity reads don't grow an unnecessary field.
 */
export interface ProjectSummary extends Project {
  agentCount: number;
}

export interface ProjectAgent {
  projectId: string;
  agentId: string;
  isDefault: boolean;
  addedAt: string;
}

// ─── Work items (project board) ──────────────────────────────────────────────

export type WorkItemStatus =
  | 'backlog'
  | 'todo'
  | 'in_progress'
  | 'blocked'
  | 'review'
  | 'done'
  | 'cancelled';

export type WorkItemKind = 'task' | 'bug' | 'story' | 'spike' | 'chore';

export interface WorkItem {
  id: string;
  projectId: string;
  title: string;
  description: string | null;
  kind: string;
  status: WorkItemStatus;
  priority: number;
  assigneeAgentId: string | null;
  createdByAgentId: string | null;
  parentWorkItemId: string | null;
  position: number;
  labels: string[];
  metadata: Record<string, unknown>;
  blockedReason: string | null;
  startedAt: string | null;
  completedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface CreateWorkItem {
  projectId: string;
  title: string;
  description?: string;
  kind?: string;
  status?: WorkItemStatus;
  priority?: number;
  assigneeAgentId?: string;
  createdByAgentId?: string;
  parentWorkItemId?: string;
  position?: number;
  labels?: string[];
  metadata?: Record<string, unknown>;
}

export interface UpdateWorkItem {
  title?: string;
  description?: string;
  kind?: string;
  priority?: number;
  labels?: string[];
  metadata?: Record<string, unknown>;
}

export interface WorkItemComment {
  id: string;
  workItemId: string;
  authorKind: 'user' | 'agent';
  authorAgentId: string | null;
  body: string;
  createdAt: string;
  updatedAt: string;
}

export type CommentAuthor = { kind: 'user' } | { kind: 'agent'; agentId: string };

// ─── Project workflows ──────────────────────────────────────────────────────

export interface NodePosition {
  x: number;
  y: number;
}

export interface WorkflowNode {
  id: string;
  type: string;
  position: NodePosition;
  data: Record<string, unknown>;
}

export interface WorkflowEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string | null;
}

export interface WorkflowGraph {
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
  schemaVersion: number;
}

export interface ProjectWorkflow {
  id: string;
  projectId: string;
  name: string;
  description: string | null;
  enabled: boolean;
  graph: WorkflowGraph;
  triggerKind: string;
  triggerConfig: Record<string, unknown>;
  version: number;
  createdAt: string;
  updatedAt: string;
}

export interface CreateProjectWorkflow {
  projectId: string;
  name: string;
  description?: string | null;
  triggerKind?: string;
  triggerConfig?: Record<string, unknown>;
  graph?: WorkflowGraph;
}

export interface UpdateProjectWorkflow {
  name?: string;
  description?: string | null;
  triggerKind?: string;
  triggerConfig?: Record<string, unknown>;
  graph?: WorkflowGraph;
}

export type RuleOperator =
  | 'equals'
  | 'notEquals'
  | 'contains'
  | 'notContains'
  | 'startsWith'
  | 'endsWith'
  | 'greaterThan'
  | 'greaterThanOrEqual'
  | 'lessThan'
  | 'lessThanOrEqual'
  | 'exists'
  | 'notExists'
  | 'isTrue'
  | 'isFalse'
  | 'matchesRegex';

export type RuleCombinator = 'and' | 'or';

export interface RuleLeaf {
  field: string;
  operator: RuleOperator;
  value?: unknown;
}

export interface RuleGroup {
  combinator: RuleCombinator;
  rules: RuleNode[];
}

export type RuleNode = RuleGroup | RuleLeaf;

export const RULE_OPERATORS: RuleOperator[] = [
  'equals',
  'notEquals',
  'contains',
  'notContains',
  'startsWith',
  'endsWith',
  'greaterThan',
  'greaterThanOrEqual',
  'lessThan',
  'lessThanOrEqual',
  'exists',
  'notExists',
  'isTrue',
  'isFalse',
  'matchesRegex',
];

export const KNOWN_NODE_TYPES = [
  'trigger.manual',
  'trigger.schedule',
  'agent.run',
  'logic.if',
  'integration.gmail.read',
  'integration.gmail.send',
  'integration.slack.send',
  'integration.http.request',
] as const;

export type WorkflowNodeType = (typeof KNOWN_NODE_TYPES)[number];

// ─── Workflow runs (Phase 4 runtime) ─────────────────────────────────────────

export type WorkflowRunStatus =
  | 'queued'
  | 'running'
  | 'success'
  | 'failed'
  | 'cancelled';

export type WorkflowRunStepStatus =
  | 'queued'
  | 'running'
  | 'success'
  | 'failed'
  | 'skipped';

export interface WorkflowRun {
  id: string;
  workflowId: string;
  workflowVersion: number;
  graphSnapshot: WorkflowGraph;
  triggerKind: string;
  triggerData: Record<string, unknown>;
  status: WorkflowRunStatus;
  error: string | null;
  startedAt: string | null;
  completedAt: string | null;
  createdAt: string;
}

export interface WorkflowRunStep {
  id: string;
  runId: string;
  nodeId: string;
  nodeType: string;
  status: WorkflowRunStepStatus;
  input: unknown;
  output: unknown;
  error: string | null;
  startedAt: string | null;
  completedAt: string | null;
  sequence: number;
}

export interface WorkflowRunWithSteps extends WorkflowRun {
  steps: WorkflowRunStep[];
}

// ─── Memory ──────────────────────────────────────────────────────────────────

export type MemoryType = 'user' | 'feedback' | 'project' | 'reference';

export interface MemoryEntry {
  id: string;
  text: string;
  memoryType: MemoryType;
  userId: string;
  createdAt: string;
  updatedAt: string;
  source: 'explicit' | 'auto_extracted';
  score?: number | null;
}

export interface Agent {
  id: string;
  name: string;
  description: string | null;
  state: 'idle' | 'busy' | 'paused' | 'error' | 'offline';
  maxConcurrentRuns: number;
  heartbeatAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export type AvatarArchetype = 'auto' | 'fox' | 'bear' | 'owl' | 'spark' | 'cat' | 'bot' | 'sage';

export interface AgentIdentityConfig {
  presetId: string;
  identityName: string;
  voice: string;
  vibe: string;
  warmth: number;
  directness: number;
  humor: number;
  customNote?: string;
  avatarEnabled: boolean;
  avatarArchetype: AvatarArchetype;
  avatarSpeakAloud: boolean;
}

export interface Task {
  id: string;
  name: string;
  description: string | null;
  kind: 'shell_command' | 'script_file' | 'http_request' | 'agent_step' | 'agent_loop';
  config:
    | ShellCommandConfig
    | ScriptFileConfig
    | HttpRequestConfig
    | AgentStepConfig
    | AgentLoopConfig
    | Record<string, unknown>;
  maxDurationSeconds: number;
  maxRetries: number;
  retryDelaySeconds: number;
  concurrencyPolicy: 'allow' | 'skip' | 'queue' | 'cancel_previous';
  tags: string[];
  agentId: string | null;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
  projectId: string | null;
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
  method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
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
  | 'pending'
  | 'queued'
  | 'running'
  | 'success'
  | 'failure'
  | 'cancelled'
  | 'timed_out';

export interface Run {
  id: string;
  taskId: string;
  scheduleId: string | null;
  agentId: string | null;
  state: RunState;
  trigger: 'scheduled' | 'manual' | 'channel' | 'retry' | 'bus' | 'sub_agent';
  exitCode: number | null;
  pid: number | null;
  logPath: string;
  startedAt: string | null;
  finishedAt: string | null;
  durationMs: number | null;
  retryCount: number;
  parentRunId: string | null;
  metadata: Record<string, unknown>;
  isSubAgent: boolean;
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
  isSubAgent: boolean;
  createdAt: string;
  chatSessionId: string | null;
  projectId: string | null;
}

export interface Schedule {
  id: string;
  taskId: string;
  kind: 'recurring' | 'one_shot' | 'triggered';
  config: RecurringConfig | OneShotConfig | Record<string, unknown>;
  enabled: boolean;
  nextRunAt: string | null;
  lastRunAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface RecurringConfig {
  intervalUnit: 'minutes' | 'hours' | 'days' | 'weeks' | 'months';
  intervalValue: number;
  daysOfWeek?: number[]; // 0=Sun … 6=Sat
  timeOfDay?: { hour: number; minute: number };
  timezone: string;
  missedRunPolicy: 'run_once' | 'skip';
  /** Original text/cron input so users can edit what they typed */
  expression?: string;
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
  sessionType: 'user_chat' | 'bus_message' | 'sub_agent' | 'pulse';
  parentSessionId: string | null;
  sourceBusMessageId: string | null;
  chainDepth: number;
  executionState:
    | 'queued'
    | 'running'
    | 'waiting_message'
    | 'waiting_user'
    | 'waiting_timeout'
    | 'waiting_sub_agents'
    | 'success'
    | 'failure'
    | 'cancelled'
    | 'timed_out'
    | null;
  finishSummary: string | null;
  terminalError: string | null;
  sourceAgentId?: string | null;
  sourceAgentName?: string | null;
  sourceSessionId?: string | null;
  sourceSessionTitle?: string | null;
  createdAt: string;
  updatedAt: string;
  projectId?: string | null;
}

export interface ChatDraft {
  id: string;
  agentId: string;
  text: string;
  createdAt: string;
  updatedAt: string;
}

// ─── IPC event payloads ───────────────────────────────────────────────────────

export interface LogLine {
  stream: 'stdout' | 'stderr';
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
  kind: Task['kind'];
  config:
    | ShellCommandConfig
    | ScriptFileConfig
    | HttpRequestConfig
    | AgentStepConfig
    | AgentLoopConfig
    | Record<string, unknown>;
  maxDurationSeconds?: number;
  maxRetries?: number;
  retryDelaySeconds?: number;
  concurrencyPolicy?: Task['concurrencyPolicy'];
  tags?: string[];
  agentId?: string;
  projectId?: string;
}

export interface CreateSchedule {
  taskId: string;
  kind: Schedule['kind'];
  config: RecurringConfig | OneShotConfig | Record<string, unknown>;
}

export interface CreateAgent {
  name: string;
  description?: string;
  maxConcurrentRuns?: number;
  identity?: AgentIdentityConfig;
  roleId?: string;
  roleSystemInstructions?: string;
}

export interface UpdateAgent {
  name?: string;
  description?: string;
  maxConcurrentRuns?: number;
}

// ─── Permission types ────────────────────────────────────────────────────────

export interface PermissionRule {
  id: string;
  tool: string;
  pattern: string;
  decision: 'allow' | 'deny';
  createdAt: string;
  description?: string;
}

export interface PermissionRequestPayload {
  requestId: string;
  runId: string;
  sessionId: string | null;
  agentId: string;
  toolName: string;
  toolInput: Record<string, unknown>;
  riskLevel: 'moderate' | 'dangerous';
  riskDescription: string;
  suggestedPattern: string;
  timestamp: string;
}

export interface PermissionCancelledPayload {
  requestId: string;
  runId: string;
  timestamp: string;
}

export interface UserQuestionPayload {
  requestId: string;
  runId: string;
  sessionId: string | null;
  question: string;
  choices: string[] | null;
  allowCustom: boolean;
  multiSelect: boolean;
  context: string | null;
  timestamp: string;
}

// ─── Agent workspace types ────────────────────────────────────────────────────

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
  compactionThreshold?: number;
  compactionRetainCount?: number;
  contextWindowOverride?: number;
  disabledSkills: string[];
  disabledTools: string[];
  identity: AgentIdentityConfig;
  memoryEnabled: boolean;
  memoryStalenessThresholdDays: number;
  roleId?: string;
  roleSystemInstructions?: string;
  defaultChannelId?: string;
}

// ─── Global settings ─────────────────────────────────────────────────────────

export type ChannelType = 'slack' | 'discord' | 'webhook';

export interface ChannelConfig {
  id: string;
  name: string;
  type: ChannelType;
  webhookUrl: string;
  enabled: boolean;
}

export interface ChatDisplaySettings {
  showAgentThoughts: boolean;
  showVerboseToolDetails: boolean;
}

export interface AgentDefaults {
  allowedTools: string[];
  permissionMode: 'normal' | 'strict' | 'permissive';
  permissionRules: PermissionRule[];
  webSearchProvider: string;
}

export interface GlobalSettings {
  version: number;
  chatDisplay: ChatDisplaySettings;
  agentDefaults: AgentDefaults;
  channels: ChannelConfig[];
}

// ─── Agent Skills types ────────────────────────────────────────────────────

export type SkillSource = 'agent_local' | 'orbit_global' | 'standard' | 'built_in';

export interface SkillInfo {
  name: string;
  description: string;
  source: SkillSource;
  enabled: boolean;
  sourcePath?: string;
}

// ─── LLM content types ──────────────────────────────────────────────────────

export type ContentBlock =
  | { type: 'text'; text: string }
  | { type: 'thinking'; thinking: string }
  | { type: 'tool_use'; id: string; name: string; input: Record<string, unknown> }
  | { type: 'tool_result'; tool_use_id: string; content: string; is_error: boolean }
  | { type: 'image'; media_type: string; data: string };

export interface ChatMessage {
  id?: string;
  role: 'user' | 'assistant';
  content: ContentBlock[];
  created_at?: string;
  isCompacted?: boolean;
}

export interface PaginatedChatMessages {
  messages: ChatMessage[];
  totalCount: number;
  hasMore: boolean;
}

export interface MessageReaction {
  id: string;
  messageId: string;
  emoji: string;
  createdAt: string;
}

export interface SendChatMessageResponse {
  streamId: string;
  userMessageId: string;
}

export interface MessageReactionPayload {
  sessionId: string;
  messageId: string;
  reactionId: string;
  emoji: string;
  timestamp: string;
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
  action: 'llm_call' | 'tool_exec' | 'finished';
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

// ─── Context management types ───────────────────────────────────────────────

export interface ChatContextUpdatePayload {
  sessionId: string;
  inputTokens: number;
  outputTokens: number;
  contextWindowSize: number;
  usagePercent: number;
  timestamp: string;
}

export interface ContextUsage {
  inputTokens: number;
  contextWindowSize: number;
  usagePercent: number;
}

export interface CompactionStatusPayload {
  sessionId: string;
  status: 'started' | 'completed' | 'failed';
  timestamp: string;
}

// ─── Agent Bus types ───────────────────────────────────────────────────────

export interface BusMessage {
  id: string;
  fromAgentId: string;
  fromRunId: string | null;
  fromSessionId: string | null;
  toAgentId: string;
  toRunId: string | null;
  toSessionId: string | null;
  kind: 'direct' | 'event';
  eventType: string | null;
  payload: Record<string, unknown>;
  status: 'delivered' | 'failed' | 'depth_exceeded';
  createdAt: string;
}

export interface BusSubscription {
  id: string;
  subscriberAgentId: string;
  sourceAgentId: string;
  eventType: 'run:completed' | 'run:failed' | 'run:any_terminal';
  taskId: string;
  payloadTemplate: string;
  enabled: boolean;
  maxChainDepth: number;
  createdAt: string;
  updatedAt: string;
}

export interface CreateBusSubscription {
  subscriberAgentId: string;
  sourceAgentId: string;
  eventType: string;
  taskId: string;
  payloadTemplate?: string;
  maxChainDepth?: number;
}

export interface SubAgentsSpawnedPayload {
  parentSessionId: string | null;
  parentRunId: string | null;
  subAgentSessionIds: string[];
  timestamp: string;
}

export interface BusThreadMessage {
  id: string;
  fromAgentId: string;
  fromAgentName: string;
  toAgentId: string;
  kind: 'direct' | 'event';
  payload: Record<string, unknown>;
  status: string;
  createdAt: string;
  triggeredRunId: string | null;
  triggeredRunState: string | null;
  triggeredRunSummary: string | null;
  triggeredSessionId: string | null;
  triggeredSessionState: string | null;
  triggeredSessionSummary: string | null;
}

export interface PaginatedBusThread {
  messages: BusThreadMessage[];
  totalCount: number;
  hasMore: boolean;
}

export interface BusMessageSentPayload {
  messageId: string;
  fromAgentId: string;
  toAgentId: string;
  kind: string;
  payload: Record<string, unknown>;
  triggeredSessionId: string | null;
  triggeredRunId: string | null;
  timestamp: string;
}

// ─── Agent metadata event payloads ───────────────────────────────────────────

export interface AgentCreatedPayload {
  agent: Agent;
  roleId: string | null;
}

export interface AgentUpdatedPayload {
  agent: Agent;
}

export interface AgentDeletedPayload {
  agentId: string;
}

export interface AgentConfigChangedPayload {
  agentId: string;
  roleId: string | null;
}

export interface SessionExecutionStatus {
  sessionId: string;
  executionState: ChatSession['executionState'];
  finishSummary: string | null;
  terminalError: string | null;
}
