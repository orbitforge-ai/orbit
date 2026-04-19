import { useQuery } from '@tanstack/react-query';
import { Node } from '@xyflow/react';
import { agentsApi } from '../../api/agents';
import { projectsApi } from '../../api/projects';
import {
  Agent,
  ProjectBoardColumn,
  RuleGroup,
  RuleNode,
  WorkItemKind,
  WorkItemStatus,
} from '../../types';
import { RecurringPicker } from '../ScheduleBuilder/RecurringPicker';
import { nodeMeta } from './nodeRegistry';
import { RuleBuilder } from './RuleBuilder';
import { ruleToSentence } from './ruleSentence';
import { getWorkflowScheduleConfig } from './scheduleConfig';

interface Props {
  node: Node | null;
  projectId: string;
  onChangeData: (nodeId: string, data: Record<string, unknown>) => void;
  onDelete: (nodeId: string) => void;
}

const WORK_ITEM_KIND_OPTIONS: Array<{ value: WorkItemKind; label: string }> = [
  { value: 'task', label: 'Task' },
  { value: 'bug', label: 'Bug' },
  { value: 'story', label: 'Story' },
  { value: 'spike', label: 'Spike' },
  { value: 'chore', label: 'Chore' },
];

const CREATE_STATUS_OPTIONS: Array<{ value: Exclude<WorkItemStatus, 'blocked'>; label: string }> = [
  { value: 'backlog', label: 'Backlog' },
  { value: 'todo', label: 'Todo' },
  { value: 'in_progress', label: 'In progress' },
  { value: 'review', label: 'Review' },
  { value: 'done', label: 'Done' },
  { value: 'cancelled', label: 'Cancelled' },
];

const ALL_STATUS_OPTIONS: Array<{ value: WorkItemStatus; label: string }> = [
  { value: 'backlog', label: 'Backlog' },
  { value: 'todo', label: 'Todo' },
  { value: 'in_progress', label: 'In progress' },
  { value: 'blocked', label: 'Blocked' },
  { value: 'review', label: 'Review' },
  { value: 'done', label: 'Done' },
  { value: 'cancelled', label: 'Cancelled' },
];

const WORK_ITEM_ACTION_OPTIONS = [
  { value: 'create', label: 'Create' },
  { value: 'list', label: 'List' },
  { value: 'get', label: 'Get by ID' },
  { value: 'update', label: 'Update' },
  { value: 'move', label: 'Move status' },
  { value: 'block', label: 'Block' },
  { value: 'complete', label: 'Complete' },
  { value: 'comment', label: 'Comment' },
  { value: 'list_comments', label: 'List comments' },
  { value: 'claim', label: 'Claim' },
  { value: 'delete', label: 'Delete' },
] as const;

type WorkItemNodeAction = (typeof WORK_ITEM_ACTION_OPTIONS)[number]['value'];

export function NodeInspector({ node, projectId, onChangeData, onDelete }: Props) {
  if (!node) {
    return (
      <aside className="w-80 border-l border-edge bg-background/50 px-4 py-4">
        <p className="text-xs text-muted">Select a node to edit its settings.</p>
      </aside>
    );
  }
  const meta = nodeMeta(node.type ?? '');
  const data = (node.data ?? {}) as Record<string, unknown>;
  const update = (patch: Record<string, unknown>) => onChangeData(node.id, { ...data, ...patch });

  return (
    <aside className="w-80 border-l border-edge bg-background/50 overflow-y-auto">
      <div className="px-4 py-3 border-b border-edge flex items-center justify-between">
        <div>
          <p className="text-[10px] uppercase tracking-wider text-muted">Node</p>
          <p className="text-sm font-semibold text-white">{meta?.label ?? node.type}</p>
        </div>
        <button
          onClick={() => onDelete(node.id)}
          className="text-[11px] text-muted hover:text-red-400 transition-colors"
        >
          Delete
        </button>
      </div>

      <div className="px-4 py-3 space-y-4">
        {node.type === 'trigger.manual' && (
          <p className="text-xs text-muted">No configuration. Run from the editor toolbar.</p>
        )}

        {node.type === 'trigger.schedule' && (
          <ScheduleInspector data={data} onUpdate={update} />
        )}

        {node.type === 'agent.run' && <AgentRunInspector data={data} onUpdate={update} />}

        {node.type === 'logic.if' && <LogicIfInspector data={data} onUpdate={update} />}

        {node.type === 'board.work_item.create' && (
          <WorkItemInspector
            data={data}
            projectId={projectId}
            onUpdate={update}
          />
        )}

        {node.type === 'board.proposal.enqueue' && (
          <ProposalQueueInspector data={data} projectId={projectId} onUpdate={update} />
        )}

        {node.type === 'integration.feed.fetch' && (
          <FeedFetchInspector data={data} onUpdate={update} />
        )}

        {node.type === 'integration.http.request' && (
          <HttpRequestInspector data={data} onUpdate={update} />
        )}

        {(node.type === 'integration.gmail.read' ||
          node.type === 'integration.gmail.send' ||
          node.type === 'integration.slack.send') && (
          <p className="text-xs text-muted italic">Integration nodes are coming in a later phase.</p>
        )}
      </div>
    </aside>
  );
}

