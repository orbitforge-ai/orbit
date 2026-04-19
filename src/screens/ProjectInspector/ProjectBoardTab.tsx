import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  pointerWithin,
  rectIntersection,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import { KanbanSquare, Plus } from 'lucide-react';
import { workItemsApi } from '../../api/workItems';
import { agentsApi } from '../../api/agents';
import { projectsApi } from '../../api/projects';
import { Agent, ProjectBoardColumn, WorkItem } from '../../types';
import { cn } from '../../lib/cn';
import { ProjectBoardCard } from './ProjectBoardCard';
import { ProjectBoardDetailDrawer } from './ProjectBoardDetailDrawer';

const CARD_DRAG_PREFIX = 'work-item:';
const COLUMN_DROP_PREFIX = 'column:';

function cardDragId(id: string): string {
  return `${CARD_DRAG_PREFIX}${id}`;
}
function parseCardDragId(id: string | number | null | undefined): string | null {
  if (typeof id !== 'string' || !id.startsWith(CARD_DRAG_PREFIX)) return null;
  return id.slice(CARD_DRAG_PREFIX.length);
}
function columnDropId(columnId: string): string {
  return `${COLUMN_DROP_PREFIX}${columnId}`;
}
function parseColumnDropId(
  id: string | number | null | undefined,
): string | null {
  if (typeof id !== 'string' || !id.startsWith(COLUMN_DROP_PREFIX)) return null;
  return id.slice(COLUMN_DROP_PREFIX.length);
}

