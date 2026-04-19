const WORKFLOW_NODE_DRAG_PREFIX = 'workflow-node:';

export const WORKFLOW_CANVAS_DROPPABLE_ID = 'workflow-canvas';

export function workflowNodeDraggableId(type: string): string {
  return `${WORKFLOW_NODE_DRAG_PREFIX}${type}`;
}

export function parseWorkflowNodeDraggableId(
  id: string | number | null | undefined,
): string | null {
  if (typeof id !== 'string' || !id.startsWith(WORKFLOW_NODE_DRAG_PREFIX)) {
    return null;
  }

  return id.slice(WORKFLOW_NODE_DRAG_PREFIX.length);
}