function ScheduleInspector({
  data,
  onUpdate,
}: {
  data: Record<string, unknown>;
  onUpdate: (patch: Record<string, unknown>) => void;
}) {
  const config = getWorkflowScheduleConfig(data);
  return (
    <div className="space-y-2">
      <label className="text-[11px] uppercase tracking-wider text-muted">Schedule</label>
      <RecurringPicker
        value={config}
        onChange={(next) => onUpdate({ ...next, cron: undefined })}
      />
    </div>
  );
}

function AgentRunInspector({
  data,
  onUpdate,
}: {
  data: Record<string, unknown>;
  onUpdate: (patch: Record<string, unknown>) => void;
}) {
  const { data: agents = [] } = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });
  const agentId = (data.agentId as string) ?? '';
  const promptTemplate = (data.promptTemplate as string) ?? '';
  const contextTemplate = (data.contextTemplate as string) ?? '';
  const outputMode = (data.outputMode as string) ?? 'text';

  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Agent</label>
        <select
          value={agentId}
          onChange={(e) => onUpdate({ agentId: e.target.value })}
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
        >
          <option value="">Select agent…</option>
          {agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      </div>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Prompt template</label>
        <textarea
          value={promptTemplate}
          onChange={(e) => onUpdate({ promptTemplate: e.target.value })}
          rows={6}
          placeholder="Categorize this email: {{trigger.body}}"
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
        />
        <p className="text-[10px] text-muted">
          Use <span className="font-mono">{`{{trigger.body}}`}</span> or{' '}
          <span className="font-mono">{`{{<nodeId>.output.<field>}}`}</span> to reference upstream
          data.
        </p>
      </div>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Fit context</label>
        <textarea
          value={contextTemplate}
          onChange={(e) => onUpdate({ contextTemplate: e.target.value })}
          rows={5}
          placeholder="Candidate profile, writing preferences, exclusions, portfolio notes…"
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
        />
      </div>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Output mode</label>
        <select
          value={outputMode}
          onChange={(e) => onUpdate({ outputMode: e.target.value })}
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
        >
          <option value="text">Text</option>
          <option value="json">JSON</option>
          <option value="proposal_candidates">Proposal candidates</option>
        </select>
      </div>
    </div>
  );
}

function LogicIfInspector({
  data,
  onUpdate,
}: {
  data: Record<string, unknown>;
  onUpdate: (patch: Record<string, unknown>) => void;
}) {
  const rule = (data.rule as RuleNode | undefined) ?? { combinator: 'and', rules: [] };
  const trueLabel = (data.trueLabel as string) ?? 'true';
  const falseLabel = (data.falseLabel as string) ?? 'false';

  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">When</label>
        <RuleBuilder
          rule={rule}
          onChange={(next: RuleGroup) => onUpdate({ rule: next })}
        />
        <p className="text-[10px] text-muted italic">
          {ruleToSentence(rule) || '(define at least one condition)'}
        </p>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <div className="space-y-1">
          <label className="text-[11px] uppercase tracking-wider text-emerald-300">
            True label
          </label>
          <input
            value={trueLabel}
            onChange={(e) => onUpdate({ trueLabel: e.target.value })}
            className="w-full bg-background border border-edge rounded px-2 py-1 text-xs text-white outline-none focus:border-accent"
          />
        </div>
        <div className="space-y-1">
          <label className="text-[11px] uppercase tracking-wider text-red-300">False label</label>
          <input
            value={falseLabel}
            onChange={(e) => onUpdate({ falseLabel: e.target.value })}
            className="w-full bg-background border border-edge rounded px-2 py-1 text-xs text-white outline-none focus:border-accent"
          />
        </div>
      </div>
    </div>
  );
}