export function ProjectBoardTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [openItemId, setOpenItemId] = useState<string | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor),
  );

  const { data: items = [], isLoading } = useQuery<WorkItem[]>({
    queryKey: ['work-items', projectId],
    queryFn: () => workItemsApi.list(projectId),
  });

  const { data: agents = [] } = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });
  const { data: columns = [] } = useQuery<ProjectBoardColumn[]>({
    queryKey: ['project-board-columns', projectId],
    queryFn: () => projectsApi.listBoardColumns(projectId),
  });

  const agentById = useMemo(() => new Map(agents.map((a) => [a.id, a])), [agents]);
  const columnById = useMemo(() => new Map(columns.map((column) => [column.id, column])), [columns]);
  const firstColumnId = columns[0]?.id ?? null;
  const backlogColumnId =
    columns.find((column) => column.status === 'backlog')?.id ?? firstColumnId;

  const itemsByColumn = useMemo(() => {
    const groups = new Map<string, WorkItem[]>();
    for (const col of columns) groups.set(col.id, []);
    for (const item of items) {
      const resolvedColumnId =
        item.columnId && columnById.has(item.columnId)
          ? item.columnId
          : columns.find((column) => column.status === item.status)?.id ?? firstColumnId;
      if (!resolvedColumnId) continue;
      const list = groups.get(resolvedColumnId);
      if (list) list.push(item);
    }
    for (const [, list] of groups) list.sort((a, b) => a.position - b.position);
    return groups;
  }, [columnById, columns, firstColumnId, items]);

  const moveMutation = useMutation({
    mutationFn: ({ id, columnId, status, position }: {
      id: string;
      columnId: string;
      status: ProjectBoardColumn['status'];
      position?: number;
    }) => workItemsApi.move(id, status, columnId, position),
    onMutate: async ({ id, columnId, status, position }) => {
      await queryClient.cancelQueries({ queryKey: ['work-items', projectId] });
      const prev = queryClient.getQueryData<WorkItem[]>(['work-items', projectId]);
      queryClient.setQueryData<WorkItem[]>(['work-items', projectId], (old = []) =>
        old.map((it) =>
          it.id === id
            ? { ...it, columnId, status, position: position ?? it.position }
            : it,
        ),
      );
      return { prev };
    },
    onError: (_err, _vars, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(['work-items', projectId], ctx.prev);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    },
  });

  const blockMutation = useMutation({
    mutationFn: ({ id, reason }: { id: string; reason: string }) =>
      workItemsApi.block(id, reason),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    },
  });

  function handleDragStart(event: DragStartEvent) {
    setDraggingId(parseCardDragId(event.active.id));
  }

  function handleDragEnd(event: DragEndEvent) {
    setDraggingId(null);
    const cardId = parseCardDragId(event.active.id);
    if (!cardId || !event.over) return;

    const overId = event.over.id;
    let targetColumnId = parseColumnDropId(overId);
    let targetCardId: string | null = null;
    if (!targetColumnId) {
      const overCardId = parseCardDragId(overId);
      if (overCardId) {
        const overItem = items.find((it) => it.id === overCardId);
        if (overItem) {
          targetColumnId =
            overItem.columnId ??
            columns.find((column) => column.status === overItem.status)?.id ??
            null;
          targetCardId = overCardId;
        }
      }
    }
    if (!targetColumnId) return;

    const card = items.find((it) => it.id === cardId);
    if (!card) return;
    const targetColumn = columnById.get(targetColumnId);
    if (!targetColumn) return;

    if (targetColumn.status === 'blocked') {
      const reason = window.prompt('Why is this card blocked?');
      if (!reason || !reason.trim()) return;
      blockMutation.mutate({ id: cardId, reason: reason.trim() });
      return;
    }

    // Compute target position
    const columnItems = (itemsByColumn.get(targetColumnId) ?? []).filter(
      (it) => it.id !== cardId,
    );
    let position: number | undefined;
    if (targetCardId) {
      const idx = columnItems.findIndex((it) => it.id === targetCardId);
      if (idx >= 0) {
        const before = idx > 0 ? columnItems[idx - 1].position : 0;
        const after = columnItems[idx].position;
        position = (before + after) / 2;
      }
    }
    // Same column same position no-op
    if (
      (card.columnId ?? columns.find((column) => column.status === card.status)?.id) === targetColumnId &&
      (position === undefined || Math.abs(position - card.position) < 0.0001)
    ) {
      return;
    }
    moveMutation.mutate({
      id: cardId,
      columnId: targetColumnId,
      status: targetColumn.status,
      position,
    });
  }

  const draggingItem = draggingId ? items.find((it) => it.id === draggingId) ?? null : null;

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-32 text-muted text-sm">Loading…</div>
    );
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={(args) => {
        const pointerCollisions = pointerWithin(args);
        if (pointerCollisions.length > 0) return pointerCollisions;
        const intersect = rectIntersection(args);
        if (intersect.length > 0) return intersect;
        return closestCenter(args);
      }}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
    >
      <div className="flex flex-col h-full">
        <div className="flex items-center justify-between px-4 py-3 border-b border-edge">
          <h3 className="flex items-center gap-2 text-sm font-semibold text-white">
            <KanbanSquare size={14} className="text-emerald-400" />
            Board
            <span className="text-xs text-muted font-normal">
              ({items.length})
            </span>
          </h3>
        </div>

        <div className="flex-1 min-h-0 overflow-x-auto overflow-y-hidden">
          <div className="flex gap-3 p-3 h-full min-w-max">
            {columns.map((col) => {
              const colItems = itemsByColumn.get(col.id) ?? [];
              return (
                <Column
                  key={col.id}
                  id={col.id}
                  label={col.name}
                  tone={columnTone(col.status)}
                  count={colItems.length}
                >
                  {backlogColumnId === col.id && (
                    <QuickAddRow projectId={projectId} columnId={col.id} />
                  )}
                  {colItems.map((item) => (
                    <DraggableCard key={item.id} id={item.id}>
                      <ProjectBoardCard
                        item={item}
                        assignee={item.assigneeAgentId ? agentById.get(item.assigneeAgentId) ?? null : null}
                        onClick={() => setOpenItemId(item.id)}
                      />
                    </DraggableCard>
                  ))}
                  {colItems.length === 0 && col.id !== 'backlog' && (
                    <div className="rounded-lg border border-dashed border-edge px-3 py-4 text-center text-[11px] text-muted">
                      Drop cards here
                    </div>
                  )}
                </Column>
              );
            })}
          </div>
        </div>
      </div>

      <DragOverlay>
        {draggingItem ? (
          <div className="rotate-1 opacity-90">
            <ProjectBoardCard
              item={draggingItem}
              assignee={draggingItem.assigneeAgentId ? agentById.get(draggingItem.assigneeAgentId) ?? null : null}
              onClick={() => {}}
            />
          </div>
        ) : null}
      </DragOverlay>

      {openItemId && (
        <ProjectBoardDetailDrawer
          projectId={projectId}
          workItemId={openItemId}
          agents={agents}
          onClose={() => setOpenItemId(null)}
        />
      )}
    </DndContext>
  );
}

