import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  DndContext,
  DragEndEvent,
  DragOverlay,
  DragStartEvent,
  pointerWithin,
  useDroppable,
} from '@dnd-kit/core';
import {
  addEdge,
  applyEdgeChanges,
  applyNodeChanges,
  Background,
  BackgroundVariant,
  Connection,
  FinalConnectionState,
  Controls,
  Edge,
  EdgeChange,
  MiniMap,
  Node,
  NodeChange,
  OnConnectStartParams,
  ReactFlow,
  ReactFlowProvider,
  reconnectEdge,
  XYPosition,
  useReactFlow,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { ArrowLeft, History, Play, Save, X } from 'lucide-react';
import { useUiStore } from '../../store/uiStore';
import { projectWorkflowsApi } from '../../api/projectWorkflows';
import { workflowRunsApi } from '../../api/workflowRuns';
import {
  onWorkflowRunCreated,
  onWorkflowRunStep,
  onWorkflowRunUpdated,
} from '../../events/workflowRunEvents';
import { ProjectWorkflow, WorkflowEdge, WorkflowGraph, WorkflowNode } from '../../types';
import { edgeTypes } from './edges';
import { nodeMeta, NODE_REGISTRY } from './nodeRegistry';
import {
  ensureFlowNodeReferenceKeysForGraph,
  ensureWorkflowNodeReferenceKeys,
  generateReferenceKeyForNewNode,
  nodeHasLinkedOutputs,
} from './nodeReferences';
import { nodeTypes } from './nodes';
import { NodePalette } from './NodePalette';
import { NodeInspector } from './NodeInspector';
import { RunHistoryDrawer } from './RunHistoryDrawer';
import {
  parseWorkflowNodeDraggableId,
  WORKFLOW_CANVAS_DROPPABLE_ID,
} from './dnd';
import { useAgentDndSensors } from '../../components/dnd/agentDnd';
import { Checkbox, Input } from '../../components/ui';

function toFlowEdge(edge: WorkflowEdge): Edge {
  return {
    id: edge.id,
    type: 'deletable',
    source: edge.source,
    target: edge.target,
    sourceHandle: edge.sourceHandle ?? undefined,
    targetHandle: edge.targetHandle ?? undefined,
    label:
      edge.sourceHandle === 'true' || edge.sourceHandle === 'false'
        ? edge.sourceHandle
        : undefined,
    style:
      edge.sourceHandle === 'true'
        ? { stroke: '#34d399' }
        : edge.sourceHandle === 'false'
          ? { stroke: '#fb7185' }
          : undefined,
  };
}

function workflowToFlow(graph: WorkflowGraph): { nodes: Node[]; edges: Edge[] } {
  const normalizedNodes = ensureWorkflowNodeReferenceKeys(graph.nodes, graph.edges);
  return {
    nodes: normalizedNodes.map((n) => ({
      id: n.id,
      type: n.type,
      position: n.position,
      data: n.data ?? {},
    })),
    edges: graph.edges.map(toFlowEdge),
  };
}

function flowToWorkflow(nodes: Node[], edges: Edge[]): WorkflowGraph {
  const normalizedNodes = ensureFlowNodeReferenceKeysForGraph(nodes, edges);
  return {
    schemaVersion: 1,
    nodes: normalizedNodes.map<WorkflowNode>((n) => ({
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
      targetHandle: e.targetHandle ?? null,
    })),
  };
}

function getUpstreamNodes(nodes: Node[], edges: Edge[], selectedNodeId: string | null): Node[] {
  if (!selectedNodeId) {
    return [];
  }

  const nodeById = new Map(nodes.map((node) => [node.id, node]));
  const nodeOrder = new Map(nodes.map((node, index) => [node.id, index]));
  const incomingByTarget = new Map<string, Edge[]>();

  for (const edge of edges) {
    const incoming = incomingByTarget.get(edge.target) ?? [];
    incoming.push(edge);
    incomingByTarget.set(edge.target, incoming);
  }

  const ordered: Node[] = [];
  const visited = new Set<string>();

  const visit = (nodeId: string) => {
    const incoming = [...(incomingByTarget.get(nodeId) ?? [])].sort((a, b) => {
      return (nodeOrder.get(a.source) ?? 0) - (nodeOrder.get(b.source) ?? 0);
    });

    for (const edge of incoming) {
      if (visited.has(edge.source)) {
        continue;
      }
      visited.add(edge.source);
      visit(edge.source);
      const sourceNode = nodeById.get(edge.source);
      if (sourceNode) {
        ordered.push(sourceNode);
      }
    }
  };

  visit(selectedNodeId);
  return ordered;
}

function getDirectParentNodeIds(
  nodes: Node[],
  edges: Edge[],
  selectedNodeId: string | null,
): string[] {
  if (!selectedNodeId) {
    return [];
  }

  const nodeOrder = new Map(nodes.map((node, index) => [node.id, index]));

  return edges
    .filter((edge) => edge.target === selectedNodeId)
    .sort((a, b) => (nodeOrder.get(a.source) ?? 0) - (nodeOrder.get(b.source) ?? 0))
    .map((edge) => edge.source);
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  if (target.isContentEditable) {
    return true;
  }
  return Boolean(target.closest('input, textarea, select, [contenteditable="true"]'));
}

type PendingCreateConnection = {
  source: string;
  sourceHandle: string | null;
  clientX: number;
  clientY: number;
};

const EDGE_DROP_NODE_GROUPS = ['Agents', 'Logic', 'Code', 'Board', 'Integrations'] as const;

const EDGE_DROP_NODE_OPTIONS = NODE_REGISTRY.filter((node) => !node.type.startsWith('trigger.'));

function getEdgeHandleDisplay(handle: string | null | undefined) {
  if (handle === 'true' || handle === 'false') {
    return handle;
  }
  return undefined;
}

function getEdgeStyle(handle: string | null | undefined) {
  if (handle === 'true') {
    return { stroke: '#34d399' };
  }
  if (handle === 'false') {
    return { stroke: '#fb7185' };
  }
  return undefined;
}

function getConnectionDropClientPosition(
  event: MouseEvent | TouchEvent,
  connectionState: FinalConnectionState,
  hostRect: DOMRect | null,
): XYPosition | null {
  if (hostRect && connectionState.pointer) {
    return {
      x: hostRect.left + connectionState.pointer.x,
      y: hostRect.top + connectionState.pointer.y,
    };
  }

  if (event instanceof MouseEvent) {
    return { x: event.clientX, y: event.clientY };
  }

  const touch = event.changedTouches[0] ?? event.touches[0];
  if (!touch) {
    return null;
  }

  return { x: touch.clientX, y: touch.clientY };
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
  const canvasHostRef = useRef<HTMLDivElement | null>(null);
  const sensors = useAgentDndSensors();
  const [draggingNodeType, setDraggingNodeType] = useState<string | null>(null);
  const { setNodeRef: setCanvasDropRef, isOver: isCanvasOver } = useDroppable({
    id: WORKFLOW_CANVAS_DROPPABLE_ID,
  });

  const { data: workflow, isLoading } = useQuery<ProjectWorkflow>({
    queryKey: ['project-workflow', workflowId],
    queryFn: () => projectWorkflowsApi.get(workflowId),
  });

  useEffect(() => {
    const refreshRun = (runId: string) => {
      queryClient.invalidateQueries({ queryKey: ['workflow-runs', workflowId] });
      queryClient.invalidateQueries({ queryKey: ['workflow-run', runId] });
    };

    const unlistenCreated = onWorkflowRunCreated((payload) => {
      if (payload.workflowId !== workflowId) return;
      refreshRun(payload.runId);
    });
    const unlistenUpdated = onWorkflowRunUpdated((payload) => {
      if (payload.workflowId !== workflowId) return;
      refreshRun(payload.runId);
    });
    const unlistenStep = onWorkflowRunStep((payload) => {
      if (payload.workflowId !== workflowId) return;
      refreshRun(payload.runId);
    });

    return () => {
      unlistenCreated.then((fn) => fn()).catch(() => {});
      unlistenUpdated.then((fn) => fn()).catch(() => {});
      unlistenStep.then((fn) => fn()).catch(() => {});
    };
  }, [queryClient, workflowId]);

  const [nodes, setNodes] = useState<Node[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [enabled, setEnabled] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [runDrawerOpen, setRunDrawerOpen] = useState(false);
  const [runDrawerFocusRunId, setRunDrawerFocusRunId] = useState<string | null>(null);
  const [runError, setRunError] = useState<string | null>(null);
  const [pendingCreateConnection, setPendingCreateConnection] = useState<PendingCreateConnection | null>(null);
  const connectStartRef = useRef<Pick<PendingCreateConnection, 'source' | 'sourceHandle'> | null>(null);

  const setCanvasHostRef = useCallback(
    (node: HTMLDivElement | null) => {
      canvasHostRef.current = node;
      setCanvasDropRef(node);
    },
    [setCanvasDropRef],
  );

  const selectedNode = useMemo(
    () => nodes.find((n) => n.id === selectedNodeId) ?? null,
    [nodes, selectedNodeId],
  );
  const upstreamNodes = useMemo(
    () => getUpstreamNodes(nodes, edges, selectedNodeId),
    [edges, nodes, selectedNodeId],
  );
  const directParentNodeIds = useMemo(
    () => getDirectParentNodeIds(nodes, edges, selectedNodeId),
    [edges, nodes, selectedNodeId],
  );
  const nodesWithDownstreamLinks = useMemo(
    () => new Set(edges.map((edge) => edge.source)),
    [edges],
  );
  const selectedNodeHasLinkedOutputs = useMemo(
    () =>
      selectedNode ? nodeHasLinkedOutputs(selectedNode, nodesWithDownstreamLinks) : false,
    [nodesWithDownstreamLinks, selectedNode],
  );

  const markDirty = useCallback(() => setDirty(true), []);

  const deleteEdge = useCallback(
    (edgeId: string) => {
      setEdges((curr) => curr.filter((edge) => edge.id !== edgeId));
      markDirty();
    },
    [markDirty],
  );

  const decorateEdge = useCallback(
    (edge: Edge): Edge => ({
      ...edge,
      type: 'deletable',
      data: {
        ...(edge.data as Record<string, unknown> | undefined),
        onDelete: deleteEdge,
      },
    }),
    [deleteEdge],
  );

  useEffect(() => {
    if (!workflow) return;
    const { nodes: ns, edges: es } = workflowToFlow(workflow.graph);
    setNodes(ns);
    setEdges(es.map(decorateEdge));
    setName(workflow.name);
    setDescription(workflow.description ?? '');
    setEnabled(workflow.enabled);
    setDirty(false);
    setPendingCreateConnection(null);
    connectStartRef.current = null;
  }, [decorateEdge, workflow]);

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
        type: 'deletable',
        source: conn.source!,
        target: conn.target!,
        sourceHandle: handle ?? undefined,
        label: getEdgeHandleDisplay(handle),
        style: getEdgeStyle(handle),
      };
      setEdges((curr) => addEdge(decorateEdge(newEdge), curr));
      markDirty();
    },
    [decorateEdge, markDirty, nodes],
  );

  const onReconnect = useCallback(
    (oldEdge: Edge, newConn: Connection) => {
      setEdges((curr) => reconnectEdge(oldEdge, newConn, curr).map(decorateEdge));
      markDirty();
    },
    [decorateEdge, markDirty],
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

  const createNodeAtClientPoint = useCallback(
    (type: string, clientX: number, clientY: number) => {
      const meta = nodeMeta(type);
      if (!meta) return null;

      const hostRect = canvasHostRef.current?.getBoundingClientRect() ?? null;
      const insideHost = hostRect
        ? clientX >= hostRect.left &&
          clientX <= hostRect.right &&
          clientY >= hostRect.top &&
          clientY <= hostRect.bottom
        : true;
      if (!insideHost) return null;

      const position = screenToFlowPosition({ x: clientX, y: clientY });
      const id = `n_${crypto.randomUUID()}`;
      setNodes((curr) => {
        const newNode: Node = {
          id,
          type,
          position,
          data: {
            ...meta.defaultData,
            referenceKey: generateReferenceKeyForNewNode(type, curr),
          },
        };
        return curr.concat(newNode);
      });
      return { id };
    },
    [screenToFlowPosition],
  );

  const addNodeAtClientPoint = useCallback(
    (type: string, clientX: number, clientY: number) => {
      const created = createNodeAtClientPoint(type, clientX, clientY);
      if (!created) {
        return false;
      }
      markDirty();
      return true;
    },
    [createNodeAtClientPoint, markDirty],
  );

  const handleConnectStart = useCallback(
    (_event: MouseEvent | TouchEvent, params: OnConnectStartParams) => {
      if (!params.nodeId || params.handleType !== 'source') {
        connectStartRef.current = null;
        return;
      }

      connectStartRef.current = {
        source: params.nodeId,
        sourceHandle: params.handleId,
      };
      setPendingCreateConnection(null);
    },
    [],
  );

  const handleConnectEnd = useCallback(
    (event: MouseEvent | TouchEvent, connectionState: FinalConnectionState) => {
      const start = connectStartRef.current;
      connectStartRef.current = null;

      if (!start || connectionState.toNode) {
        return;
      }

      const hostRect = canvasHostRef.current?.getBoundingClientRect() ?? null;
      const pointer = getConnectionDropClientPosition(event, connectionState, hostRect);
      if (!pointer || !hostRect) {
        return;
      }

      const insideHost =
        pointer.x >= hostRect.left &&
        pointer.x <= hostRect.right &&
        pointer.y >= hostRect.top &&
        pointer.y <= hostRect.bottom;
      if (!insideHost) {
        return;
      }

      setPendingCreateConnection({
        source: start.source,
        sourceHandle: start.sourceHandle,
        clientX: pointer.x,
        clientY: pointer.y,
      });
    },
    [],
  );

  const handleCreateConnectedNode = useCallback(
    (type: string) => {
      if (!pendingCreateConnection) {
        return;
      }

      const created = createNodeAtClientPoint(
        type,
        pendingCreateConnection.clientX,
        pendingCreateConnection.clientY,
      );
      if (!created) {
        setPendingCreateConnection(null);
        return;
      }

      const sourceNode = nodes.find((node) => node.id === pendingCreateConnection.source);
      const isLogicIf = sourceNode?.type === 'logic.if';
      const handle =
        pendingCreateConnection.sourceHandle ?? (isLogicIf ? 'true' : null);

      const newEdge: Edge = {
        id: `e_${crypto.randomUUID()}`,
        type: 'deletable',
        source: pendingCreateConnection.source,
        target: created.id,
        sourceHandle: handle ?? undefined,
        label: getEdgeHandleDisplay(handle),
        style: getEdgeStyle(handle),
      };

      setEdges((curr) => addEdge(decorateEdge(newEdge), curr));
      setSelectedNodeId(created.id);
      setPendingCreateConnection(null);
      markDirty();
    },
    [createNodeAtClientPoint, decorateEdge, markDirty, nodes, pendingCreateConnection],
  );

  const closePendingCreateConnection = useCallback(() => {
    connectStartRef.current = null;
    setPendingCreateConnection(null);
  }, []);

  const handleDragStart = useCallback((event: DragStartEvent) => {
    const type = parseWorkflowNodeDraggableId(event.active.id);
    setDraggingNodeType(type);
  }, []);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const type = parseWorkflowNodeDraggableId(event.active.id);
      const overId = event.over?.id ?? null;
      const hostRect = canvasHostRef.current?.getBoundingClientRect() ?? null;
      const activatorEvent = event.activatorEvent;
      const fallbackX = hostRect ? hostRect.left + hostRect.width / 2 : 0;
      const fallbackY = hostRect ? hostRect.top + hostRect.height / 2 : 0;
      const clientX =
        activatorEvent instanceof MouseEvent
          ? activatorEvent.clientX + event.delta.x
          : fallbackX;
      const clientY =
        activatorEvent instanceof MouseEvent
          ? activatorEvent.clientY + event.delta.y
          : fallbackY;
      const endedInsideCanvas = hostRect
        ? clientX >= hostRect.left &&
          clientX <= hostRect.right &&
          clientY >= hostRect.top &&
          clientY <= hostRect.bottom
        : false;
      setDraggingNodeType(null);
      if (!type) return;
      if (overId !== WORKFLOW_CANVAS_DROPPABLE_ID && !endedInsideCanvas) return;

      addNodeAtClientPoint(type, clientX, clientY);
    },
    [addNodeAtClientPoint],
  );

  const handleDragCancel = useCallback(() => {
    setDraggingNodeType(null);
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

  const handleSave = useCallback(() => {
    if (!workflow || !dirty || saveMutation.isPending) {
      return;
    }
    saveMutation.mutate();
  }, [dirty, saveMutation, workflow]);

  const runMutation = useMutation({
    mutationFn: () => workflowRunsApi.start(workflowId, {}),
    onSuccess: (run) => {
      setRunError(null);
      setRunDrawerFocusRunId(run.id);
      setRunDrawerOpen(true);
      queryClient.invalidateQueries({ queryKey: ['workflow-runs', workflowId] });
      queryClient.invalidateQueries({ queryKey: ['workflow-run', run.id] });
    },
    onError: (err) => {
      setRunError(String(err));
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

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const isSaveShortcut =
        (event.metaKey || event.ctrlKey) &&
        event.key.toLowerCase() === 's' &&
        !event.altKey;

      if (isSaveShortcut) {
        event.preventDefault();
        handleSave();
        return;
      }

      if (event.key !== 'Delete' || isEditableTarget(event.target)) {
        return;
      }

      if (!selectedNodeId) {
        return;
      }

      event.preventDefault();
      deleteNode(selectedNodeId);
    }

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [deleteNode, handleSave, selectedNodeId]);

  if (isLoading || !workflow) {
    return (
      <div className="flex items-center justify-center h-full text-muted text-sm">Loading…</div>
    );
  }

  const draggingNodeMeta = draggingNodeType ? nodeMeta(draggingNodeType) : null;

  return (
    <DndContext
      collisionDetection={pointerWithin}
      sensors={sensors}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
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
        <Input
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setDirty(true);
          }}
          className="bg-transparent border-transparent border-b hover:border-edge focus:border-accent text-sm font-semibold rounded-none px-1 py-0.5 min-w-[200px]"
        />
        <Input
          value={description}
          onChange={(e) => {
            setDescription(e.target.value);
            setDirty(true);
          }}
          placeholder="Description (optional)"
          className="bg-transparent border-transparent border-b hover:border-edge focus:border-accent text-xs text-muted rounded-none px-1 py-0.5 flex-1 max-w-[420px]"
        />
        <span className="text-[10px] uppercase tracking-wider text-muted font-mono">
          v{workflow.version}
        </span>
        <div className="flex-1" />
        <Checkbox
          checked={enabled}
          onCheckedChange={(checked) => enabledMutation.mutate(checked === true)}
          label="Enabled"
          labelClassName="text-xs text-muted"
        />
        <button
          onClick={() => {
            setRunDrawerFocusRunId(null);
            setRunDrawerOpen(true);
          }}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-edge hover:bg-edge/30 text-white text-xs font-medium transition-colors"
        >
          <History size={12} />
          History
        </button>
        <button
          onClick={() => runMutation.mutate()}
          disabled={runMutation.isPending || dirty}
          title={
            dirty
              ? 'Save changes before running'
              : 'Run this workflow now'
          }
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-emerald-500/80 hover:bg-emerald-500 text-white text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <Play size={12} />
          {runMutation.isPending ? 'Starting…' : 'Run'}
        </button>
        <button
          onClick={handleSave}
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

      {runError && (
        <div className="px-4 py-2 bg-red-500/10 border-b border-red-500/20 text-xs text-red-300 flex items-center justify-between">
          <span>{runError}</span>
          <button
            onClick={() => setRunError(null)}
            className="text-red-300 hover:text-white"
          >
            dismiss
          </button>
        </div>
      )}

      <div className="flex flex-1 min-h-0 overflow-hidden">
        <NodePalette />
        <div
          ref={setCanvasHostRef}
          className={`flex-1 relative ${isCanvasOver ? 'ring-1 ring-accent bg-accent/5' : ''}`}
        >
          <ReactFlow
            className="workflow-editor-flow"
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            edgeTypes={edgeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onConnectStart={handleConnectStart}
            onConnectEnd={handleConnectEnd}
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
          {pendingCreateConnection && (
            <CreateConnectedNodeMenu
              clientX={pendingCreateConnection.clientX}
              clientY={pendingCreateConnection.clientY}
              host={canvasHostRef.current}
              onClose={closePendingCreateConnection}
              onSelect={handleCreateConnectedNode}
            />
          )}
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
        <div
          className={`shrink-0 overflow-hidden transition-[width] duration-300 ease-out ${
            selectedNode ? 'w-80' : 'w-0'
          }`}
        >
          <NodeInspector
            isOpen={Boolean(selectedNode)}
            node={selectedNode}
            nodeHasLinkedOutputs={selectedNodeHasLinkedOutputs}
            upstreamNodes={upstreamNodes}
            directParentNodeIds={directParentNodeIds}
            projectId={workflow.projectId}
            workflowId={workflowId}
            onChangeData={updateNodeData}
            onDelete={deleteNode}
          />
        </div>
      </div>

      {runDrawerOpen && (
        <RunHistoryDrawer
          workflowId={workflowId}
          focusRunId={runDrawerFocusRunId}
          onClose={() => {
            setRunDrawerOpen(false);
            setRunDrawerFocusRunId(null);
          }}
        />
      )}
      </div>
      <DragOverlay>
        {draggingNodeMeta ? (
          <div className="pointer-events-none flex items-center gap-2 rounded-md border border-accent bg-surface px-3 py-2 text-xs font-medium text-white shadow-xl">
            <draggingNodeMeta.icon size={12} className="text-accent-hover" />
            <span>{draggingNodeMeta.label}</span>
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

function CreateConnectedNodeMenu({
  clientX,
  clientY,
  host,
  onClose,
  onSelect,
}: {
  clientX: number;
  clientY: number;
  host: HTMLDivElement | null;
  onClose: () => void;
  onSelect: (type: string) => void;
}) {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  const hostRect = host?.getBoundingClientRect();
  if (!hostRect) {
    return null;
  }

  const panelWidth = 280;
  const panelHeight = 360;
  const margin = 12;
  const left = Math.min(
    Math.max(clientX - hostRect.left, margin),
    Math.max(margin, hostRect.width - panelWidth - margin),
  );
  const top = Math.min(
    Math.max(clientY - hostRect.top, margin),
    Math.max(margin, hostRect.height - panelHeight - margin),
  );

  return (
    <div
      className="absolute inset-0 z-20"
      onPointerDown={onClose}
    >
      <div
        className="absolute w-[280px] rounded-xl border border-edge bg-surface/95 shadow-2xl backdrop-blur-sm"
        style={{ left, top }}
        onPointerDown={(event) => event.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-3 border-b border-edge px-3 py-2.5">
          <div>
            <p className="text-[10px] uppercase tracking-wider text-muted">Create And Connect</p>
            <p className="text-xs text-white">Choose the next node to add on the canvas.</p>
          </div>
          <button
            type="button"
            aria-label="Close create node menu"
            onClick={onClose}
            className="rounded-md p-1 text-muted transition-colors hover:bg-edge hover:text-white"
          >
            <X size={12} />
          </button>
        </div>

        <div className="max-h-[320px] overflow-y-auto px-2 py-2">
          {EDGE_DROP_NODE_GROUPS.map((group) => {
            const options = EDGE_DROP_NODE_OPTIONS.filter((node) => node.group === group);
            if (options.length === 0) {
              return null;
            }

            return (
              <div key={group} className="mb-2 last:mb-0">
                <div className="px-2 pb-1 text-[10px] uppercase tracking-wider text-muted">
                  {group}
                </div>
                <div className="space-y-1">
                  {options.map((node, index) => {
                    const Icon = node.icon;
                    return (
                      <button
                        key={node.type}
                        type="button"
                        autoFocus={index === 0 && group === EDGE_DROP_NODE_GROUPS[0]}
                        onClick={() => onSelect(node.type)}
                        className="flex w-full items-center gap-2 rounded-lg border border-edge bg-background/60 px-2.5 py-2 text-left transition-colors hover:border-accent hover:bg-accent/5"
                      >
                        <Icon size={12} className="text-muted" />
                        <span className="flex-1 text-xs text-white">{node.label}</span>
                        {node.comingSoon ? (
                          <span className="rounded bg-muted/15 px-1 py-0.5 text-[9px] uppercase tracking-wider text-muted">
                            Soon
                          </span>
                        ) : null}
                      </button>
                    );
                  })}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
