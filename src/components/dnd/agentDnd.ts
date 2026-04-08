import { useMutation, useQueryClient } from '@tanstack/react-query';
import {
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from '@dnd-kit/core';
import { projectsApi } from '../../api/projects';

/**
 * Shared sensor config for agent↔project drag-and-drop.
 *
 * - 8px activation distance avoids accidental drops from click-misfires.
 * - KeyboardSensor gives keyboard-only users an accessible drag path.
 */
export function useAgentDndSensors() {
  return useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor),
  );
}

/**
 * Drag item identifiers used by both the sidebar and the project agents tab.
 * Encoded as strings so they can round-trip through dnd-kit's `id` field.
 */
export function agentDraggableId(agentId: string): string {
  return `agent:${agentId}`;
}

export function projectDroppableId(projectId: string): string {
  return `project:${projectId}`;
}

/**
 * Droppable ID for the "Available" pane in the two-pane Project Agents tab.
 * Dropping an agent here removes it from the project. Kept distinct from
 * `projectDroppableId` so a single DndContext can host both panes without
 * collision.
 */
export function projectAvailableDroppableId(projectId: string): string {
  return `project-available:${projectId}`;
}

export function parseAgentDraggableId(id: string | number | null | undefined): string | null {
  if (typeof id !== 'string' || !id.startsWith('agent:')) return null;
  return id.slice('agent:'.length);
}

export function parseProjectDroppableId(id: string | number | null | undefined): string | null {
  if (typeof id !== 'string' || !id.startsWith('project:')) return null;
  // Must not match `project-available:...`
  if (id.startsWith('project-available:')) return null;
  return id.slice('project:'.length);
}

export function parseProjectAvailableDroppableId(
  id: string | number | null | undefined,
): string | null {
  if (typeof id !== 'string' || !id.startsWith('project-available:')) return null;
  return id.slice('project-available:'.length);
}

/**
 * Mutation hook shared across every DnD surface that assigns an agent to a
 * project. Invalidates the three caches that depend on project membership so
 * the sidebar count badge, the project agents tab, and the agent inspector
 * projects section all refresh without per-call bookkeeping.
 */
export function useAssignAgentToProject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ projectId, agentId }: { projectId: string; agentId: string }) =>
      projectsApi.addAgent(projectId, agentId, false),
    onSuccess: (_data, { projectId, agentId }) => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      queryClient.invalidateQueries({ queryKey: ['project-agents', projectId] });
      queryClient.invalidateQueries({ queryKey: ['agent-projects', agentId] });
    },
  });
}

/**
 * Matching mutation for the "Available ↔ Assigned" two-pane layout in the
 * Project Agents tab, where dragging off the Assigned pane unassigns.
 */
export function useRemoveAgentFromProject() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ projectId, agentId }: { projectId: string; agentId: string }) =>
      projectsApi.removeAgent(projectId, agentId),
    onSuccess: (_data, { projectId, agentId }) => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      queryClient.invalidateQueries({ queryKey: ['project-agents', projectId] });
      queryClient.invalidateQueries({ queryKey: ['agent-projects', agentId] });
    },
  });
}
