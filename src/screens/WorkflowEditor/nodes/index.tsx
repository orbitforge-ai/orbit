import { Handle, NodeProps, Position } from '@xyflow/react';
import { nodeMeta } from '../nodeRegistry';

const NODE_BASE =
  'rounded-lg border bg-surface text-white text-xs shadow-sm min-w-[160px] ' +
  'transition-colors';

export function TriggerNode({ data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
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
        {(data as { description?: string }).description ?? 'Trigger'}
      </div>
      <Handle type="source" position={Position.Right} className="!bg-accent" />
    </div>
  );
}

export function AgentNode({ data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
  const d = data as { agentId?: string; promptTemplate?: string };
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
        <p className="text-muted">
          Agent: <span className="text-white font-mono">{d.agentId || '(unset)'}</span>
        </p>
        {d.promptTemplate && (
          <p className="text-muted text-[10px] line-clamp-2 italic">{d.promptTemplate}</p>
        )}
      </div>
      <Handle type="source" position={Position.Right} className="!bg-accent" />
    </div>
  );
}

export function LogicIfNode({ data, type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
  const d = data as { trueLabel?: string; falseLabel?: string };
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

export function IntegrationNode({ type, selected }: NodeProps) {
  const meta = nodeMeta(type);
  const Icon = meta?.icon;
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
        <span className="inline-block px-1.5 py-0.5 rounded bg-muted/15 text-[9px] uppercase tracking-wider text-muted font-mono">
          Coming soon
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
  'logic.if': LogicIfNode,
  'integration.gmail.read': IntegrationNode,
  'integration.gmail.send': IntegrationNode,
  'integration.slack.send': IntegrationNode,
  'integration.http.request': IntegrationNode,
};
