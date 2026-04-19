import { DragEvent, useCallback, useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  addEdge,
  applyEdgeChanges,
  applyNodeChanges,
  Background,
  BackgroundVariant,
  Connection,
  Controls,
  Edge,
  EdgeChange,
  MiniMap,
  Node,
  NodeChange,
  ReactFlow,
  ReactFlowProvider,
  reconnectEdge,
  useReactFlow,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { ArrowLeft, Play, Save } from 'lucide-react';
import { useUiStore } from '../../store/uiStore';
import { projectWorkflowsApi } from '../../api/projectWorkflows';
import { ProjectWorkflow, WorkflowEdge, WorkflowGraph, WorkflowNode } from '../../types';
import { nodeMeta, NODE_REGISTRY } from './nodeRegistry';
import { nodeTypes } from './nodes';
import { NodePalette } from './NodePalette';
import { NodeInspector } from './NodeInspector';

function workflowToFlow(graph: WorkflowGraph): { nodes: Node[]; edges: Edge[] } {
  return {
    nodes: graph.nodes.map((n) => ({
      id: n.id,
      type: n.type,
      position: n.position,
      data: n.data ?? {},
    })),
    edges: graph.edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      sourceHandle: e.sourceHandle ?? undefined,
      label:
        e.sourceHandle === 'true' || e.sourceHandle === 'false' ? e.sourceHandle : undefined,
      style:
        e.sourceHandle === 'true'
          ? { stroke: '#34d399' }
          : e.sourceHandle === 'false'
            ? { stroke: '#fb7185' }
            : undefined,
    })),
  };
}

function flowToWorkflow(nodes: Node[], edges: Edge[]): WorkflowGraph {
  return {
    schemaVersion: 1,
    nodes: nodes.map<WorkflowNode>((n) => ({
      id: n.id,
      type: n.type ?? 'unknown',
      position: { x: n.position.x, y: n.position.y },
      data: (n.data as Record<string, unknown>) ?? {},
    })),
    edges: edges.map<WorkflowEdge>((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      sourceHandle: e.sourceHandle ?? null,
    })),
  };
}

export function WorkflowEditor() {
  const { selectedWorkflowId } = useUiStore();
  if (!selectedWorkflowId) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">
        No workflow selected.
      </div>
    );
  }
  return (
    <ReactFlowProvider>
      <Editor workflowId={selectedWorkflowId} />
    </ReactFlowProvider>
  );
}