// ── Column droppable ──────────────────────────────────────────────────────────

function Column({
  id,
  label,
  tone,
  count,
  children,
}: {
  id: string;
  label: string;
  tone: string;
  count: number;
  children: React.ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: columnDropId(id) });
  return (
    <div className="flex flex-col w-72 shrink-0 rounded-xl border border-edge bg-panel">
      <div className="flex items-center justify-between px-3 py-2 border-b border-edge">
        <div className="flex items-center gap-2">
          <span className={cn('text-xs font-semibold uppercase tracking-wide', tone)}>
            {label}
          </span>
          <span className="text-[10px] text-muted tabular-nums">{count}</span>
        </div>
      </div>
      <div
        ref={setNodeRef}
        className={cn(
          'flex-1 min-h-0 overflow-y-auto p-2 space-y-2 transition-colors',
          isOver && 'bg-accent/5',
        )}
      >
        {children}
      </div>
    </div>
  );
}

// ── Draggable card wrapper ────────────────────────────────────────────────────

function DraggableCard({
  id,
  children,
}: {
  id: string;
  children: React.ReactNode;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: cardDragId(id),
  });
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      className={cn('touch-none', isDragging && 'opacity-30')}
    >
      {children}
    </div>
  );
}

// ── Quick-add row at top of Backlog ───────────────────────────────────────────

function QuickAddRow({ projectId, columnId }: { projectId: string; columnId: string }) {
  const queryClient = useQueryClient();
  const [title, setTitle] = useState('');
  const [adding, setAdding] = useState(false);

  const createMutation = useMutation({
    mutationFn: (titleValue: string) =>
      workItemsApi.create({ projectId, title: titleValue, columnId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
      setTitle('');
      setAdding(false);
    },
  });

  if (!adding) {
    return (
      <button
        onClick={() => setAdding(true)}
        className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-xs text-muted hover:text-accent-hover hover:bg-accent/10 transition-colors"
      >
        <Plus size={12} />
        Add card
      </button>
    );
  }

  return (
    <div className="rounded-md border border-accent/40 bg-background px-2 py-1.5">
      <input
        autoFocus
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && title.trim()) {
            createMutation.mutate(title.trim());
          } else if (e.key === 'Escape') {
            setTitle('');
            setAdding(false);
          }
        }}
        onBlur={() => {
          if (!title.trim()) setAdding(false);
        }}
        placeholder="Card title…"
        className="w-full bg-transparent text-xs text-white outline-none placeholder-muted"
      />
    </div>
  );
}

function columnTone(status: ProjectBoardColumn['status']): string {
  switch (status) {
    case 'todo':
      return 'text-secondary';
    case 'in_progress':
      return 'text-blue-300';
    case 'blocked':
      return 'text-red-300';
    case 'review':
      return 'text-amber-300';
    case 'done':
      return 'text-emerald-300';
    case 'cancelled':
      return 'text-muted';
    default:
      return 'text-muted';
  }
}
