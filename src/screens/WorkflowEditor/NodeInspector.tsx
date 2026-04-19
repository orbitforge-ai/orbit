import { useQuery } from '@tanstack/react-query';
import { Node } from '@xyflow/react';
import { agentsApi } from '../../api/agents';
import { Agent, RuleGroup, RuleNode } from '../../types';
import { nodeMeta } from './nodeRegistry';
import { RuleBuilder } from './RuleBuilder';
import { ruleToSentence } from './ruleSentence';

interface Props {
  node: Node | null;
  onChangeData: (nodeId: string, data: Record<string, unknown>) => void;
  onDelete: (nodeId: string) => void;
}

export function NodeInspector({ node, onChangeData, onDelete }: Props) {
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
  const cron = (data.cron as string) ?? '';
  return (
    <div className="space-y-2">
      <label className="text-[11px] uppercase tracking-wider text-muted">Cron expression</label>
      <input
        value={cron}
        onChange={(e) => onUpdate({ cron: e.target.value })}
        placeholder="0 * * * *"
        className="w-full bg-background border border-edge rounded-lg px-2 py-1.5 text-xs text-white placeholder-muted outline-none focus:border-accent font-mono"
      />
      <p className="text-[10px] text-muted">Standard cron — minute hour day month weekday.</p>
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
