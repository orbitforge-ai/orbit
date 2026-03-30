import { useState, useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowRight,
  Plus,
  Trash2,
  Zap,
  MessageSquare,
  Radio,
} from 'lucide-react';
import * as Switch from '@radix-ui/react-switch';
import { busApi } from '../../api/bus';
import { agentsApi } from '../../api/agents';
import { tasksApi } from '../../api/tasks';
import { onBusMessageSent } from '../../events/runEvents';
import {
  Agent,
  Task,
  CreateBusSubscription,
} from '../../types';

interface BusTabProps {
  agentId: string;
}

export function BusTab({ agentId }: BusTabProps) {
  return (
    <div className="p-6 space-y-8 h-full overflow-y-auto">
      <SubscriptionsSection agentId={agentId} />
      <div className="border-t border-edge" />
      <ActivitySection agentId={agentId} />
    </div>
  );
}

// ─── Subscriptions Section ─────────────────────────────────────────────────

function SubscriptionsSection({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();
  const [showForm, setShowForm] = useState(false);

  const { data: subscriptions = [] } = useQuery({
    queryKey: ['bus-subscriptions', agentId],
    queryFn: () => busApi.listSubscriptions(agentId),
    refetchInterval: 10_000,
  });

  const { data: agents = [] } = useQuery({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  const { data: tasks = [] } = useQuery({
    queryKey: ['tasks'],
    queryFn: tasksApi.list,
  });

  async function handleToggle(id: string, enabled: boolean) {
    await busApi.toggleSubscription(id, enabled);
    queryClient.invalidateQueries({ queryKey: ['bus-subscriptions', agentId] });
  }

  async function handleDelete(id: string) {
    await busApi.deleteSubscription(id);
    queryClient.invalidateQueries({ queryKey: ['bus-subscriptions', agentId] });
  }

  function agentName(id: string) {
    return agents.find((a) => a.id === id)?.name ?? id;
  }

  function taskName(id: string) {
    return tasks.find((t) => t.id === id)?.name ?? id;
  }

  const eventLabels: Record<string, string> = {
    'run:completed': 'Completed',
    'run:failed': 'Failed',
    'run:any_terminal': 'Any Terminal',
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <Radio size={16} className="text-accent-hover" />
          <h4 className="text-sm font-semibold text-white">Event Subscriptions</h4>
        </div>
        <button
          onClick={() => setShowForm(!showForm)}
          className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
        >
          <Plus size={12} /> Add Subscription
        </button>
      </div>

      {showForm && (
        <NewSubscriptionForm
          agentId={agentId}
          agents={agents}
          tasks={tasks}
          onCreated={() => {
            setShowForm(false);
            queryClient.invalidateQueries({ queryKey: ['bus-subscriptions', agentId] });
          }}
          onCancel={() => setShowForm(false)}
        />
      )}

      {subscriptions.length === 0 && !showForm ? (
        <p className="text-sm text-muted">
          No subscriptions. Add one to auto-trigger this agent when another agent completes.
        </p>
      ) : (
        <div className="space-y-2">
          {subscriptions.map((sub) => (
            <div
              key={sub.id}
              className="flex items-center gap-3 px-4 py-3 rounded-lg border border-edge bg-surface"
            >
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 text-sm text-white">
                  <span className="font-medium">{agentName(sub.sourceAgentId)}</span>
                  <ArrowRight size={12} className="text-muted" />
                  <span className="font-medium">{agentName(sub.subscriberAgentId)}</span>
                </div>
                <div className="flex items-center gap-2 mt-1">
                  <span className="text-xs px-1.5 py-0.5 rounded bg-accent/20 text-accent-hover">
                    {eventLabels[sub.eventType] ?? sub.eventType}
                  </span>
                  <span className="text-xs text-muted">
                    triggers: {taskName(sub.taskId)}
                  </span>
                  {sub.maxChainDepth < 10 && (
                    <span className="text-xs text-muted">
                      max depth: {sub.maxChainDepth}
                    </span>
                  )}
                </div>
              </div>
              <Switch.Root
                checked={sub.enabled}
                onCheckedChange={(checked) => handleToggle(sub.id, checked)}
                className="w-9 h-5 rounded-full bg-surface-hover data-[state=checked]:bg-accent transition-colors"
              >
                <Switch.Thumb className="block w-4 h-4 rounded-full bg-white translate-x-0.5 data-[state=checked]:translate-x-[18px] transition-transform" />
              </Switch.Root>
              <button
                onClick={() => handleDelete(sub.id)}
                className="p-1.5 rounded hover:bg-red-500/10 text-muted hover:text-red-400 transition-colors"
              >
                <Trash2 size={14} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── New Subscription Form ─────────────────────────────────────────────────

function NewSubscriptionForm({
  agentId,
  agents,
  tasks,
  onCreated,
  onCancel,
}: {
  agentId: string;
  agents: Agent[];
  tasks: Task[];
  onCreated: () => void;
  onCancel: () => void;
}) {
  const [sourceAgentId, setSourceAgentId] = useState('');
  const [eventType, setEventType] = useState('run:completed');
  const [taskId, setTaskId] = useState('');
  const [maxChainDepth, setMaxChainDepth] = useState(10);
  const [saving, setSaving] = useState(false);

  // Filter tasks to those belonging to this agent
  const agentTasks = tasks.filter((t) => t.agentId === agentId);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!sourceAgentId || !taskId) return;

    setSaving(true);
    try {
      const payload: CreateBusSubscription = {
        subscriberAgentId: agentId,
        sourceAgentId,
        eventType,
        taskId,
        maxChainDepth,
      };
      await busApi.createSubscription(payload);
      onCreated();
    } finally {
      setSaving(false);
    }
  }

  return (
    <form onSubmit={handleSubmit} className="rounded-lg border border-edge bg-surface p-4 mb-4 space-y-3">
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="text-xs text-muted mb-1 block">When this agent finishes:</label>
          <select
            value={sourceAgentId}
            onChange={(e) => setSourceAgentId(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
          >
            <option value="">Select agent...</option>
            {agents
              .filter((a) => a.id !== agentId)
              .map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
          </select>
        </div>
        <div>
          <label className="text-xs text-muted mb-1 block">Event type:</label>
          <select
            value={eventType}
            onChange={(e) => setEventType(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
          >
            <option value="run:completed">Completed (success)</option>
            <option value="run:failed">Failed</option>
            <option value="run:any_terminal">Any terminal state</option>
          </select>
        </div>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="text-xs text-muted mb-1 block">Trigger this task:</label>
          <select
            value={taskId}
            onChange={(e) => setTaskId(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
          >
            <option value="">Select task...</option>
            {agentTasks.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="text-xs text-muted mb-1 block">Max chain depth:</label>
          <input
            type="number"
            min={1}
            max={50}
            value={maxChainDepth}
            onChange={(e) => setMaxChainDepth(Number(e.target.value))}
            className="w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent"
          />
        </div>
      </div>
      <div className="flex gap-2 pt-1">
        <button
          type="submit"
          disabled={saving || !sourceAgentId || !taskId}
          className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium"
        >
          {saving ? 'Creating...' : 'Create Subscription'}
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="px-3 py-1.5 rounded-lg text-muted hover:text-white text-xs"
        >
          Cancel
        </button>
      </div>
    </form>
  );
}

// ─── Activity Section ──────────────────────────────────────────────────────

function ActivitySection({ agentId }: { agentId: string }) {
  const queryClient = useQueryClient();

  const { data: messages = [] } = useQuery({
    queryKey: ['bus-messages', agentId],
    queryFn: () => busApi.listMessages(agentId, 30),
    refetchInterval: 10_000,
  });

  const { data: agents = [] } = useQuery({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  // Real-time updates
  useEffect(() => {
    const unlisten = onBusMessageSent(() => {
      queryClient.invalidateQueries({ queryKey: ['bus-messages', agentId] });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [agentId, queryClient]);

  function agentName(id: string) {
    return agents.find((a) => a.id === id)?.name ?? id;
  }

  return (
    <div>
      <div className="flex items-center gap-2 mb-4">
        <MessageSquare size={16} className="text-accent-hover" />
        <h4 className="text-sm font-semibold text-white">Bus Activity</h4>
      </div>

      {messages.length === 0 ? (
        <p className="text-sm text-muted">
          No bus activity yet. Messages will appear here when agents communicate.
        </p>
      ) : (
        <div className="space-y-2">
          {messages.map((msg) => (
            <div
              key={msg.id}
              className="flex items-center gap-3 px-4 py-3 rounded-lg border border-edge bg-surface"
            >
              <div
                className={`w-8 h-8 rounded-full flex items-center justify-center ${
                  msg.kind === 'direct'
                    ? 'bg-blue-500/20 text-blue-400'
                    : 'bg-purple-500/20 text-purple-400'
                }`}
              >
                {msg.kind === 'direct' ? (
                  <Zap size={14} />
                ) : (
                  <Radio size={14} />
                )}
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 text-sm">
                  <span className="font-medium text-white">
                    {agentName(msg.fromAgentId)}
                  </span>
                  <ArrowRight size={12} className="text-muted" />
                  <span className="font-medium text-white">
                    {agentName(msg.toAgentId)}
                  </span>
                  <span
                    className={`text-xs px-1.5 py-0.5 rounded ${
                      msg.kind === 'direct'
                        ? 'bg-blue-500/20 text-blue-400'
                        : 'bg-purple-500/20 text-purple-400'
                    }`}
                  >
                    {msg.kind}
                  </span>
                  {msg.eventType && (
                    <span className="text-xs text-muted">{msg.eventType}</span>
                  )}
                </div>
                {msg.payload && typeof msg.payload === 'object' && 'message' in msg.payload && (
                  <p className="text-xs text-muted mt-1 truncate max-w-md">
                    {String(msg.payload.message ?? '').slice(0, 120)}
                  </p>
                )}
              </div>
              <div className="text-right shrink-0">
                <span
                  className={`text-xs px-1.5 py-0.5 rounded ${
                    msg.status === 'delivered'
                      ? 'bg-green-500/20 text-green-400'
                      : 'bg-red-500/20 text-red-400'
                  }`}
                >
                  {msg.status}
                </span>
                <p className="text-xs text-muted mt-1">
                  {new Date(msg.createdAt).toLocaleTimeString()}
                </p>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