function Editor({ workflowId }: { workflowId: string }) {
  const { closeWorkflowEditor } = useUiStore();
  const queryClient = useQueryClient();
  const { screenToFlowPosition } = useReactFlow();

  const { data: workflow, isLoading } = useQuery<ProjectWorkflow>({
    queryKey: ['project-workflow', workflowId],
    queryFn: () => projectWorkflowsApi.get(workflowId),
  });

  const [nodes, setNodes] = useState<Node[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [enabled, setEnabled] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    if (!workflow) return;
    const { nodes: ns, edges: es } = workflowToFlow(workflow.graph);
    setNodes(ns);
    setEdges(es);
    setName(workflow.name);
    setDescription(workflow.description ?? '');
    setEnabled(workflow.enabled);
    setDirty(false);
  }, [workflow]);

  const selectedNode = useMemo(
    () => nodes.find((n) => n.id === selectedNodeId) ?? null,
    [nodes, selectedNodeId],
  );

  const markDirty = useCallback(() => setDirty(true), []);

  const onNodesChange = useCallback(
    (changes: NodeChange[]) => {
      setNodes((curr) => applyNodeChanges(changes, curr));
      const meaningful = changes.some(
        (c) => c.type !== 'select' && c.type !== 'dimensions',
      );
      if (meaningful) markDirty();
    },
    [markDirty],
  );

  const onEdgesChange = useCallback(
    (changes: EdgeChange[]) => {
      setEdges((curr) => applyEdgeChanges(changes, curr));
      const meaningful = changes.some((c) => c.type !== 'select');
      if (meaningful) markDirty();
    },
    [markDirty],
  );

  const onConnect = useCallback(
    (conn: Connection) => {
      const sourceNode = nodes.find((n) => n.id === conn.source);
      const isLogicIf = sourceNode?.type === 'logic.if';
      const handle = conn.sourceHandle ?? (isLogicIf ? 'true' : null);
      const newEdge: Edge = {
        id: `e_${crypto.randomUUID()}`,
        source: conn.source!,
        target: conn.target!,
        sourceHandle: handle ?? undefined,
        label: handle === 'true' || handle === 'false' ? handle : undefined,
        style:
          handle === 'true'
            ? { stroke: '#34d399' }
            : handle === 'false'
              ? { stroke: '#fb7185' }
              : undefined,
      };
      setEdges((curr) => addEdge(newEdge, curr));
      markDirty();
    },
    [markDirty, nodes],
  );

  const onReconnect = useCallback(
    (oldEdge: Edge, newConn: Connection) => {
      setEdges((curr) => reconnectEdge(oldEdge, newConn, curr));
      markDirty();
    },
    [markDirty],
  );

  const updateNodeData = useCallback(
    (nodeId: string, data: Record<string, unknown>) => {
      setNodes((curr) =>
        curr.map((n) => (n.id === nodeId ? { ...n, data } : n)),
      );
      markDirty();
    },
    [markDirty],
  );

  const deleteNode = useCallback(
    (nodeId: string) => {
      setNodes((curr) => curr.filter((n) => n.id !== nodeId));
      setEdges((curr) => curr.filter((e) => e.source !== nodeId && e.target !== nodeId));
      if (selectedNodeId === nodeId) setSelectedNodeId(null);
      markDirty();
    },
    [markDirty, selectedNodeId],
  );

  const onDrop = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      event.preventDefault();
      const type = event.dataTransfer.getData('application/orbit-node-type');
      if (!type) return;
      const meta = nodeMeta(type);
      const position = screenToFlowPosition({ x: event.clientX, y: event.clientY });
      const id = `n_${crypto.randomUUID()}`;
      const newNode: Node = {
        id,
        type,
        position,
        data: { ...(meta?.defaultData ?? {}) },
      };
      setNodes((curr) => curr.concat(newNode));
      markDirty();
    },
    [markDirty, screenToFlowPosition],
  );

  const onDragOver = useCallback((event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
  }, []);

  const saveMutation = useMutation({
    mutationFn: () =>
      projectWorkflowsApi.update(workflowId, {
        name: name.trim() || workflow!.name,
        description: description.trim() || null,
        graph: flowToWorkflow(nodes, edges),
      }),
    onSuccess: (updated) => {
      setSaveError(null);
      setDirty(false);
      queryClient.setQueryData(['project-workflow', workflowId], updated);
      queryClient.invalidateQueries({ queryKey: ['project-workflows', updated.projectId] });
    },
    onError: (err) => {
      setSaveError(String(err));
    },
  });

  const enabledMutation = useMutation({
    mutationFn: (next: boolean) => projectWorkflowsApi.setEnabled(workflowId, next),
    onSuccess: (updated) => {
      setEnabled(updated.enabled);
      queryClient.setQueryData(['project-workflow', workflowId], updated);
      queryClient.invalidateQueries({ queryKey: ['project-workflows', updated.projectId] });
    },
  });

  if (isLoading || !workflow) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">Loading…</div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <header className="flex items-center gap-3 px-4 py-3 border-b border-edge">
        <button
          onClick={closeWorkflowEditor}
          className="flex items-center gap-1 text-xs text-muted hover:text-white transition-colors"
        >
          <ArrowLeft size={14} />
          Back
        </button>
        <div className="h-5 w-px bg-edge" />
        <input
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setDirty(true);
          }}
          className="bg-transparent border-b border-transparent hover:border-edge focus:border-accent text-sm font-semibold text-white px-1 py-0.5 outline-none min-w-[200px]"
        />
        <input
          value={description}
          onChange={(e) => {
            setDescription(e.target.value);
            setDirty(true);
          }}
          placeholder="Description (optional)"
          className="bg-transparent border-b border-transparent hover:border-edge focus:border-accent text-xs text-muted px-1 py-0.5 outline-none flex-1 max-w-[420px]"
        />
        <span className="text-[10px] uppercase tracking-wider text-muted font-mono">
          v{workflow.version}
        </span>
        <div className="flex-1" />
        <label className="flex items-center gap-1.5 text-xs text-muted cursor-pointer">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => enabledMutation.mutate(e.target.checked)}
            className="accent-accent"
          />
          Enabled
        </label>
        <button
          disabled
          title="Run is available in Phase 4 (workflow runtime)"
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-edge text-muted text-xs font-medium cursor-not-allowed opacity-50"
        >
          <Play size={12} />
          Run
        </button>
        <button
          onClick={() => saveMutation.mutate()}
          disabled={!dirty || saveMutation.isPending}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Save size={12} />
          {saveMutation.isPending ? 'Saving…' : dirty ? 'Save' : 'Saved'}
        </button>
      </header>

      {saveError && (
        <div className="px-4 py-2 bg-red-500/10 border-b border-red-500/20 text-xs text-red-300">
          {saveError}
        </div>
      )}

      <div className="flex flex-1 min-h-0">
        <NodePalette />
        <div className="flex-1 relative" onDrop={onDrop} onDragOver={onDragOver}>
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onReconnect={onReconnect}
            onNodeClick={(_, node) => setSelectedNodeId(node.id)}
            onPaneClick={() => setSelectedNodeId(null)}
            fitView
            proOptions={{ hideAttribution: true }}
          >
            <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
            <Controls />
            <MiniMap pannable zoomable className="!bg-surface" />
          </ReactFlow>
          {nodes.length === 0 && (
            <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
              <div className="text-center text-muted text-sm">
                <p>Drag a node from the left palette to start.</p>
                <p className="text-[11px] mt-1 opacity-70">
                  {NODE_REGISTRY.length} node types available
                </p>
              </div>
            </div>
          )}
        </div>
        <NodeInspector
          node={selectedNode}
          onChangeData={updateNodeData}
          onDelete={deleteNode}
        />
      </div>
    </div>
  );
}
