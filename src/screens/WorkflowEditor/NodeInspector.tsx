import { useEffect, useMemo, useState } from 'react';
import type { ComponentPropsWithoutRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Node } from '@xyflow/react';
import { workflowRunsApi } from '../../api/workflowRuns';
import { agentsApi } from '../../api/agents';
import { projectsApi } from '../../api/projects';
import {
  Agent,
  ProjectBoardColumn,
  RuleGroup,
  RuleNode,
  WorkflowRunWithSteps,
  WorkItemKind,
  WorkItemStatus,
} from '../../types';
import { RecurringPicker } from '../ScheduleBuilder/RecurringPicker';
import {
  getObservedOutputHintEntries,
  getOutputReferenceLabel,
  getStaticOutputHintEntries,
  OutputHintEntry,
} from './outputHints';
import { getNodeReferenceKey, slugifyReferenceKey } from './nodeReferences';
import {
  OutputInsertionMode,
  OutputInsertionProvider,
  useOutputInsertion,
  useOutputInsertionField,
} from './outputInsertion';
import { nodeMeta } from './nodeRegistry';
import { RuleBuilder } from './RuleBuilder';
import { ruleToSentence } from './ruleSentence';
import { getWorkflowScheduleConfig } from './scheduleConfig';

interface Props {
  isOpen: boolean;
  node: Node | null;
  nodeHasLinkedOutputs: boolean;
  projectId: string;
  upstreamNodes: Node[];
  workflowId: string;
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

const TEMPLATE_FIELD_CLASSNAME =
  'w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono';

export function NodeInspector({
  isOpen,
  node,
  nodeHasLinkedOutputs,
  projectId,
  upstreamNodes,
  workflowId,
  onChangeData,
  onDelete,
}: Props) {
  const meta = node ? nodeMeta(node.type ?? '') : null;
  const data = normalizeData(node?.data);
  const update = (patch: Record<string, unknown>) => {
    if (!node) return;
    onChangeData(node.id, { ...data, ...patch });
  };
  const showOutputHelper = node ? nodeSupportsOutputReferences(node.type ?? '') : false;
  const storedReferenceKey = asString(data.referenceKey);
  const referenceKey =
    node?.type?.startsWith('trigger.')
      ? 'trigger'
      : storedReferenceKey;
  const [referenceKeyDraft, setReferenceKeyDraft] = useState(referenceKey);

  useEffect(() => {
    setReferenceKeyDraft(referenceKey);
  }, [node?.id, referenceKey]);

  const { data: latestRunDetail, isLoading: isLoadingLatestRun } = useQuery<WorkflowRunWithSteps | null>({
    queryKey: ['workflow-runs', workflowId, 'latest-output-hints'],
    queryFn: async () => {
      const runs = await workflowRunsApi.list(workflowId, 1);
      const latestRun = runs[0];
      if (!latestRun) {
        return null;
      }
      return workflowRunsApi.get(latestRun.id);
    },
    enabled: Boolean(node) && showOutputHelper && upstreamNodes.length > 0,
    staleTime: 30_000,
  });

  const observedOutputsByNodeId = useMemo(() => {
    const outputs = new Map<string, unknown>();
    for (const step of latestRunDetail?.steps ?? []) {
      outputs.set(step.nodeId, step.output);
    }
    return outputs;
  }, [latestRunDetail]);

  return (
    <OutputInsertionProvider>
      <aside
        className={`h-full w-80 border-l border-edge bg-background/50 overflow-y-auto transition-all duration-300 ease-out ${
          isOpen ? 'translate-x-0 opacity-100' : 'translate-x-6 opacity-0 pointer-events-none'
        }`}
      >
        {node ? (
          <>
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
              {!node.type?.startsWith('trigger.') && (
                <div className="space-y-1.5">
                  <label className="text-[11px] uppercase tracking-wider text-muted">Reference name</label>
                  <HintableInput
                    mode="raw"
                    value={referenceKeyDraft}
                    onValueChange={setReferenceKeyDraft}
                    onBlur={() => {
                      const nextValue = slugifyReferenceKey(referenceKeyDraft);
                      const committedValue = nextValue || (nodeHasLinkedOutputs ? referenceKey : '');
                      setReferenceKeyDraft(committedValue);
                      update({
                        referenceKey: committedValue,
                      });
                    }}
                    placeholder="run-agent-1"
                    className={TEMPLATE_FIELD_CLASSNAME}
                  />
                  <p className="text-[10px] text-muted">
                    {nodeHasLinkedOutputs ? (
                      <>
                        Use this name in templates and rules, for example{' '}
                        <span className="font-mono">{`{{${referenceKey}.output.text}}`}</span>.
                      </>
                    ) : (
                      'This node is not feeding any downstream nodes, so the reference name can be left empty.'
                    )}
                  </p>
                </div>
              )}

              {node.type === 'trigger.manual' && (
                <p className="text-xs text-muted">No configuration. Run from the editor toolbar.</p>
              )}

              {node.type === 'trigger.schedule' && (
                <ScheduleInspector data={data} onUpdate={update} />
              )}

              {node.type === 'agent.run' && <AgentRunInspector data={data} onUpdate={update} />}

              {node.type === 'logic.if' && <LogicIfInspector data={data} onUpdate={update} />}

              {node.type === 'board.work_item.create' && (
                <WorkItemInspector data={data} projectId={projectId} onUpdate={update} />
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

              {showOutputHelper && (
                <OutputReferencePanel
                  isLoadingLatestRun={isLoadingLatestRun}
                  latestRunDetail={latestRunDetail}
                  observedOutputsByNodeId={observedOutputsByNodeId}
                  upstreamNodes={upstreamNodes}
                />
              )}
            </div>
          </>
        ) : null}
      </aside>
    </OutputInsertionProvider>
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
  const agentId = asString(data.agentId);
  const promptTemplate = asString(data.promptTemplate);
  const contextTemplate = asString(data.contextTemplate);
  const outputMode = asString(data.outputMode) || 'text';

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
          {agents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.name}
            </option>
          ))}
        </select>
      </div>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Prompt template</label>
        <HintableTextarea
          value={promptTemplate}
          onValueChange={(value) => onUpdate({ promptTemplate: value })}
          rows={6}
          placeholder="Categorize this email: {{trigger.body}}"
          className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
        />
        <p className="text-[10px] text-muted">
          Use <span className="font-mono">{`{{trigger.body}}`}</span> or{' '}
          <span className="font-mono">{`{{run-agent-1.output.text}}`}</span> to reference upstream
          data.
        </p>
      </div>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Fit context</label>
        <HintableTextarea
          value={contextTemplate}
          onValueChange={(value) => onUpdate({ contextTemplate: value })}
          rows={5}
          placeholder="Candidate profile, writing preferences, exclusions, portfolio notes…"
          className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
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
  const trueLabel = asString(data.trueLabel) || 'true';
  const falseLabel = asString(data.falseLabel) || 'false';

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

  const action = (asString(data.action) || 'create') as WorkItemNodeAction;
  const itemIdTemplate = asString(data.itemIdTemplate);
  const titleTemplate = asString(data.titleTemplate);
  const descriptionTemplate = asString(data.descriptionTemplate);
  const columnId = asString(data.columnId);
  const kind = (asString(data.kind) || 'task') as WorkItemKind;
  const status = asString(data.status) as WorkItemStatus;
  const priorityValue = data.priority;
  const priority =
    typeof priorityValue === 'number' && Number.isFinite(priorityValue) ? priorityValue : 0;
  const labelsText = asString(data.labelsText);
  const assigneeAgentId = asString(data.assigneeAgentId);
  const parentWorkItemId = asString(data.parentWorkItemId);
  const reasonTemplate = asString(data.reasonTemplate);
  const bodyTemplate = asString(data.bodyTemplate);
  const commentAuthorAgentId = asString(data.commentAuthorAgentId);
  const listColumnId = asString(data.listColumnId);
  const listStatus = asString(data.listStatus);
  const listKind = (asString(data.listKind) || 'all') as string;
  const listAssignee = asString(data.listAssignee);
  const limitValue = data.limit;
  const limit = typeof limitValue === 'number' && Number.isFinite(limitValue) ? limitValue : 25;

  const showItemId = action !== 'create' && action !== 'list';
  const showTitle = action === 'create' || action === 'update';
  const showDescription = action === 'create' || action === 'update';
  const showColumn = action === 'create' || action === 'update' || action === 'move' || action === 'list';
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
        <span className="font-mono">{`{{run-agent-1.output.parsed.title}}`}</span>.
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
          <HintableInput
            value={itemIdTemplate}
            onValueChange={(value) => onUpdate({ itemIdTemplate: value })}
            placeholder="{{someNode.output.workItem.id}}"
            className={TEMPLATE_FIELD_CLASSNAME}
          />
        </div>
      )}

      {showTitle && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Title template</label>
          <HintableTextarea
            value={titleTemplate}
            onValueChange={(value) => onUpdate({ titleTemplate: value })}
            rows={3}
            placeholder="Follow up on {{agentNode.output.parsed.customerName}}"
            className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
          />
        </div>
      )}

      {showDescription && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">
            Description template
          </label>
          <HintableTextarea
            value={descriptionTemplate}
            onValueChange={(value) => onUpdate({ descriptionTemplate: value })}
            rows={6}
            placeholder={`Customer summary:\n{{agentNode.output.text}}`}
            className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
          />
        </div>
      )}

      {showColumn && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Board column</label>
          <select
            value={action === 'list' ? listColumnId : columnId}
            onChange={(e) =>
              onUpdate(action === 'list' ? { listColumnId: e.target.value } : { columnId: e.target.value })
            }
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
          >
            <option value="">{action === 'list' ? 'Any column' : 'Resolve from status/default'}</option>
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
                {action === 'list' ? 'Status filter' : 'Status'}
              </label>
              <select
                value={action === 'list' ? listStatus || 'all' : status || 'backlog'}
                onChange={(e) =>
                  onUpdate(
                    action === 'list' ? { listStatus: e.target.value } : { status: e.target.value },
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
          <HintableTextarea
            value={labelsText}
            onValueChange={(value) => onUpdate({ labelsText: value })}
            rows={3}
            placeholder="workflow, customer, {{trigger.data.channel}}"
            className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
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
          <HintableInput
            value={parentWorkItemId}
            onValueChange={(value) => onUpdate({ parentWorkItemId: value })}
            placeholder="Optional parent card id or template"
            className={TEMPLATE_FIELD_CLASSNAME}
          />
        </div>
      )}

      {showReason && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Blocked reason</label>
          <HintableTextarea
            value={reasonTemplate}
            onValueChange={(value) => onUpdate({ reasonTemplate: value })}
            rows={4}
            placeholder="Waiting on {{agentNode.output.parsed.owner}}"
            className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
          />
        </div>
      )}

      {showBody && (
        <div className="space-y-1.5">
          <label className="text-[11px] uppercase tracking-wider text-muted">Comment body</label>
          <HintableTextarea
            value={bodyTemplate}
            onValueChange={(value) => onUpdate({ bodyTemplate: value })}
            rows={5}
            placeholder="Summarized findings:\n{{agentNode.output.text}}"
            className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
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
  const candidatesPath = asString(data.candidatesPath);
  const reviewColumnId = asString(data.reviewColumnId);
  const kind = (asString(data.kind) || 'task') as WorkItemKind;
  const priority = typeof data.priority === 'number' ? data.priority : 1;
  const labelsText = asString(data.labelsText);

  return (
    <div className="space-y-3">
      <p className="text-[10px] text-muted">
        Expects an upstream array of proposal candidates. Point this at something like{' '}
        <span className="font-mono">run-agent-1.output.parsed</span>.
      </p>
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Candidates path</label>
        <HintableInput
          mode="raw"
          value={candidatesPath}
          onValueChange={(value) => onUpdate({ candidatesPath: value })}
          placeholder="agentNode.output.parsed"
          className={TEMPLATE_FIELD_CLASSNAME}
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
        <HintableTextarea
          value={labelsText}
          onValueChange={(value) => onUpdate({ labelsText: value })}
          rows={2}
          placeholder="proposal-review, freelance"
          className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
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
  const feedUrlsText = asString(data.feedUrlsText);
  const limit = typeof data.limit === 'number' ? data.limit : 50;
  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Feed URLs</label>
        <HintableTextarea
          value={feedUrlsText}
          onValueChange={(value) => onUpdate({ feedUrlsText: value })}
          rows={6}
          placeholder={'https://example.com/jobs.xml\nhttps://example.com/feed'}
          className={`${TEMPLATE_FIELD_CLASSNAME} resize-none`}
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
  const url = asString(data.url);
  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">URL template</label>
        <HintableInput
          value={url}
          onValueChange={(value) => onUpdate({ url: value, method: 'GET' })}
          placeholder="https://example.com/jobs/{{trigger.data.slug}}"
          className={TEMPLATE_FIELD_CLASSNAME}
        />
      </div>
      <p className="text-[10px] text-muted">
        V1 supports plain HTTP GET only. HTML responses are normalized into text, and JSON
        responses are exposed under <span className="font-mono">output.json</span>.
      </p>
    </div>
  );
}

function OutputReferencePanel({
  isLoadingLatestRun,
  latestRunDetail,
  observedOutputsByNodeId,
  upstreamNodes,
}: {
  isLoadingLatestRun: boolean;
  latestRunDetail: WorkflowRunWithSteps | null | undefined;
  observedOutputsByNodeId: Map<string, unknown>;
  upstreamNodes: Node[];
}) {
  const insertion = useOutputInsertion();
  const hasActiveField = insertion?.hasActiveField ?? false;
  const latestRunExists = latestRunDetail !== null && latestRunDetail !== undefined;

  return (
    <section className="space-y-3 rounded-xl border border-edge bg-surface/60 p-3">
      <div className="space-y-1">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-[11px] uppercase tracking-wider text-muted">Output references</h3>
          <span className="text-[10px] text-muted font-mono">
            {hasActiveField ? 'click to insert' : 'select a field first'}
          </span>
        </div>
        <p className="text-[10px] text-muted">
          Suggestions come from connected upstream nodes only. Template fields insert with braces;
          rule fields and raw path inputs insert the plain path.
        </p>
      </div>

      {upstreamNodes.length === 0 ? (
        <p className="text-[10px] text-muted">
          Connect at least one earlier node to see available outputs here.
        </p>
      ) : (
        <div className="space-y-3">
          {upstreamNodes.map((upstreamNode) => {
            const normalizedNode = {
              data: normalizeData(upstreamNode.data),
              id: upstreamNode.id,
              type: upstreamNode.type ?? 'unknown',
            };
            const referenceKey = getNodeReferenceKey(normalizedNode);
            const staticEntries = getStaticOutputHintEntries(normalizedNode);
            const observedEntries = getObservedOutputHintEntries(
              normalizedNode,
              observedOutputsByNodeId.get(upstreamNode.id),
            );

            return (
              <div key={upstreamNode.id} className="space-y-2 rounded-lg border border-edge/70 p-2">
                <div className="flex items-center justify-between gap-2">
                  <p className="text-[11px] font-medium text-white">
                    {getOutputReferenceLabel(normalizedNode)}
                  </p>
                  <span className="text-[10px] text-muted font-mono">{referenceKey}</span>
                </div>

                <ReferenceSection
                  entries={staticEntries}
                  label="Likely paths"
                  onInsert={(path) => insertion?.insertPath(path)}
                  disabled={!hasActiveField}
                />

                {observedEntries.length > 0 && (
                  <ReferenceSection
                    entries={observedEntries}
                    label="Latest run examples"
                    onInsert={(path) => insertion?.insertPath(path)}
                    disabled={!hasActiveField}
                  />
                )}
              </div>
            );
          })}
        </div>
      )}

      {isLoadingLatestRun && (
        <p className="text-[10px] text-muted">Loading examples from the latest workflow run…</p>
      )}

      {!isLoadingLatestRun && !latestRunExists && upstreamNodes.length > 0 && (
        <p className="text-[10px] text-muted">
          No recent workflow runs yet, so only static path hints are available.
        </p>
      )}
    </section>
  );
}

function ReferenceSection({
  entries,
  label,
  onInsert,
  disabled,
}: {
  entries: OutputHintEntry[];
  label: string;
  onInsert: (path: string) => void;
  disabled: boolean;
}) {
  return (
    <div className="space-y-1.5">
      <p className="text-[10px] uppercase tracking-wide text-muted">{label}</p>
      <div className="space-y-1.5">
        {entries.map((entry) => (
          <ReferenceButton
            key={`${label}:${entry.path}`}
            entry={entry}
            onInsert={onInsert}
            disabled={disabled}
          />
        ))}
      </div>
    </div>
  );
}

function ReferenceButton({
  entry,
  onInsert,
  disabled,
}: {
  entry: OutputHintEntry;
  onInsert: (path: string) => void;
  disabled: boolean;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => onInsert(entry.path)}
      className="w-full rounded-md border border-edge/70 bg-background/60 px-2 py-1.5 text-left transition-colors hover:border-accent/60 hover:bg-accent/5 disabled:cursor-not-allowed disabled:opacity-60"
    >
      <div className="text-[11px] text-white font-mono break-all">{entry.path}</div>
      {entry.description && (
        <div className="mt-1 text-[10px] text-muted">{entry.description}</div>
      )}
      {entry.preview && (
        <div className="mt-1 text-[10px] text-muted font-mono break-all">{entry.preview}</div>
      )}
    </button>
  );
}

