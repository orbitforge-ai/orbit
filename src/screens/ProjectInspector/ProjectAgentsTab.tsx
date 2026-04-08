import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  DndContext,
  DragOverlay,
  useDraggable,
  useDroppable,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import { Bot, Plus, X } from 'lucide-react';
import { projectsApi } from '../../api/projects';
import { agentsApi } from '../../api/agents';
import { Agent } from '../../types';
import { useUiStore } from '../../store/uiStore';
import { cn } from '../../lib/cn';
import {
  agentDraggableId,
  parseAgentDraggableId,
  parseProjectAvailableDroppableId,
  parseProjectDroppableId,
  projectAvailableDroppableId,
  projectDroppableId,
  useAgentDndSensors,
  useAssignAgentToProject,
  useRemoveAgentFromProject,
} from '../../components/dnd/agentDnd';

export function ProjectAgentsTab({ projectId }: { projectId: string }) {
  const { selectAgent } = useUiStore();
  const [addingModal, setAddingModal] = useState(false);

  const { data: projectAgents = [] } = useQuery<Agent[]>({
    queryKey: ['project-agents', projectId],
    queryFn: () => projectsApi.listAgents(projectId),
  });

  const { data: allAgents = [] } = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });

  const projectAgentIds = new Set(projectAgents.map((a) => a.id));
  const availableAgents = allAgents.filter((a) => !projectAgentIds.has(a.id));

  // ── DnD wiring ────────────────────────────────────────────────────────────
  const sensors = useAgentDndSensors();
  const assignAgent = useAssignAgentToProject();
  const removeAgent = useRemoveAgentFromProject();
  const [draggingAgentId, setDraggingAgentId] = useState<string | null>(null);

  const handleDragStart = (event: DragStartEvent) => {
    setDraggingAgentId(parseAgentDraggableId(event.active.id));
  };

  const handleDragEnd = (event: DragEndEvent) => {
    setDraggingAgentId(null);
    const agentId = parseAgentDraggableId(event.active.id);
    if (!agentId) return;
    const overId = event.over?.id;
    const toProjectId = parseProjectDroppableId(overId);
    const toAvailableProjectId = parseProjectAvailableDroppableId(overId);
    if (toProjectId === projectId) {
      // Drop onto Assigned pane — add if not already a member.
      if (!projectAgentIds.has(agentId)) {
        assignAgent.mutate({ projectId, agentId });
      }
    } else if (toAvailableProjectId === projectId) {
      // Drop onto Available pane — remove if currently assigned.
      if (projectAgentIds.has(agentId)) {
        removeAgent.mutate({ projectId, agentId });
      }
    }
  };

  const draggingAgent = draggingAgentId
    ? allAgents.find((a) => a.id === draggingAgentId) ?? null
    : null;

  async function handleAddFromModal(agentId: string) {
    await assignAgent.mutateAsync({ projectId, agentId });
    setAddingModal(false);
  }

  return (
    <DndContext sensors={sensors} onDragStart={handleDragStart} onDragEnd={handleDragEnd}>
      <div className="flex flex-col h-full p-4 gap-4">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-white">
            Project Agents
            <span className="ml-2 text-xs text-muted font-normal">
              ({projectAgents.length} assigned)
            </span>
          </h3>
          <button
            onClick={() => setAddingModal(!addingModal)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-edge text-muted hover:text-white hover:border-edge-hover text-xs font-medium transition-colors"
            title="Add via picker (keyboard-friendly)"
          >
            <Plus size={13} />
            Add via picker
          </button>
        </div>

        <p className="text-xs text-muted">
          Drag agents between panes to assign or unassign.
        </p>

        {/* Add agent picker (fallback / a11y) */}
        {addingModal && (
          <div className="rounded-xl border border-edge bg-surface p-3 space-y-2">
            <p className="text-xs text-muted font-medium">Select an agent to add:</p>
            {availableAgents.length === 0 ? (
              <p className="text-xs text-muted italic">All agents are already assigned.</p>
            ) : (
              availableAgents.map((agent) => (
                <button
                  key={agent.id}
                  onClick={() => handleAddFromModal(agent.id)}
                  className="w-full flex items-center gap-3 px-3 py-2.5 rounded-lg border border-edge bg-panel hover:border-accent hover:bg-accent/10 transition-colors text-left"
                >
                  <Bot size={14} className="text-muted shrink-0" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium text-white">{agent.name}</p>
                    {agent.description && (
                      <p className="text-xs text-muted truncate">{agent.description}</p>
                    )}
                  </div>
                </button>
              ))
            )}
            <button
              onClick={() => setAddingModal(false)}
              className="text-xs text-muted hover:text-white transition-colors"
            >
              Cancel
            </button>
          </div>
        )}

        {/* Two-pane layout */}
        <div className="flex-1 grid grid-cols-2 gap-4 min-h-0">
          <AvailablePane
            projectId={projectId}
            agents={availableAgents}
            onSelect={selectAgent}
          />
          <AssignedPane
            projectId={projectId}
            agents={projectAgents}
            onSelect={selectAgent}
            onRemove={(agentId) => removeAgent.mutate({ projectId, agentId })}
          />
        </div>
      </div>

      <DragOverlay>
        {draggingAgent ? (
          <div className="pointer-events-none rounded-lg border border-accent bg-surface px-3 py-2 text-xs font-medium text-white shadow-xl">
            {draggingAgent.name}
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

// ─── Panes ──────────────────────────────────────────────────────────────────

function AvailablePane({
  projectId,
  agents,
  onSelect,
}: {
  projectId: string;
  agents: Agent[];
  onSelect: (id: string) => void;
}) {
  const { setNodeRef, isOver } = useDroppable({
    id: projectAvailableDroppableId(projectId),
  });
  return (
    <div
      ref={setNodeRef}
      className={cn(
        'flex flex-col rounded-xl border border-dashed border-edge bg-background/40 p-3 min-h-0 transition-colors',
        isOver && 'border-accent bg-accent/5',
      )}
    >
      <div className="mb-2 flex items-center justify-between">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted">Available</h4>
        <span className="text-[10px] text-muted tabular-nums">{agents.length}</span>
      </div>
      <div className="flex-1 overflow-y-auto space-y-1.5 min-h-0">
        {agents.length === 0 ? (
          <p className="text-xs italic text-muted px-1 py-4 text-center">
            All agents are assigned.
          </p>
        ) : (
          agents.map((agent) => (
            <DraggableAgentCard key={agent.id} agent={agent} onSelect={onSelect} />
          ))
        )}
      </div>
    </div>
  );
}

function AssignedPane({
  projectId,
  agents,
  onSelect,
  onRemove,
}: {
  projectId: string;
  agents: Agent[];
  onSelect: (id: string) => void;
  onRemove: (agentId: string) => void;
}) {
  const { setNodeRef, isOver } = useDroppable({
    id: projectDroppableId(projectId),
  });
  return (
    <div
      ref={setNodeRef}
      className={cn(
        'flex flex-col rounded-xl border border-edge bg-surface p-3 min-h-0 transition-colors',
        isOver && 'border-accent bg-accent/10 ring-1 ring-accent',
      )}
    >
      <div className="mb-2 flex items-center justify-between">
        <h4 className="text-xs font-semibold uppercase tracking-wide text-muted">Assigned</h4>
        <span className="text-[10px] text-muted tabular-nums">{agents.length}</span>
      </div>
      <div className="flex-1 overflow-y-auto space-y-1.5 min-h-0">
        {agents.length === 0 ? (
          <p className="text-xs italic text-muted px-1 py-4 text-center">
            Drop an agent here to assign it.
          </p>
        ) : (
          agents.map((agent) => (
            <DraggableAgentCard
              key={agent.id}
              agent={agent}
              onSelect={onSelect}
              onRemove={() => onRemove(agent.id)}
            />
          ))
        )}
      </div>
    </div>
  );
}

// ─── Draggable card ─────────────────────────────────────────────────────────

function DraggableAgentCard({
  agent,
  onSelect,
  onRemove,
}: {
  agent: Agent;
  onSelect: (id: string) => void;
  onRemove?: () => void;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: agentDraggableId(agent.id),
  });
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      className={cn(
        'group flex items-center gap-2.5 px-3 py-2 rounded-lg border border-edge bg-panel cursor-grab active:cursor-grabbing touch-none transition-colors hover:border-accent',
        isDragging && 'opacity-40',
      )}
    >
      <div
        className={cn(
          'w-2 h-2 rounded-full shrink-0',
          agent.state === 'idle' ? 'bg-emerald-400' : 'bg-slate-500',
        )}
      />
      <button
        onClick={(e) => {
          e.stopPropagation();
          onSelect(agent.id);
        }}
        className="flex-1 min-w-0 text-left"
      >
        <p className="text-xs font-medium text-white truncate group-hover:text-accent-hover transition-colors">
          {agent.name}
        </p>
        {agent.description && (
          <p className="text-[10px] text-muted truncate">{agent.description}</p>
        )}
      </button>
      {onRemove && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
          className="p-1 rounded text-muted opacity-0 transition-opacity hover:text-red-400 hover:bg-red-400/10 group-hover:opacity-100"
          title="Remove from project"
        >
          <X size={12} />
        </button>
      )}
    </div>
  );
}
