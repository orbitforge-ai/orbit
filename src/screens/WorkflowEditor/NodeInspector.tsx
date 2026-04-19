import { useQuery } from '@tanstack/react-query';
import { Node } from '@xyflow/react';
import { agentsApi } from '../../api/agents';
import { projectsApi } from '../../api/projects';
import { Agent, RuleGroup, RuleNode, WorkItemKind, WorkItemStatus } from '../../types';
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
          <WorkItemCreateInspector
            data={data}
            projectId={projectId}
            onUpdate={update}
          />
        )}

        {node.type?.startsWith('integration.') && (
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

function WorkItemCreateInspector({
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

  const titleTemplate = (data.titleTemplate as string) ?? '';
  const descriptionTemplate = (data.descriptionTemplate as string) ?? '';
  const kind = ((data.kind as WorkItemKind | undefined) ?? 'task') as WorkItemKind;
  const status =
    ((data.status as Exclude<WorkItemStatus, 'blocked'> | undefined) ?? 'backlog') as Exclude<
      WorkItemStatus,
      'blocked'
    >;
  const priorityValue = data.priority;
  const priority =
    typeof priorityValue === 'number' && Number.isFinite(priorityValue) ? priorityValue : 0;
  const labelsText = (data.labelsText as string) ?? '';
  const assigneeAgentId = (data.assigneeAgentId as string) ?? '';
  const parentWorkItemId = (data.parentWorkItemId as string) ?? '';

  return (
    <div className="space-y-3">
      <p className="text-[10px] text-muted">
        Creates a board card in this workflow&apos;s project. Template fields can reference earlier
        node outputs like <span className="font-mono">{`{{trigger.data.subject}}`}</span> or{' '}
        <span className="font-mono">{`{{nodeId.output.parsed.title}}`}</span>.
      </p>

      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Title template</label>
        <textarea
          value={titleTemplate}
          onChange={(e) => onUpdate({ titleTemplate: e.target.value })}
          rows={3}
          placeholder="Follow up on {{agentNode.output.parsed.customerName}}"
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono resize-none"
        />
      </div>

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

      <div className="grid grid-cols-3 gap-2">
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
          <label className="text-[11px] uppercase tracking-wider text-muted">Status</label>
          <select
            value={status}
            onChange={(e) => onUpdate({ status: e.target.value })}
            className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
          >
            {CREATE_STATUS_OPTIONS.map((option) => (
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

      <div className="space-y-1.5">
        <label className="text-[11px] uppercase tracking-wider text-muted">Assignee</label>
        <select
          value={assigneeAgentId}
          onChange={(e) => onUpdate({ assigneeAgentId: e.target.value })}
          className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white outline-none focus:border-accent"
        >
          <option value="">Unassigned</option>
          {projectAgents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.name}
            </option>
          ))}
        </select>
      </div>

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
    </div>
  );
}