function HintableInput({
  mode = 'template',
  onValueChange,
  value,
  ...props
}: Omit<ComponentPropsWithoutRef<'input'>, 'onChange' | 'value'> & {
  mode?: OutputInsertionMode;
  onValueChange: (value: string) => void;
  value: string;
}) {
  const binding = useOutputInsertionField<HTMLInputElement>({
    mode,
    onChange: onValueChange,
    value,
  });

  return (
    <input
      {...props}
      {...binding.bind}
      value={value}
      onChange={(event) => onValueChange(event.target.value)}
    />
  );
}

function HintableTextarea({
  mode = 'template',
  onValueChange,
  value,
  ...props
}: Omit<ComponentPropsWithoutRef<'textarea'>, 'onChange' | 'value'> & {
  mode?: OutputInsertionMode;
  onValueChange: (value: string) => void;
  value: string;
}) {
  const binding = useOutputInsertionField<HTMLTextAreaElement>({
    mode,
    onChange: onValueChange,
    value,
  });

  return (
    <textarea
      {...props}
      {...binding.bind}
      value={value}
      onChange={(event) => onValueChange(event.target.value)}
    />
  );
}

function nodeSupportsOutputReferences(type: string): boolean {
  return (
    type === 'agent.run' ||
    type === 'logic.if' ||
    type === 'board.work_item.create' ||
    type === 'board.proposal.enqueue' ||
    type === 'integration.feed.fetch' ||
    type === 'integration.http.request'
  );
}

function normalizeData(data: unknown): Record<string, unknown> {
  return data && typeof data === 'object' && !Array.isArray(data)
    ? (data as Record<string, unknown>)
    : {};
}

function asString(value: unknown): string {
  return typeof value === 'string' ? value : '';
}
