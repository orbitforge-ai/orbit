import { Handle, NodeProps, Position } from '@xyflow/react';
import type { RuleNode } from '../../../types';
import { getNodeReferenceKey } from '../nodeReferences';
import { nodeMeta } from '../nodeRegistry';
import { ruleToSentence } from '../ruleSentence';
import { describeWorkflowSchedule } from '../scheduleConfig';

const NODE_BASE =
  'max-w-[320px] rounded-lg border bg-surface text-white text-xs shadow-sm min-w-[160px] ' +
  'transition-colors';

export function TriggerNode({ data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
  const description =
    type === 'trigger.schedule'
      ? describeWorkflowSchedule((data as Record<string, unknown>) ?? {})
      : (data as { description?: string }).description ?? 'Trigger';
  return (
    <div
      className={`${NODE_BASE} ${selected ? 'border-accent' : 'border-edge'}`}
    >
      <div className="flex items-center gap-2 px-3 py-2 border-b border-edge bg-accent/5">
        {Icon && <Icon size={12} className="text-accent-hover" />}
        <span className="font-semibold uppercase text-[10px] tracking-wider text-accent-hover">
          {meta?.label ?? type}
        </span>
      </div>
      <div className="px-3 py-2 text-muted">
        {description}
      </div>
      <Handle type="source" position={Position.Right} className="!bg-accent" />
    </div>
  );
}

export function AgentNode({ id, data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
  const d = data as { agentId?: string; promptTemplate?: string };
  const referenceKey = getNodeReferenceKey({ id, type, data: (data as Record<string, unknown>) ?? {} });
  return (
    <div className={`${NODE_BASE} ${selected ? 'border-accent' : 'border-edge'}`}>
      <Handle type="target" position={Position.Left} className="!bg-muted" />
      <div className="flex items-center gap-2 px-3 py-2 border-b border-edge bg-emerald-500/5">
        {Icon && <Icon size={12} className="text-emerald-300" />}
        <span className="font-semibold uppercase text-[10px] tracking-wider text-emerald-300">
          {meta?.label ?? type}
        </span>
      </div>
      <div className="px-3 py-2 space-y-1">
        <p className="text-muted text-[10px]">
          Ref: <span className="text-white font-mono">{referenceKey}</span>
        </p>
        <p className="text-muted">
          Agent: <span className="text-white font-mono">{d.agentId || '(unset)'}</span>
        </p>
        {d.promptTemplate && (
          <p className="text-muted text-[10px] italic whitespace-pre-wrap break-words">
            {d.promptTemplate}
          </p>
        )}
      </div>
      <Handle type="source" position={Position.Right} className="!bg-accent" />
    </div>
  );
}

export function WorkItemNode({ id, data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
  const d = data as {
    action?: string;
    itemIdTemplate?: string;
    titleTemplate?: string;
    kind?: string;
    status?: string;
    priority?: number;
    assigneeAgentId?: string;
    listColumn?: string;
    listStatus?: string;
    listKind?: string;
  };
  const action = d.action || 'create';
  const listColumn = d.listColumn || d.listStatus;
  const referenceKey = getNodeReferenceKey({ id, type, data: (data as Record<string, unknown>) ?? {} });

  const priorityLabel =
    d.priority === 3 ? 'urgent' : d.priority === 2 ? 'high' : d.priority === 1 ? 'normal' : 'low';

  const summary =
    action === 'create'
      ? d.titleTemplate?.trim() || '(title template required)'
      : action === 'list'
        ? `list ${listColumn && listColumn !== 'all' ? listColumn : 'all'} items`
        : d.itemIdTemplate?.trim() || '(work item id required)';

  return (
    <div className={`${NODE_BASE} ${selected ? 'border-accent' : 'border-edge'}`}>
      <Handle type="target" position={Position.Left} className="!bg-muted" />
      <div className="flex items-center gap-2 px-3 py-2 border-b border-edge bg-sky-500/5">
        {Icon && <Icon size={12} className="text-sky-300" />}
        <span className="font-semibold uppercase text-[10px] tracking-wider text-sky-300">
          {meta?.label ?? type}
        </span>
      </div>
      <div className="px-3 py-2 space-y-1">
        <p className="text-muted text-[10px]">
          Ref: <span className="text-white font-mono">{referenceKey}</span>
        </p>
        <p className="text-white text-[11px] whitespace-pre-wrap break-words">
          {summary}
        </p>
        <div className="flex items-center gap-2 text-[10px] text-muted font-mono">
          <span>{action}</span>
          {action === 'create' ? (
            <>
              <span>·</span>
              <span>{d.kind || 'task'}</span>
              <span>·</span>
              <span>{d.status || 'backlog'}</span>
              <span>·</span>
              <span>{priorityLabel}</span>
            </>
          ) : action === 'update' ? (
            <>
              <span>·</span>
              <span>{d.kind || 'task'}</span>
              <span>·</span>
              <span>{priorityLabel}</span>
            </>
          ) : action === 'list' ? (
            <>
              <span>·</span>
              <span>{d.listKind || 'all kinds'}</span>
            </>
          ) : null}
        </div>
        {d.assigneeAgentId && (
          <p className="text-muted text-[10px]">
            Assignee: <span className="text-white font-mono">{d.assigneeAgentId}</span>
          </p>
        )}
      </div>
      <Handle type="source" position={Position.Right} className="!bg-accent" />
    </div>
  );
}

export function ProposalQueueNode({ id, data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
  const d = data as { candidatesPath?: string; reviewColumnId?: string };
  const referenceKey = getNodeReferenceKey({ id, type, data: (data as Record<string, unknown>) ?? {} });
  return (
    <div className={`${NODE_BASE} ${selected ? 'border-accent' : 'border-edge'}`}>
      <Handle type="target" position={Position.Left} className="!bg-muted" />
      <div className="flex items-center gap-2 px-3 py-2 border-b border-edge bg-fuchsia-500/5">
        {Icon && <Icon size={12} className="text-fuchsia-300" />}
        <span className="font-semibold uppercase text-[10px] tracking-wider text-fuchsia-300">
          {meta?.label ?? type}
        </span>
      </div>
      <div className="px-3 py-2 space-y-1">
        <p className="text-muted text-[10px]">
          Ref: <span className="text-white font-mono">{referenceKey}</span>
        </p>
        <p className="text-muted text-[10px] whitespace-pre-wrap break-words">
          {d.candidatesPath || '(candidates path required)'}
        </p>
        <p className="text-muted text-[10px]">
          Review column: <span className="text-white font-mono">{d.reviewColumnId || '(unset)'}</span>
        </p>
      </div>
      <Handle type="source" position={Position.Right} className="!bg-accent" />
    </div>
  );
}

export function LogicIfNode({ id, data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
  const d = data as { rule?: RuleNode; trueLabel?: string; falseLabel?: string };
  const referenceKey = getNodeReferenceKey({ id, type, data: (data as Record<string, unknown>) ?? {} });
  const conditionSummary = ruleToSentence(d.rule) || '(no conditions)';
  return (
    <div className={`${NODE_BASE} ${selected ? 'border-accent' : 'border-edge'} min-w-[180px]`}>
      <Handle type="target" position={Position.Left} className="!bg-muted" />
      <div className="flex items-center gap-2 px-3 py-2 border-b border-edge bg-amber-500/5">
        {Icon && <Icon size={12} className="text-amber-300" />}
        <span className="font-semibold uppercase text-[10px] tracking-wider text-amber-300">
          {meta?.label ?? type}
        </span>
      </div>
      <div className="px-3 py-2 space-y-1.5">
        <div className="text-[10px] text-muted">
          Ref: <span className="text-white font-mono">{referenceKey}</span>
        </div>
        <p className="text-[11px] text-white/90 leading-relaxed whitespace-pre-wrap break-words">
          {conditionSummary}
        </p>
        <div className="flex items-center justify-between text-[10px]">
          <span className="text-muted">true →</span>
          <span className="text-emerald-300 font-mono">{d.trueLabel || 'true'}</span>
        </div>
        <div className="flex items-center justify-between text-[10px]">
          <span className="text-muted">false →</span>
          <span className="text-red-300 font-mono">{d.falseLabel || 'false'}</span>
        </div>
      </div>
      <Handle
        type="source"
        position={Position.Right}
        id="true"
        style={{ top: '60%' }}
        className="!bg-emerald-400"
      />
      <Handle
        type="source"
        position={Position.Right}
        id="false"
        style={{ top: '85%' }}
        className="!bg-red-400"
      />
    </div>
  );
}

export function IntegrationNode({ id, data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
  const referenceKey = getNodeReferenceKey({ id, type, data: (data as Record<string, unknown>) ?? {} });
  return (
    <div className={`${NODE_BASE} ${selected ? 'border-accent' : 'border-edge'} opacity-80`}>
      <Handle type="target" position={Position.Left} className="!bg-muted" />
      <div className="flex items-center gap-2 px-3 py-2 border-b border-edge bg-purple-500/5">
        {Icon && <Icon size={12} className="text-purple-300" />}
        <span className="font-semibold uppercase text-[10px] tracking-wider text-purple-300">
          {meta?.label ?? type}
        </span>
      </div>
      <div className="px-3 py-2">
        <p className="text-muted text-[10px] mb-1">
          Ref: <span className="text-white font-mono">{referenceKey}</span>
        </p>
        <span className="inline-block px-1.5 py-0.5 rounded bg-muted/15 text-[9px] uppercase tracking-wider text-muted font-mono">
          Integration
        </span>
      </div>
      <Handle type="source" position={Position.Right} className="!bg-muted" />
    </div>
  );
}

export const nodeTypes = {
  'trigger.manual': TriggerNode,
  'trigger.schedule': TriggerNode,
  'agent.run': AgentNode,
  'board.work_item.create': WorkItemNode,
  'board.proposal.enqueue': ProposalQueueNode,
  'logic.if': LogicIfNode,
  'integration.feed.fetch': IntegrationNode,
  'integration.gmail.read': IntegrationNode,
  'integration.gmail.send': IntegrationNode,
  'integration.slack.send': IntegrationNode,
  'integration.http.request': IntegrationNode,
};