function WorkItemInspector({
  data,
  projectId,
  onUpdate,
}: {
  data: Record<string, unknown>;
  projectId: string;
  onUpdate: (patch: Record<string, unknown>) => void;
}) {
  const { data: projectAgents = [] } = useQuery<Agent[]>({
    queryKey: ['project-agents', projectId],
    queryFn: () => projectsApi.listAgents(projectId),
  });
  const { data: boardColumns = [] } = useQuery<ProjectBoardColumn[]>({
    queryKey: ['project-board-columns', projectId],
    queryFn: () => projectsApi.listBoardColumns(projectId),
  });

  const action = ((data.action as WorkItemNodeAction | undefined) ?? 'create') as WorkItemNodeAction;
  const itemIdTemplate = (data.itemIdTemplate as string) ?? '';
  const titleTemplate = (data.titleTemplate as string) ?? '';
  const descriptionTemplate = (data.descriptionTemplate as string) ?? '';
  const columnId = (data.columnId as string) ?? '';
  const kind = ((data.kind as WorkItemKind | undefined) ?? 'task') as WorkItemKind;
  const status = ((data.status as WorkItemStatus | undefined) ?? 'backlog') as WorkItemStatus;
  const priorityValue = data.priority;
  const priority =
    typeof priorityValue === 'number' && Number.isFinite(priorityValue) ? priorityValue : 0;
  const labelsText = (data.labelsText as string) ?? '';
  const assigneeAgentId = (data.assigneeAgentId as string) ?? '';
  const parentWorkItemId = (data.parentWorkItemId as string) ?? '';
  const reasonTemplate = (data.reasonTemplate as string) ?? '';
  const bodyTemplate = (data.bodyTemplate as string) ?? '';
  const commentAuthorAgentId = (data.commentAuthorAgentId as string) ?? '';
  const listColumn = ((data.listColumn as string | undefined) ?? (data.listStatus as string | undefined) ?? 'all') as string;
  const listStatus = ((data.listStatus as string | undefined) ?? 'all') as string;
  const listKind = ((data.listKind as string | undefined) ?? 'all') as string;
  const listAssignee = (data.listAssignee as string) ?? '';
  const limitValue = data.limit;
  const limit = typeof limitValue === 'number' && Number.isFinite(limitValue) ? limitValue : 25;

  const showItemId = action !== 'create' && action !== 'list';
  const showTitle = action === 'create' || action === 'update';
  const showDescription = action === 'create' || action === 'update';
  const showColumn = action === 'create' || action === 'update' || action === 'move';
  const showKind = action === 'create' || action === 'update' || action === 'list';
  const showStatus = action === 'create' || action === 'move' || action === 'list';
  const showPriority = action === 'create' || action === 'update';
  const showLabels = action === 'create' || action === 'update';
  const showAssignee = action === 'create' || action === 'claim';
  const showParent = action === 'create';
  const showReason = action === 'block';
  const showBody = action === 'comment';
  const showCommentAuthor = action === 'comment';
  const showListAssignee = action === 'list';
  const showLimit = action === 'list';

  return (
    <div className="space-y-3">
      <p className="text-[10px] text-muted">
        Interacts with this workflow&apos;s board. Template fields can reference earlier node
        outputs like <span className="font-mono">{`{{trigger.data.subject}}`}</span> or{' '}
        <span className="font-mono">{`{{nodeId.output.parsed.title}}`}</span>.
      </p>

      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Action</label>
        <select
          value={action}
          onChange={(e) => onUpdate({ action: e.target.value })}
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
        >
          {WORK_ITEM_ACTION_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>

      {showItemId && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Work item ID</label>
          <input
            value={itemIdTemplate}
            onChange={(e) => onUpdate({ itemIdTemplate: e.target.value })}
            placeholder="{{someNode.output.workItem.id}}"
            className="w-full bg-background border border-edge rounded px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono"
          />
        </div>
      )}

      <div className="space-y-1.5">
        {showTitle && (
          <>
            <label className="text-[11px] uppercase tracking-wider text-muted">Title template</label>
            <textarea
              value={titleTemplate}
              onChange={(e) => onUpdate({ titleTemplate: e.target.value })}
              rows={3}
              placeholder="Follow up on {{agentNode.output.parsed.customerName}}"
              className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
            />
          </>
        )}
      </div>

      {showDescription && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">
            Description template
          </label>
          <textarea
            value={descriptionTemplate}
            onChange={(e) => onUpdate({ descriptionTemplate: e.target.value })}
            rows={6}
            placeholder={`Customer summary:\n{{agentNode.output.text}}`}
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
          />
        </div>
      )}

      {showColumn && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Board column</label>
          <select
            value={columnId}
            onChange={(e) => onUpdate({ columnId: e.target.value })}
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
          >
            <option value="">Resolve from status</option>
            {boardColumns.map((column) => (
              <option key={column.id} value={column.id}>
                {column.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {(showKind || showStatus || showPriority) && (
        <div className="grid grid-cols-3 gap-2">
          {showKind && (
            <div className="space-y-1.5">
              <label className="text-[11px] uppercase tracking-wider text-muted">
                {action === 'list' ? 'Kind filter' : 'Kind'}
              </label>
              <select
                value={action === 'list' ? listKind || 'all' : kind || 'task'}
                onChange={(e) =>
                  onUpdate(action === 'list' ? { listKind: e.target.value } : { kind: e.target.value })
                }
                className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
              >
                {action === 'list' && <option value="all">All kinds</option>}
                {WORK_ITEM_KIND_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </div>
          )}

          {showStatus && (
            <div className="space-y-1.5">
              <label className="text-[11px] uppercase tracking-wider text-muted">
                {action === 'list' ? 'Board column' : 'Status'}
              </label>
              <select
                value={action === 'list' ? listColumn || listStatus || 'all' : status || 'backlog'}
                onChange={(e) =>
                  onUpdate(
                    action === 'list'
                      ? { listColumn: e.target.value, listStatus: e.target.value }
                      : { status: e.target.value },
                  )
                }
                className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
              >
                {action === 'list' ? (
                  <>
                    <option value="all">All columns</option>
                    {ALL_STATUS_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </>
                ) : (
                  (action === 'move' ? ALL_STATUS_OPTIONS : CREATE_STATUS_OPTIONS).map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))
                )}
              </select>
            </div>
          )}

          {showPriority && (
            <div className="space-y-1.5">
              <label className="text-[11px] uppercase tracking-wider text-muted">Priority</label>
              <select
                value={String(priority)}
                onChange={(e) => onUpdate({ priority: Number(e.target.value) })}
                className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
              >
                <option value="0">Low</option>
                <option value="1">Normal</option>
                <option value="2">High</option>
                <option value="3">Urgent</option>
              </select>
            </div>
          )}
        </div>
      )}

      {showLabels && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">
            Labels (comma or newline separated)
          </label>
          <textarea
            value={labelsText}
            onChange={(e) => onUpdate({ labelsText: e.target.value })}
            rows={3}
            placeholder="workflow, customer, {{trigger.data.channel}}"
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
          />
        </div>
      )}

      {showAssignee && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">
            {action === 'claim' ? 'Agent to claim with' : 'Assignee'}
          </label>
          <select
            value={assigneeAgentId}
            onChange={(e) => onUpdate({ assigneeAgentId: e.target.value })}
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
          >
            {action !== 'claim' && <option value="">Unassigned</option>}
            {projectAgents.map((agent) => (
              <option key={agent.id} value={agent.id}>
                {agent.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {showParent && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">
            Parent work item ID
          </label>
          <input
            value={parentWorkItemId}
            onChange={(e) => onUpdate({ parentWorkItemId: e.target.value })}
            placeholder="Optional parent card id or template"
            className="w-full bg-background border border-edge rounded px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono"
          />
        </div>
      )}

      {showReason && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Blocked reason</label>
          <textarea
            value={reasonTemplate}
            onChange={(e) => onUpdate({ reasonTemplate: e.target.value })}
            rows={4}
            placeholder="Waiting on {{agentNode.output.parsed.owner}}"
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
          />
        </div>
      )}

      {showBody && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Comment body</label>
          <textarea
            value={bodyTemplate}
            onChange={(e) => onUpdate({ bodyTemplate: e.target.value })}
            rows={5}
            placeholder="Summarized findings:\n{{agentNode.output.text}}"
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
          />
        </div>
      )}

      {showCommentAuthor && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">
            Comment author agent ID
          </label>
          <select
            value={commentAuthorAgentId}
            onChange={(e) => onUpdate({ commentAuthorAgentId: e.target.value })}
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
          >
            <option value="">Workflow user</option>
            {projectAgents.map((agent) => (
              <option key={agent.id} value={agent.id}>
                {agent.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {showListAssignee && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Assignee filter</label>
          <select
            value={listAssignee}
            onChange={(e) => onUpdate({ listAssignee: e.target.value })}
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
          >
            <option value="">Any assignee</option>
            <option value="none">Unassigned only</option>
            {projectAgents.map((agent) => (
              <option key={agent.id} value={agent.id}>
                {agent.name}
              </option>
            ))}
          </select>
        </div>
      )}

      {showLimit && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Result limit</label>
          <input
            type="number"
            min={1}
            max={500}
            value={limit}
            onChange={(e) => onUpdate({ limit: Number(e.target.value) || 25 })}
            className="w-full bg-background border border-edge rounded px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent"
          />
        </div>
      )}
    </div>
  );
}

function ProposalQueueInspector({
  data,
  projectId,
  onUpdate,
}: {
  data: Record<string, unknown>;
  projectId: string;
  onUpdate: (patch: Record<string, unknown>) => void;
}) {
  const { data: boardColumns = [] } = useQuery<ProjectBoardColumn[]>({
    queryKey: ['project-board-columns', projectId],
    queryFn: () => projectsApi.listBoardColumns(projectId),
  });
  const candidatesPath = (data.candidatesPath as string) ?? '';
  const reviewColumnId = (data.reviewColumnId as string) ?? '';
  const kind = ((data.kind as WorkItemKind | undefined) ?? 'task') as WorkItemKind;
  const priority = typeof data.priority === 'number' ? data.priority : 1;
  const labelsText = (data.labelsText as string) ?? '';

  return (
    <div className="space-y-3">
      <p className="text-[10px] text-muted">
        Expects an upstream array of proposal candidates. Point this at something like{' '}
        <span className="font-mono">agentNode.output.parsed</span>.
      </p>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Candidates path</label>
        <input
          value={candidatesPath}
          onChange={(e) => onUpdate({ candidatesPath: e.target.value })}
          placeholder="agentNode.output.parsed"
          className="w-full bg-background border border-edge rounded px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono"
        />
      </div>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Review column</label>
        <select
          value={reviewColumnId}
          onChange={(e) => onUpdate({ reviewColumnId: e.target.value })}
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
        >
          <option value="">Select column…</option>
          {boardColumns.map((column) => (
            <option key={column.id} value={column.id}>
              {column.name}
            </option>
          ))}
        </select>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Kind</label>
          <select
            value={kind}
            onChange={(e) => onUpdate({ kind: e.target.value })}
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
          >
            {WORK_ITEM_KIND_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </div>
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Priority</label>
          <select
            value={String(priority)}
            onChange={(e) => onUpdate({ priority: Number(e.target.value) })}
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
          >
            <option value="0">Low</option>
            <option value="1">Normal</option>
            <option value="2">High</option>
            <option value="3">Urgent</option>
          </select>
        </div>
      </div>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Labels</label>
        <textarea
          value={labelsText}
          onChange={(e) => onUpdate({ labelsText: e.target.value })}
          rows={2}
          placeholder="proposal-review, freelance"
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
        />
      </div>
    </div>
  );
}

function FeedFetchInspector({
  data,
  onUpdate,
}: {
  data: Record<string, unknown>;
  onUpdate: (patch: Record<string, unknown>) => void;
}) {
  const feedUrlsText = (data.feedUrlsText as string) ?? '';
  const limit = typeof data.limit === 'number' ? data.limit : 50;
  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Feed URLs</label>
        <textarea
          value={feedUrlsText}
          onChange={(e) => onUpdate({ feedUrlsText: e.target.value })}
          rows={6}
          placeholder={'https://example.com/jobs.xml\nhttps://example.com/feed'}
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
        />
      </div>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Per-feed limit</label>
        <input
          type="number"
          min={1}
          max={200}
          value={limit}
          onChange={(e) => onUpdate({ limit: Number(e.target.value) || 50 })}
          className="w-full bg-background border border-edge rounded px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent"
        />
      </div>
    </div>
  );
}

function HttpRequestInspector({
  data,
  onUpdate,
}: {
  data: Record<string, unknown>;
  onUpdate: (patch: Record<string, unknown>) => void;
}) {
  const url = (data.url as string) ?? '';
  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">URL template</label>
        <input
          value={url}
          onChange={(e) => onUpdate({ url: e.target.value, method: 'GET' })}
          placeholder="https://example.com/jobs/{{trigger.data.slug}}"
          className="w-full bg-background border border-edge rounded px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono"
        />
      </div>
      <p className="text-[10px] text-muted">
        V1 supports plain HTTP GET only. HTML responses are normalized into text, and JSON
        responses are exposed under <span className="font-mono">output.json</span>.
      </p>
    </div>
  );
}
