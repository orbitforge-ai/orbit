import { useCallback, useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Background,
  BackgroundVariant,
  Controls,
  Edge,
  Handle,
  MiniMap,
  Node,
  NodeProps,
  Position,
  ReactFlow,
  ReactFlowProvider,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import {
  AlertCircle,
  ArrowLeft,
  CheckCircle,
  Clock,
  ExternalLink,
  Loader2,
  StopCircle,
} from 'lucide-react';
import { workflowRunsApi } from '../../api/workflowRuns';
import {
  onWorkflowRunCreated,
  onWorkflowRunStep,
  onWorkflowRunUpdated,
} from '../../events/workflowRunEvents';
import {
  onAgentContentBlock,
  onAgentIteration,
  onAgentToolResult,
} from '../../events/runEvents';
import { useUiStore } from '../../store/uiStore';
import {
  AgentContentBlockPayload,
  WorkflowEdge,
  WorkflowNode,
  WorkflowRunAgentSession,
  WorkflowRunStep,
  WorkflowRunStatus,
  WorkflowRunStepStatus,
  WorkflowRunView,
} from '../../types';
import { DisplayBlock } from '../../components/chat/types';
import { chatMessagesToDisplay } from '../../components/chat/utils';
import { formatToolName, getToolVisual } from '../../components/chat/toolVisuals';
import {
  applyContentBlock,
  applyToolResult,
  createEmptyPreviewState,
  StreamPreviewState,
} from '../../store/streaming/streamReducer';
import { nodeMeta } from '../WorkflowEditor/nodeRegistry';

type ToolCall = Extract<DisplayBlock, { kind: 'tool_call' }>;

type RunNodeData = {
  originalType: string;
  originalData: Record<string, unknown>;
  status: WorkflowRunStepStatus | 'not_reached';
  step: WorkflowRunStep | null;
  triggerKind: string;
  triggerData: Record<string, unknown>;
  tools: ToolCall[];
};

type LiveToolState = {
  nodeId: string;
  preview: StreamPreviewState;
};

const RUN_GRAPH_HORIZONTAL_GAP = 520;
const RUN_GRAPH_VERTICAL_GAP = 210;

let liveMessageCounter = 0;
function nextLiveMessageId() {
  liveMessageCounter += 1;
  return `workflow-live-${liveMessageCounter}`;
}

function durationLabel(startedAt: string | null, completedAt: string | null): string {
  if (!startedAt) return '-';
  const end = completedAt ? new Date(completedAt).getTime() : Date.now();
  const start = new Date(startedAt).getTime();
  const ms = Math.max(0, end - start);
  if (ms < 1000) return `${ms}ms`;
  const seconds = Math.round(ms / 100) / 10;
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
}

function stringifyPreview(value: unknown, max = 96): string {
  if (value === null || value === undefined) return '-';
  const raw =
    typeof value === 'string'
      ? value
      : (() => {
          try {
            return JSON.stringify(value);
          } catch {
            return String(value);
          }
        })();
  return raw.length > max ? `${raw.slice(0, max - 1)}...` : raw;
}

function statusClasses(status: RunNodeData['status']) {
  switch (status) {
    case 'running':
      return 'border-blue-400/60 bg-blue-500/15 text-blue-200';
    case 'queued':
      return 'border-sky-400/50 bg-sky-500/10 text-sky-200';
    case 'success':
      return 'border-emerald-400/60 bg-emerald-500/15 text-emerald-200';
    case 'failed':
      return 'border-red-400/60 bg-red-500/15 text-red-200';
    case 'skipped':
      return 'border-muted/40 bg-muted/10 text-muted';
    default:
      return 'border-edge bg-background/80 text-muted';
  }
}

function StatusIcon({
  status,
  size = 13,
}: {
  status: WorkflowRunStatus | WorkflowRunStepStatus | 'not_reached';
  size?: number;
}) {
  if (status === 'running' || status === 'queued') {
    return <Loader2 size={size} className="animate-spin text-blue-300" />;
  }
  if (status === 'success') {
    return <CheckCircle size={size} className="text-emerald-300" />;
  }
  if (status === 'failed') {
    return <AlertCircle size={size} className="text-red-300" />;
  }
  if (status === 'cancelled' || status === 'skipped') {
    return <StopCircle size={size} className="text-muted" />;
  }
  return <Clock size={size} className="text-muted" />;
}

function toolStatus(tool: ToolCall): 'running' | 'success' | 'error' {
  if (!tool.result) return 'running';
  return tool.result.isError ? 'error' : 'success';
}

function collectSessionTools(session: WorkflowRunAgentSession | undefined): ToolCall[] {
  if (!session) return [];
  return chatMessagesToDisplay(session.messages)
    .flatMap((message) => message.blocks)
    .filter((block): block is ToolCall => block.kind === 'tool_call');
}

function collectLiveTools(liveState: LiveToolState | null, nodeId: string): ToolCall[] {
  if (!liveState || liveState.nodeId !== nodeId || !liveState.preview.previewMessage) return [];
  return liveState.preview.previewMessage.blocks.filter(
    (block): block is ToolCall => block.kind === 'tool_call',
  );
}

function mergeTools(persisted: ToolCall[], live: ToolCall[]): ToolCall[] {
  const byId = new Map<string, ToolCall>();
  for (const tool of persisted) byId.set(tool.id, tool);
  for (const tool of live) byId.set(tool.id, tool);
  return [...byId.values()];
}

function edgeLabel(edge: WorkflowEdge) {
  return edge.sourceHandle === 'true' || edge.sourceHandle === 'false'
    ? edge.sourceHandle
    : undefined;
}

function edgeStyle(edge: WorkflowEdge) {
  if (edge.sourceHandle === 'true') return { stroke: '#34d399' };
  if (edge.sourceHandle === 'false') return { stroke: '#fb7185' };
  return undefined;
}

function toFlowEdges(edges: WorkflowEdge[]): Edge[] {
  return edges.map((edge) => ({
    id: edge.id,
    source: edge.source,
    target: edge.target,
    sourceHandle: edge.sourceHandle ?? undefined,
    targetHandle: edge.targetHandle ?? undefined,
    label: edgeLabel(edge),
    style: edgeStyle(edge),
    animated: false,
  }));
}

function computeRunNodePositions(detail: WorkflowRunView): Map<string, { x: number; y: number }> {
  const nodes = detail.graphSnapshot.nodes;
  const positions = new Map<string, { x: number; y: number }>();
  if (nodes.length === 0) return positions;

  const nodeById = new Map(nodes.map((node) => [node.id, node]));
  const originalMinX = Math.min(...nodes.map((node) => node.position.x));
  const executedNodeIds = [...detail.steps]
    .sort((a, b) => a.sequence - b.sequence)
    .map((step) => step.nodeId)
    .filter((nodeId) => nodeById.has(nodeId));
  const baselineNodes =
    executedNodeIds.length > 0
      ? executedNodeIds.map((nodeId) => nodeById.get(nodeId)!)
      : nodes;
  const baselineY =
    baselineNodes.reduce((sum, node) => sum + node.position.y, 0) / baselineNodes.length;

  // The run cards are wider than editable workflow cards because they include
  // status and replay details, so the viewer lays out the executed path with
  // a dedicated readable gap instead of trusting the saved editor positions.
  executedNodeIds.forEach((nodeId, index) => {
    positions.set(nodeId, {
      x: originalMinX + index * RUN_GRAPH_HORIZONTAL_GAP,
      y: baselineY,
    });
  });

  const depthById = new Map<string, number>();
  executedNodeIds.forEach((nodeId, index) => depthById.set(nodeId, index));

  const trigger = nodes.find((node) => node.type.startsWith('trigger.')) ?? nodes[0];
  if (trigger && !depthById.has(trigger.id)) {
    depthById.set(trigger.id, 0);
  }

  let changed = true;
  while (changed) {
    changed = false;
    for (const edge of detail.graphSnapshot.edges) {
      const sourceDepth = depthById.get(edge.source);
      if (sourceDepth === undefined || depthById.has(edge.target)) continue;
      depthById.set(edge.target, sourceDepth + 1);
      changed = true;
    }
  }

  const fallbackOrder = [...nodes].sort((a, b) => a.position.x - b.position.x);
  fallbackOrder.forEach((node, index) => {
    if (!depthById.has(node.id)) depthById.set(node.id, index);
  });

  const siblingsByDepth = new Map<number, WorkflowNode[]>();
  for (const node of nodes) {
    if (positions.has(node.id)) continue;
    const depth = depthById.get(node.id) ?? 0;
    const siblings = siblingsByDepth.get(depth) ?? [];
    siblings.push(node);
    siblingsByDepth.set(depth, siblings);
  }

  siblingsByDepth.forEach((siblings, depth) => {
    const depthHasMainNode = executedNodeIds.some((nodeId) => depthById.get(nodeId) === depth);
    siblings
      .sort((a, b) => a.position.y - b.position.y)
      .forEach((node, index) => {
        const lane = depthHasMainNode ? index + 1 : index - (siblings.length - 1) / 2;
        const direction = depthHasMainNode && index % 2 === 1 ? -1 : 1;
        positions.set(node.id, {
          x: originalMinX + depth * RUN_GRAPH_HORIZONTAL_GAP,
          y: baselineY + lane * direction * RUN_GRAPH_VERTICAL_GAP,
        });
      });
  });

  return positions;
}

function buildFlowNodes(detail: WorkflowRunView, liveState: LiveToolState | null): Node<RunNodeData>[] {
  const stepByNodeId = new Map(detail.steps.map((step) => [step.nodeId, step]));
  const sessionByNodeId = new Map(detail.agentSessions.map((session) => [session.nodeId, session]));
  const positions = computeRunNodePositions(detail);

  return detail.graphSnapshot.nodes.map((node: WorkflowNode) => {
    const step = stepByNodeId.get(node.id) ?? null;
    const persistedTools = collectSessionTools(sessionByNodeId.get(node.id));
    const liveTools = collectLiveTools(liveState, node.id);
    return {
      id: node.id,
      type: 'workflowRunNode',
      position: positions.get(node.id) ?? node.position,
      data: {
        originalType: node.type,
        originalData: node.data ?? {},
        status: step?.status ?? 'not_reached',
        step,
        triggerKind: detail.triggerKind,
        triggerData: detail.triggerData,
        tools: mergeTools(persistedTools, liveTools),
      },
      draggable: false,
      selectable: true,
    };
  });
}

function WorkflowRunNode({ data, selected }: NodeProps<Node<RunNodeData>>) {
  const meta = nodeMeta(data.originalType);
  const Icon = meta?.icon;
  const isTrigger = data.originalType.startsWith('trigger.');
  const isAgent = data.originalType === 'agent.run';

  return (
    <div
      className={`relative min-w-[190px] max-w-[340px] rounded-lg border bg-surface text-xs text-white shadow-sm ${
        selected ? 'border-accent' : 'border-edge'
      }`}
    >
      <Handle type="target" position={Position.Left} className="!opacity-0" />
      <div className="flex items-center gap-2 border-b border-edge bg-background/30 px-3 py-2">
        {Icon ? <Icon size={13} className="text-accent-hover" /> : null}
        <span className="min-w-0 flex-1 truncate text-[10px] font-semibold uppercase tracking-wider text-muted">
          {meta?.label ?? data.originalType}
        </span>
        <span
          className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] ${statusClasses(
            data.status,
          )}`}
        >
          <StatusIcon status={data.status} size={10} />
          {data.status === 'not_reached' ? 'not reached' : data.status}
        </span>
      </div>
      <div className="space-y-2 px-3 py-2">
        <p className="font-mono text-[10px] text-muted">{data.step?.nodeId ?? 'pending'}</p>
        {data.step ? (
          <p className="text-[10px] text-muted">
            {durationLabel(data.step.startedAt, data.step.completedAt)}
            {data.step.error ? <span className="ml-2 text-red-300">failed</span> : null}
          </p>
        ) : (
          <p className="text-[10px] text-muted">Waiting for this path to execute.</p>
        )}
      </div>

      {isTrigger ? (
        <ConnectedBubble side="right">
          <span className="font-medium text-white">{data.triggerKind}</span>
          <span className="truncate text-muted">{stringifyPreview(data.triggerData, 64)}</span>
        </ConnectedBubble>
      ) : null}

      {isAgent && data.tools.length > 0 ? (
        <ConnectedBubble side="bottom">
          <div className="flex max-w-[300px] flex-wrap gap-1">
            {data.tools.slice(0, 6).map((tool) => (
              <ToolPill key={tool.id} tool={tool} compact />
            ))}
            {data.tools.length > 6 ? (
              <span className="rounded-full border border-edge bg-background px-2 py-1 text-[10px] text-muted">
                +{data.tools.length - 6}
              </span>
            ) : null}
          </div>
        </ConnectedBubble>
      ) : null}

      <Handle type="source" position={Position.Right} className="!opacity-0" />
    </div>
  );
}

function ConnectedBubble({
  children,
  side,
}: {
  children: ReactNode;
  side: 'right' | 'bottom';
}) {
  if (side === 'right') {
    return (
      <div className="absolute left-full top-1/2 ml-4 flex -translate-y-1/2 items-center">
        <span className="absolute -left-4 h-px w-4 bg-edge" />
        <div className="flex min-w-[150px] max-w-[240px] flex-col gap-0.5 rounded-full border border-edge bg-background px-3 py-1.5 text-[10px] shadow-lg">
          {children}
        </div>
      </div>
    );
  }

  return (
    <div className="absolute left-3 top-full mt-4">
      <span className="absolute -top-4 left-6 h-4 w-px bg-edge" />
      <div className="rounded-xl border border-edge bg-background/95 px-2 py-2 shadow-lg">
        {children}
      </div>
    </div>
  );
}

function ToolPill({ tool, compact = false }: { tool: ToolCall; compact?: boolean }) {
  const visual = getToolVisual(tool.name);
  const status = toolStatus(tool);
  const statusClass =
    status === 'running'
      ? 'border-blue-400/40 bg-blue-500/10'
      : status === 'error'
        ? 'border-red-400/50 bg-red-500/10'
        : 'border-emerald-400/40 bg-emerald-500/10';

  return (
    <span
      className={`inline-flex max-w-full items-center gap-1.5 rounded-full border px-2 py-1 text-[10px] text-white ${statusClass}`}
      title={formatToolName(tool.name)}
    >
      <visual.Icon size={compact ? 10 : 12} className={`${visual.colorClass} shrink-0`} />
      <span className="truncate">{formatToolName(tool.name)}</span>
      <StatusIcon status={status === 'error' ? 'failed' : status} size={9} />
    </span>
  );
}

function PayloadBlock({ label, value }: { label: string; value: unknown }) {
  const text = useMemo(() => {
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }, [value]);

  return (
    <div className="rounded-md border border-edge bg-background/50">
      <div className="border-b border-edge px-3 py-2 text-[10px] uppercase tracking-wider text-muted">
        {label}
      </div>
      <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words px-3 py-2 font-mono text-[11px] text-muted">
        {text}
      </pre>
    </div>
  );
}

function NodeDetail({
  node,
  agentSession,
}: {
  node: Node<RunNodeData> | null;
  agentSession: WorkflowRunAgentSession | undefined;
}) {
  if (!node) {
    return (
      <div className="p-4 text-sm text-muted">
        Select a node to inspect its run data.
      </div>
    );
  }

  const data = node.data;
  return (
    <div className="space-y-4 p-4">
      <div>
        <div className="flex items-center gap-2">
          <StatusIcon status={data.status} size={16} />
          <h2 className="truncate text-sm font-semibold text-white">
            {nodeMeta(data.originalType)?.label ?? data.originalType}
          </h2>
        </div>
        <p className="mt-1 font-mono text-[11px] text-muted">{node.id}</p>
      </div>

      {data.step ? (
        <div className="grid grid-cols-2 gap-3 text-xs">
          <MetaRow label="Status" value={data.step.status} />
          <MetaRow label="Duration" value={durationLabel(data.step.startedAt, data.step.completedAt)} />
          <MetaRow
            label="Started"
            value={data.step.startedAt ? new Date(data.step.startedAt).toLocaleString() : '-'}
          />
          <MetaRow
            label="Completed"
            value={data.step.completedAt ? new Date(data.step.completedAt).toLocaleString() : '-'}
          />
        </div>
      ) : (
        <div className="rounded-md border border-edge bg-background/50 px-3 py-2 text-xs text-muted">
          This node has not been reached in this run.
        </div>
      )}

      {data.step?.error ? (
        <div className="rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
          {data.step.error}
        </div>
      ) : null}

      {data.originalType.startsWith('trigger.') ? (
        <PayloadBlock label="Trigger data" value={data.triggerData} />
      ) : null}

      {data.tools.length > 0 ? (
        <div className="space-y-2">
          <h3 className="text-[10px] uppercase tracking-wider text-muted">
            Agent tools ({data.tools.length})
          </h3>
          <div className="flex flex-wrap gap-1.5">
            {data.tools.map((tool) => (
              <ToolPill key={tool.id} tool={tool} />
            ))}
          </div>
          {data.tools.map((tool) => (
            <div key={`detail-${tool.id}`} className="rounded-md border border-edge bg-background/50">
              <div className="flex items-center gap-2 border-b border-edge px-3 py-2 text-xs text-white">
                <ToolPill tool={tool} compact />
              </div>
              <PayloadBlock label="Input" value={tool.input} />
              {tool.result ? (
                <PayloadBlock label={tool.result.isError ? 'Error result' : 'Result'} value={tool.result.content} />
              ) : null}
            </div>
          ))}
        </div>
      ) : data.originalType === 'agent.run' ? (
        <div className="rounded-md border border-edge bg-background/50 px-3 py-2 text-xs text-muted">
          {agentSession ? 'No tool calls recorded for this agent run.' : 'No linked agent session yet.'}
        </div>
      ) : null}

      {data.step ? (
        <>
          <PayloadBlock label="Input" value={data.step.input} />
          {data.step.output !== null && data.step.output !== undefined ? (
            <PayloadBlock label="Output" value={data.step.output} />
          ) : null}
        </>
      ) : null}
    </div>
  );
}

function MetaRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-[10px] uppercase tracking-wider text-muted">{label}</div>
      <div className="mt-0.5 text-white">{value}</div>
    </div>
  );
}

const nodeTypes = { workflowRunNode: WorkflowRunNode };

function ViewerInner({ runId }: { runId: string }) {
  const queryClient = useQueryClient();
  const { closeWorkflowRunViewer, openWorkflowEditor } = useUiStore();
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [liveState, setLiveState] = useState<LiveToolState | null>(null);

  const { data: detail, isLoading } = useQuery<WorkflowRunView>({
    queryKey: ['workflow-run-view', runId],
    queryFn: () => workflowRunsApi.getView(runId),
    refetchInterval: (query) => {
      const status = query.state.data?.status;
      return status === 'queued' || status === 'running' ? 2000 : false;
    },
  });

  const activeAgentNodeId = useMemo(() => {
    return (
      detail?.steps.find(
        (step) => step.status === 'running' && step.nodeType === 'agent.run',
      )?.nodeId ?? null
    );
  }, [detail?.steps]);

  const invalidateRunView = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ['workflow-run-view', runId] });
    queryClient.invalidateQueries({ queryKey: ['workflow-run', runId] });
  }, [queryClient, runId]);

  useEffect(() => {
    const unsubs = [
      onWorkflowRunCreated((payload) => {
        if (payload.runId === runId) invalidateRunView();
      }),
      onWorkflowRunUpdated((payload) => {
        if (payload.runId !== runId) return;
        invalidateRunView();
      }),
      onWorkflowRunStep((payload) => {
        if (payload.runId === runId) invalidateRunView();
      }),
      onAgentContentBlock((payload) => {
        if (payload.runId !== runId || !activeAgentNodeId) return;
        setLiveState((current) => {
          const base =
            current?.nodeId === activeAgentNodeId ? current.preview : createEmptyPreviewState();
          return {
            nodeId: activeAgentNodeId,
            preview: applyContentBlock(base, payload as AgentContentBlockPayload, nextLiveMessageId),
          };
        });
      }),
      onAgentToolResult((payload) => {
        if (payload.runId !== runId || !activeAgentNodeId) return;
        setLiveState((current) => {
          const base =
            current?.nodeId === activeAgentNodeId ? current.preview : createEmptyPreviewState();
          return {
            nodeId: activeAgentNodeId,
            preview: applyToolResult(
              [],
              base,
              payload.toolUseId,
              payload.content,
              payload.isError,
            ).state,
          };
        });
      }),
      onAgentIteration((payload) => {
        if (payload.runId !== runId) return;
        if (payload.action === 'finished') {
          invalidateRunView();
        }
      }),
    ];

    return () => {
      unsubs.forEach((unsub) => {
        unsub.then((cleanup) => cleanup()).catch(() => {});
      });
    };
  }, [activeAgentNodeId, invalidateRunView, runId]);

  const nodes = useMemo(() => (detail ? buildFlowNodes(detail, liveState) : []), [detail, liveState]);
  const edges = useMemo(() => (detail ? toFlowEdges(detail.graphSnapshot.edges) : []), [detail]);
  const selectedNode = useMemo(
    () => nodes.find((node) => node.id === selectedNodeId) ?? null,
    [nodes, selectedNodeId],
  );
  const selectedAgentSession = useMemo(
    () => detail?.agentSessions.find((session) => session.nodeId === selectedNodeId),
    [detail?.agentSessions, selectedNodeId],
  );

  const cancelMutation = useMutation({
    mutationFn: () => workflowRunsApi.cancel(runId),
    onSuccess: invalidateRunView,
  });

  if (isLoading || !detail) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted">
        Loading workflow run...
      </div>
    );
  }

  const isActive = detail.status === 'queued' || detail.status === 'running';

  return (
    <div className="flex h-full flex-col bg-background">
      <header className="flex items-center gap-3 border-b border-edge px-4 py-3">
        <button
          onClick={closeWorkflowRunViewer}
          className="flex items-center gap-1 text-xs text-muted transition-colors hover:text-white"
        >
          <ArrowLeft size={14} />
          Back
        </button>
        <div className="h-5 w-px bg-edge" />
        <StatusIcon status={detail.status} size={16} />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h1 className="truncate text-sm font-semibold text-white">Workflow run</h1>
            <span className="font-mono text-[10px] text-muted">{detail.id.slice(-12)}</span>
          </div>
          <div className="flex items-center gap-2 text-[10px] text-muted">
            <span className="capitalize">{detail.status}</span>
            <span>·</span>
            <span>{detail.triggerKind}</span>
            <span>·</span>
            <span>{durationLabel(detail.startedAt, detail.completedAt)}</span>
          </div>
        </div>
        <div className="flex-1" />
        {isActive ? (
          <button
            onClick={() => cancelMutation.mutate()}
            disabled={cancelMutation.isPending}
            className="flex items-center gap-1.5 rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-1.5 text-xs font-medium text-red-300 transition-colors hover:bg-red-500/20 disabled:opacity-50"
          >
            <StopCircle size={12} />
            {cancelMutation.isPending ? 'Cancelling...' : 'Cancel'}
          </button>
        ) : null}
        <button
          onClick={() => openWorkflowEditor(detail.workflowId)}
          className="flex items-center gap-1.5 rounded-lg border border-edge px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-edge/30"
        >
          <ExternalLink size={12} />
          Open workflow
        </button>
      </header>

      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            nodesDraggable={false}
            nodesConnectable={false}
            elementsSelectable
            onNodeClick={(_, node) => setSelectedNodeId(node.id)}
            onPaneClick={() => setSelectedNodeId(null)}
            fitView
            fitViewOptions={{ padding: 0.25 }}
            proOptions={{ hideAttribution: true }}
          >
            <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
            <Controls />
            <MiniMap pannable zoomable className="!bg-surface" />
          </ReactFlow>
        </div>
        <aside className="w-[380px] shrink-0 overflow-y-auto border-l border-edge bg-panel">
          <NodeDetail node={selectedNode} agentSession={selectedAgentSession} />
        </aside>
      </div>
    </div>
  );
}

export function WorkflowRunViewer() {
  const runId = useUiStore((state) => state.selectedWorkflowRunId);
  if (!runId) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted">
        No workflow run selected.
      </div>
    );
  }

  return (
    <ReactFlowProvider>
      <ViewerInner runId={runId} />
    </ReactFlowProvider>
  );
}
