import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
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
import {
  SortableContext,
  arrayMove,
  horizontalListSortingStrategy,
  useSortable,
} from '@dnd-kit/sortable';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import * as Select from '@radix-ui/react-select';
import { confirm } from '@tauri-apps/plugin-dialog';
import {
  ChevronDown,
  GripVertical,
  KanbanSquare,
  MoreHorizontal,
  Pencil,
  Plus,
  Star,
  Trash2,
} from 'lucide-react';
import { agentsApi } from '../../api/agents';
import { projectsApi } from '../../api/projects';
import { workItemsApi } from '../../api/workItems';
import { cn } from '../../lib/cn';
import { toast } from '../../store/toastStore';
import { useUiStore } from '../../store/uiStore';
import {
  Agent,
  ProjectBoard,
  ProjectBoardColumn,
  WorkItem,
  WorkItemStatus,
} from '../../types';
import { ProjectBoardCard } from './ProjectBoardCard';
import { ProjectBoardDetailDrawer } from './ProjectBoardDetailDrawer';
import { Input, SimpleSelect } from '../../components/ui';

const CARD_DRAG_PREFIX = 'work-item:';
const COLUMN_DROP_PREFIX = 'column-drop:';
const COLUMN_DRAG_PREFIX = 'board-column:';
const ROLE_OPTIONS: Array<{ value: WorkItemStatus | null; label: string }> = [
  { value: null, label: 'No role' },
  { value: 'backlog', label: 'Backlog' },
  { value: 'todo', label: 'Todo' },
  { value: 'in_progress', label: 'In Progress' },
  { value: 'blocked', label: 'Blocked' },
  { value: 'review', label: 'Review' },
  { value: 'done', label: 'Done' },
  { value: 'cancelled', label: 'Cancelled' },
];

function cardDragId(id: string): string {
  return `${CARD_DRAG_PREFIX}${id}`;
}

function parseCardDragId(id: string | number | null | undefined): string | null {
  if (typeof id !== 'string' || !id.startsWith(CARD_DRAG_PREFIX)) return null;
  return id.slice(CARD_DRAG_PREFIX.length);
}

function columnDropId(id: string): string {
  return `${COLUMN_DROP_PREFIX}${id}`;
}

function parseColumnDropId(id: string | number | null | undefined): string | null {
  if (typeof id !== 'string' || !id.startsWith(COLUMN_DROP_PREFIX)) return null;
  return id.slice(COLUMN_DROP_PREFIX.length);
}

function columnDragId(id: string): string {
  return `${COLUMN_DRAG_PREFIX}${id}`;
}

function parseColumnDragId(id: string | number | null | undefined): string | null {
  if (typeof id !== 'string' || !id.startsWith(COLUMN_DRAG_PREFIX)) return null;
  return id.slice(COLUMN_DRAG_PREFIX.length);
}

function currentBoardRevision(columns: ProjectBoardColumn[]): string | undefined {
  const revisions = columns.map((column) => column.updatedAt).sort();
  return revisions[revisions.length - 1];
}

function resolveColumnIdForItem(
  item: WorkItem,
  columns: ProjectBoardColumn[],
  columnById: Map<string, ProjectBoardColumn>,
): string | null {
  if (item.columnId && columnById.has(item.columnId)) {
    return item.columnId;
  }
  return columns.find((column) => column.role === item.status)?.id ?? columns[0]?.id ?? null;
}

export function ProjectBoardTab({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const boardScrollRef = useRef<HTMLDivElement | null>(null);
  const panStateRef = useRef<{
    pointerId: number;
    startX: number;
    scrollLeft: number;
  } | null>(null);
  const [draggingCardId, setDraggingCardId] = useState<string | null>(null);
  const [draggingColumnId, setDraggingColumnId] = useState<string | null>(null);
  const [openItemId, setOpenItemId] = useState<string | null>(null);
  const [editingColumnId, setEditingColumnId] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState('');
  const [isPanning, setIsPanning] = useState(false);
  const [deleteState, setDeleteState] = useState<{
    columnId: string;
    force?: boolean;
  } | null>(null);
  const [boardFormOpen, setBoardFormOpen] = useState(false);
  const [editingBoard, setEditingBoard] = useState<ProjectBoard | null>(null);
  const [deleteBoardState, setDeleteBoardState] = useState<ProjectBoard | null>(null);

  const selectedBoardIdByProject = useUiStore((state) => state.selectedBoardIdByProject);
  const setSelectedBoard = useUiStore((state) => state.setSelectedBoard);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor),
  );

  const { data: boards = [] } = useQuery<ProjectBoard[]>({
    queryKey: ['project-boards', projectId],
    queryFn: () => projectsApi.listBoards(projectId),
  });

  const selectedBoardId = useMemo(() => {
    const stored = selectedBoardIdByProject[projectId];
    if (stored && boards.some((b) => b.id === stored)) return stored;
    return boards.find((b) => b.isDefault)?.id ?? boards[0]?.id ?? null;
  }, [boards, projectId, selectedBoardIdByProject]);

  const activeBoard = useMemo(
    () => boards.find((b) => b.id === selectedBoardId) ?? null,
    [boards, selectedBoardId],
  );

  const { data: items = [], isLoading } = useQuery<WorkItem[]>({
    queryKey: ['work-items', projectId, selectedBoardId],
    queryFn: () => workItemsApi.list(projectId, selectedBoardId ?? undefined),
    enabled: !!selectedBoardId,
  });
  const { data: agents = [] } = useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: agentsApi.list,
  });
  const { data: columns = [] } = useQuery<ProjectBoardColumn[]>({
    queryKey: ['project-board-columns', projectId, selectedBoardId],
    queryFn: () => projectsApi.listBoardColumns(projectId, selectedBoardId ?? undefined),
    enabled: !!selectedBoardId,
  });

  const agentById = useMemo(() => new Map(agents.map((agent) => [agent.id, agent])), [agents]);
  const columnById = useMemo(() => new Map(columns.map((column) => [column.id, column])), [columns]);
  const boardRevision = currentBoardRevision(columns);
  const defaultColumnId =
    columns.find((column) => column.isDefault)?.id ?? columns[0]?.id ?? null;

  const itemsByColumn = useMemo(() => {
    const groups = new Map<string, WorkItem[]>();
    for (const column of columns) groups.set(column.id, []);
    for (const item of items) {
      const resolvedColumnId = resolveColumnIdForItem(item, columns, columnById);
      if (!resolvedColumnId) continue;
      groups.get(resolvedColumnId)?.push(item);
    }
    for (const list of groups.values()) {
      list.sort((a, b) => a.position - b.position);
    }
    return groups;
  }, [columnById, columns, items]);

  function invalidateBoard() {
    queryClient.invalidateQueries({ queryKey: ['project-board-columns', projectId] });
    queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    queryClient.invalidateQueries({ queryKey: ['project-boards', projectId] });
  }

  const workItemsKey = ['work-items', projectId, selectedBoardId] as const;
  const moveMutation = useMutation({
    mutationFn: ({
      id,
      columnId,
      position,
    }: {
      id: string;
      columnId: string;
      role: ProjectBoardColumn['role'];
      position?: number;
    }) => workItemsApi.move(id, columnId, position),
    onMutate: async ({ id, columnId, role, position }) => {
      await queryClient.cancelQueries({ queryKey: workItemsKey });
      const previous = queryClient.getQueryData<WorkItem[]>([...workItemsKey]);
      queryClient.setQueryData<WorkItem[]>([...workItemsKey], (old = []) =>
        old.map((item) =>
          item.id === id
            ? {
                ...item,
                columnId,
                status: role ?? item.status,
                position: position ?? item.position,
              }
            : item,
        ),
      );
      return { previous };
    },
    onError: (error, _vars, ctx) => {
      if (ctx?.previous) {
        queryClient.setQueryData([...workItemsKey], ctx.previous);
      }
      toast.error('Failed to move card', error);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    },
  });

  const blockMutation = useMutation({
    mutationFn: ({ id, reason }: { id: string; reason: string }) => workItemsApi.block(id, reason),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    },
    onError: (error) => toast.error('Failed to block card', error),
  });

  const createColumnMutation = useMutation({
    mutationFn: (payload: {
      name: string;
      role?: ProjectBoardColumn['role'];
      isDefault?: boolean;
      position?: number;
    }) =>
      projectsApi.createBoardColumn({
        projectId,
        boardId: selectedBoardId ?? undefined,
        ...payload,
      }),
    onSuccess: (column) => {
      queryClient.invalidateQueries({ queryKey: ['project-board-columns', projectId] });
      setEditingColumnId(column.id);
      setEditingTitle(column.name);
      toast.success(`Created ${column.name}`);
    },
    onError: (error) => toast.error('Failed to create column', error),
  });

  const updateColumnMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: {
      id: string;
      payload: Parameters<typeof projectsApi.updateBoardColumn>[1];
    }) => projectsApi.updateBoardColumn(id, payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-board-columns', projectId] });
    },
    onError: (error) => {
      toast.error('Failed to update column', error);
      invalidateBoard();
    },
  });

  const reorderColumnsMutation = useMutation({
    mutationFn: (orderedIds: string[]) =>
      projectsApi.reorderBoardColumns(
        projectId,
        orderedIds,
        selectedBoardId ?? undefined,
        boardRevision,
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-board-columns', projectId] });
    },
    onError: (error) => {
      toast.error('Failed to reorder columns', error);
      invalidateBoard();
    },
  });

  const deleteColumnMutation = useMutation({
    mutationFn: ({
      id,
      destinationColumnId,
      force,
    }: {
      id: string;
      destinationColumnId?: string;
      force?: boolean;
    }) =>
      projectsApi.deleteBoardColumn(id, {
        destinationColumnId,
        force,
        expectedRevision: boardRevision,
      }),
    onSuccess: () => {
      setDeleteState(null);
      invalidateBoard();
    },
    onError: async (error, vars) => {
      const message = String(error);
      if (message.includes('Retry with force')) {
        const approved = await confirm(`${message}\n\nDelete anyway?`);
        if (approved) {
          deleteColumnMutation.mutate({ ...vars, force: true });
        }
        return;
      }
      toast.error('Failed to delete column', error);
    },
  });

  const createBoardMutation = useMutation({
    mutationFn: (payload: { name: string; prefix: string }) =>
      projectsApi.createBoard({ projectId, ...payload }),
    onSuccess: (board) => {
      queryClient.invalidateQueries({ queryKey: ['project-boards', projectId] });
      setSelectedBoard(projectId, board.id);
      setBoardFormOpen(false);
      toast.success(`Created ${board.name}`);
    },
    onError: (error) => toast.error('Failed to create board', error),
  });

  const updateBoardMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: {
      id: string;
      payload: { name?: string; prefix?: string };
    }) => projectsApi.updateBoard(id, payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-boards', projectId] });
      setEditingBoard(null);
    },
    onError: (error) => toast.error('Failed to update board', error),
  });

  const deleteBoardMutation = useMutation({
    mutationFn: ({
      id,
      destinationBoardId,
      force,
    }: {
      id: string;
      destinationBoardId?: string;
      force?: boolean;
    }) => projectsApi.deleteBoard(id, { destinationBoardId, force }),
    onSuccess: (_result, vars) => {
      setDeleteBoardState(null);
      if (selectedBoardId === vars.id) {
        const fallback = boards.find((b) => b.id !== vars.id);
        if (fallback) setSelectedBoard(projectId, fallback.id);
      }
      invalidateBoard();
    },
    onError: (error) => toast.error('Failed to delete board', error),
  });

  function beginEditing(column: ProjectBoardColumn) {
    setEditingColumnId(column.id);
    setEditingTitle(column.name);
  }

  function finishEditing(column: ProjectBoardColumn) {
    const nextName = editingTitle.trim();
    setEditingColumnId(null);
    if (!nextName || nextName === column.name) {
      setEditingTitle('');
      return;
    }
    updateColumnMutation.mutate({
      id: column.id,
      payload: { name: nextName, expectedRevision: boardRevision },
    });
    setEditingTitle('');
  }

  function handleDragStart(event: DragStartEvent) {
    setDraggingCardId(parseCardDragId(event.active.id));
    setDraggingColumnId(parseColumnDragId(event.active.id));
  }

  function handleDragEnd(event: DragEndEvent) {
    const activeCardId = parseCardDragId(event.active.id);
    const activeColumnId = parseColumnDragId(event.active.id);
    setDraggingCardId(null);
    setDraggingColumnId(null);

    if (activeColumnId) {
      const overColumnId =
        parseColumnDragId(event.over?.id) ?? parseColumnDropId(event.over?.id) ?? null;
      if (!overColumnId || overColumnId === activeColumnId) return;
      const oldIndex = columns.findIndex((column) => column.id === activeColumnId);
      const newIndex = columns.findIndex((column) => column.id === overColumnId);
      if (oldIndex < 0 || newIndex < 0 || oldIndex === newIndex) return;
      const next = arrayMove(columns, oldIndex, newIndex);
      reorderColumnsMutation.mutate(next.map((column) => column.id));
      return;
    }

    if (!activeCardId || !event.over) return;
    let targetColumnId =
      parseColumnDropId(event.over.id) ?? parseColumnDragId(event.over.id) ?? null;
    let targetCardId: string | null = null;
    if (!targetColumnId) {
      const overCardId = parseCardDragId(event.over.id);
      if (overCardId) {
        const overItem = items.find((item) => item.id === overCardId);
        if (overItem) {
          targetColumnId = resolveColumnIdForItem(overItem, columns, columnById);
          targetCardId = overCardId;
        }
      }
    }
    if (!targetColumnId) return;

    const card = items.find((item) => item.id === activeCardId);
    const targetColumn = columnById.get(targetColumnId);
    if (!card || !targetColumn) return;

    if (targetColumn.role === 'blocked') {
      const reason = window.prompt('Why is this card blocked?');
      if (!reason?.trim()) return;
      blockMutation.mutate({ id: activeCardId, reason: reason.trim() });
      return;
    }

    const columnItems = (itemsByColumn.get(targetColumnId) ?? []).filter((item) => item.id !== activeCardId);
    let position: number | undefined;
    if (targetCardId) {
      const idx = columnItems.findIndex((item) => item.id === targetCardId);
      if (idx >= 0) {
        const before = idx > 0 ? columnItems[idx - 1].position : 0;
        const after = columnItems[idx].position;
        position = (before + after) / 2;
      }
    }

    const currentColumnId = resolveColumnIdForItem(card, columns, columnById);
    const currentRole = columnById.get(currentColumnId ?? '')?.role ?? null;
    if (
      currentColumnId === targetColumnId &&
      currentRole === targetColumn.role &&
      (position === undefined || Math.abs(position - card.position) < 0.0001)
    ) {
      return;
    }

    moveMutation.mutate({
      id: activeCardId,
      columnId: targetColumnId,
      role: targetColumn.role,
      position,
    });
  }

  function handlePointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.pointerType !== 'mouse') return;
    const target = event.target as HTMLElement;
    if (target.closest('[data-pan-disabled="true"]')) return;
    const scroller = boardScrollRef.current;
    if (!scroller) return;
    panStateRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      scrollLeft: scroller.scrollLeft,
    };
    setIsPanning(true);
    scroller.setPointerCapture?.(event.pointerId);
  }

  function handlePointerMove(event: ReactPointerEvent<HTMLDivElement>) {
    const pan = panStateRef.current;
    const scroller = boardScrollRef.current;
    if (!pan || !scroller || pan.pointerId !== event.pointerId) return;
    scroller.scrollLeft = pan.scrollLeft - (event.clientX - pan.startX);
  }

  function endPan(pointerId?: number) {
    if (!panStateRef.current) return;
    if (pointerId != null && panStateRef.current.pointerId !== pointerId) return;
    panStateRef.current = null;
    setIsPanning(false);
  }

  const draggingItem = draggingCardId
    ? items.find((item) => item.id === draggingCardId) ?? null
    : null;

  if (isLoading) {
    return <div className="flex h-32 items-center justify-center text-sm text-muted">Loading…</div>;
  }

  return (
    <>
      <DndContext
        sensors={sensors}
        collisionDetection={(args) => {
          const pointerCollisions = pointerWithin(args);
          if (pointerCollisions.length > 0) return pointerCollisions;
          const intersections = rectIntersection(args);
          if (intersections.length > 0) return intersections;
          return closestCenter(args);
        }}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
      >
        <div className="flex h-full flex-col">
          <div className="flex items-center justify-between gap-3 border-b border-edge px-4 py-3">
            <BoardPicker
              boards={boards}
              selectedBoardId={selectedBoardId}
              itemCount={items.length}
              onSelect={(boardId) => setSelectedBoard(projectId, boardId)}
              onCreate={() => setBoardFormOpen(true)}
              onRename={(board) => setEditingBoard(board)}
              onDelete={(board) => setDeleteBoardState(board)}
            />
            <button
              data-pan-disabled="true"
              disabled={!selectedBoardId}
              onClick={() =>
                createColumnMutation.mutate({
                  name: `Column ${columns.length + 1}`,
                })
              }
              className="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-50"
            >
              <Plus size={12} />
              Add Column
            </button>
          </div>

          <div
            ref={boardScrollRef}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={(event) => endPan(event.pointerId)}
            onPointerCancel={(event) => endPan(event.pointerId)}
            onPointerLeave={(event) => endPan(event.pointerId)}
            className={cn(
              'flex-1 overflow-x-auto overflow-y-hidden',
              isPanning ? 'cursor-grabbing' : 'cursor-grab',
            )}
          >
            {columns.length === 0 ? (
              <div className="flex h-full items-center justify-center p-6">
                <button
                  data-pan-disabled="true"
                  onClick={() => createColumnMutation.mutate({ name: 'Backlog', role: 'backlog', isDefault: true })}
                  className="rounded-xl border border-dashed border-edge px-5 py-6 text-sm text-muted transition-colors hover:border-accent hover:text-white"
                >
                  Create your first board column
                </button>
              </div>
            ) : (
              <SortableContext
                items={columns.map((column) => columnDragId(column.id))}
                strategy={horizontalListSortingStrategy}
              >
                <div className="flex h-full min-w-max gap-3 p-3">
                  {columns.map((column) => {
                    const columnItems = itemsByColumn.get(column.id) ?? [];
                    return (
                      <BoardColumn
                        key={column.id}
                        column={column}
                        count={columnItems.length}
                        editing={editingColumnId === column.id}
                        editingTitle={editingTitle}
                        onEditingTitleChange={setEditingTitle}
                        onBeginEditing={() => beginEditing(column)}
                        onFinishEditing={() => finishEditing(column)}
                        onCancelEditing={() => {
                          setEditingColumnId(null);
                          setEditingTitle('');
                        }}
                        onSetRole={(role) =>
                          updateColumnMutation.mutate({
                            id: column.id,
                            payload: { role, expectedRevision: boardRevision },
                          })
                        }
                        onSetDefault={() =>
                          updateColumnMutation.mutate({
                            id: column.id,
                            payload: { isDefault: true, expectedRevision: boardRevision },
                          })
                        }
                        onDelete={() => setDeleteState({ columnId: column.id })}
                      >
                        {defaultColumnId === column.id && (
                          <QuickAddRow projectId={projectId} columnId={column.id} />
                        )}
                        {columnItems.map((item) => (
                          <DraggableCard key={item.id} id={item.id}>
                            <ProjectBoardCard
                              item={item}
                              boardPrefix={activeBoard?.prefix ?? null}
                              assignee={
                                item.assigneeAgentId
                                  ? agentById.get(item.assigneeAgentId) ?? null
                                  : null
                              }
                              onClick={() => setOpenItemId(item.id)}
                            />
                          </DraggableCard>
                        ))}
                        {columnItems.length === 0 && (
                          <div className="rounded-lg border border-dashed border-edge px-3 py-4 text-center text-[11px] text-muted">
                            {defaultColumnId === column.id ? 'Add a card or drop one here' : 'Drop cards here'}
                          </div>
                        )}
                      </BoardColumn>
                    );
                  })}
                </div>
              </SortableContext>
            )}
          </div>
        </div>

        <DragOverlay>
          {draggingItem ? (
            <div className="rotate-1 opacity-90">
              <ProjectBoardCard
                item={draggingItem}
                boardPrefix={activeBoard?.prefix ?? null}
                assignee={
                  draggingItem.assigneeAgentId
                    ? agentById.get(draggingItem.assigneeAgentId) ?? null
                    : null
                }
                onClick={() => {}}
              />
            </div>
          ) : draggingColumnId ? (
            <div className="w-72 rounded-xl border border-edge bg-panel px-3 py-2 text-xs text-white shadow-xl">
              {columnById.get(draggingColumnId)?.name ?? 'Column'}
            </div>
          ) : null}
        </DragOverlay>
      </DndContext>

      {openItemId && (
        <ProjectBoardDetailDrawer
          projectId={projectId}
          workItemId={openItemId}
          boardPrefix={activeBoard?.prefix ?? null}
          agents={agents}
          onClose={() => setOpenItemId(null)}
        />
      )}

      {(boardFormOpen || editingBoard) && (
        <BoardFormDialog
          mode={editingBoard ? 'edit' : 'create'}
          board={editingBoard}
          isPending={createBoardMutation.isPending || updateBoardMutation.isPending}
          onCancel={() => {
            setBoardFormOpen(false);
            setEditingBoard(null);
          }}
          onSubmit={({ name, prefix }) => {
            if (editingBoard) {
              updateBoardMutation.mutate({
                id: editingBoard.id,
                payload: {
                  name: name !== editingBoard.name ? name : undefined,
                  prefix: prefix !== editingBoard.prefix ? prefix : undefined,
                },
              });
            } else {
              createBoardMutation.mutate({ name, prefix });
            }
          }}
        />
      )}

      {deleteBoardState && (
        <DeleteBoardDialog
          board={deleteBoardState}
          boards={boards}
          onCancel={() => setDeleteBoardState(null)}
          onConfirm={(destinationBoardId) =>
            deleteBoardMutation.mutate({ id: deleteBoardState.id, destinationBoardId })
          }
        />
      )}

      {deleteState && (
        <DeleteColumnDialog
          column={columnById.get(deleteState.columnId) ?? null}
          columns={columns}
          itemCount={itemsByColumn.get(deleteState.columnId)?.length ?? 0}
          onCancel={() => setDeleteState(null)}
          onConfirm={(destinationColumnId) =>
            deleteColumnMutation.mutate({
              id: deleteState.columnId,
              destinationColumnId,
              force: deleteState.force,
            })
          }
        />
      )}
    </>
  );
}

function BoardColumn({
  column,
  count,
  editing,
  editingTitle,
  onEditingTitleChange,
  onBeginEditing,
  onFinishEditing,
  onCancelEditing,
  onSetRole,
  onSetDefault,
  onDelete,
  children,
}: {
  column: ProjectBoardColumn;
  count: number;
  editing: boolean;
  editingTitle: string;
  onEditingTitleChange: (value: string) => void;
  onBeginEditing: () => void;
  onFinishEditing: () => void;
  onCancelEditing: () => void;
  onSetRole: (role: ProjectBoardColumn['role']) => void;
  onSetDefault: () => void;
  onDelete: () => void;
  children: React.ReactNode;
}) {
  const { setNodeRef: setDropRef, isOver } = useDroppable({ id: columnDropId(column.id) });
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: columnDragId(column.id) });
  const style = transform
    ? {
        transform: `translate3d(${transform.x}px, ${transform.y}px, 0)`,
        transition,
      }
    : { transition };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(
        'flex w-72 shrink-0 flex-col rounded-xl border border-edge bg-panel',
        isDragging && 'opacity-60',
      )}
    >
      <div className="flex items-center justify-between gap-2 border-b border-edge px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <button
            type="button"
            data-pan-disabled="true"
            className="rounded p-1 text-muted transition-colors hover:bg-edge hover:text-white"
            aria-label={`Drag ${column.name}`}
            {...attributes}
            {...listeners}
          >
            <GripVertical size={13} />
          </button>
          {editing ? (
            <Input
              data-pan-disabled="true"
              autoFocus
              value={editingTitle}
              onChange={(event) => onEditingTitleChange(event.target.value)}
              onBlur={onFinishEditing}
              onKeyDown={(event) => {
                if (event.key === 'Enter') onFinishEditing();
                if (event.key === 'Escape') onCancelEditing();
              }}
              className="min-w-0 flex-1 bg-background rounded px-2 py-1 text-xs font-semibold"
            />
          ) : (
            <button
              type="button"
              data-pan-disabled="true"
              onDoubleClick={onBeginEditing}
              onClick={onBeginEditing}
              className="min-w-0 truncate text-left text-xs font-semibold uppercase tracking-wide text-white"
            >
              {column.name}
            </button>
          )}
          {column.isDefault && (
            <span
              data-pan-disabled="true"
              className="rounded-full bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-300"
            >
              <Star size={10} className="inline" />
            </span>
          )}
          {column.role && (
            <span
              data-pan-disabled="true"
              className={cn(
                'rounded-full px-1.5 py-0.5 text-[10px]',
                roleTone(column.role),
              )}
            >
              {roleLabel(column.role)}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">
          <span className="text-[10px] text-muted tabular-nums">{count}</span>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger asChild>
              <button
                type="button"
                data-pan-disabled="true"
                className="rounded p-1 text-muted transition-colors hover:bg-edge hover:text-white"
                aria-label={`Open ${column.name} menu`}
              >
                <MoreHorizontal size={13} />
              </button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content
                sideOffset={6}
                className="z-50 min-w-44 rounded-lg border border-edge bg-panel p-1 shadow-xl"
              >
                <DropdownMenu.Item
                  className="cursor-pointer rounded px-2 py-1.5 text-xs text-white outline-none hover:bg-edge"
                  onSelect={onBeginEditing}
                >
                  Rename
                </DropdownMenu.Item>
                <DropdownMenu.Item
                  className="cursor-pointer rounded px-2 py-1.5 text-xs text-white outline-none hover:bg-edge"
                  onSelect={onSetDefault}
                  disabled={column.isDefault}
                >
                  Set as default
                </DropdownMenu.Item>
                <DropdownMenu.Separator className="my-1 h-px bg-edge" />
                {ROLE_OPTIONS.map((option) => (
                  <DropdownMenu.Item
                    key={option.label}
                    className="cursor-pointer rounded px-2 py-1.5 text-xs text-white outline-none hover:bg-edge"
                    onSelect={() => onSetRole(option.value)}
                  >
                    Role: {option.label}
                  </DropdownMenu.Item>
                ))}
                <DropdownMenu.Separator className="my-1 h-px bg-edge" />
                <DropdownMenu.Item
                  className="cursor-pointer rounded px-2 py-1.5 text-xs text-red-300 outline-none hover:bg-red-500/10"
                  onSelect={onDelete}
                >
                  <span className="inline-flex items-center gap-2">
                    <Trash2 size={12} />
                    Delete
                  </span>
                </DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </div>
      </div>
      <div
        ref={setDropRef}
        className={cn('flex-1 space-y-2 overflow-y-auto p-2 transition-colors', isOver && 'bg-accent/5')}
      >
        {children}
      </div>
    </div>
  );
}

function DraggableCard({ id, children }: { id: string; children: React.ReactNode }) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: cardDragId(id),
  });
  return (
    <div
      ref={setNodeRef}
      data-pan-disabled="true"
      {...attributes}
      {...listeners}
      className={cn('touch-none', isDragging && 'opacity-30')}
    >
      {children}
    </div>
  );
}

function QuickAddRow({ projectId, columnId }: { projectId: string; columnId: string }) {
  const queryClient = useQueryClient();
  const [title, setTitle] = useState('');
  const [adding, setAdding] = useState(false);

  const createMutation = useMutation({
    mutationFn: (value: string) => workItemsApi.create({ projectId, title: value, columnId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
      setTitle('');
      setAdding(false);
    },
    onError: (error) => toast.error('Failed to create card', error),
  });

  if (!adding) {
    return (
      <button
        data-pan-disabled="true"
        onClick={() => setAdding(true)}
        className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-xs text-muted transition-colors hover:bg-accent/10 hover:text-accent-hover"
      >
        <Plus size={12} />
        Add card
      </button>
    );
  }

  return (
    <div className="rounded-md border border-accent/40 bg-background px-2 py-1.5">
      <Input
        data-pan-disabled="true"
        autoFocus
        value={title}
        onChange={(event) => setTitle(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && title.trim()) {
            createMutation.mutate(title.trim());
          } else if (event.key === 'Escape') {
            setTitle('');
            setAdding(false);
          }
        }}
        onBlur={() => {
          if (!title.trim()) setAdding(false);
        }}
        placeholder="Card title…"
        className="bg-transparent border-transparent rounded-none px-0 py-0 text-xs placeholder-muted"
      />
    </div>
  );
}

function DeleteColumnDialog({
  column,
  columns,
  itemCount,
  onCancel,
  onConfirm,
}: {
  column: ProjectBoardColumn | null;
  columns: ProjectBoardColumn[];
  itemCount: number;
  onCancel: () => void;
  onConfirm: (destinationColumnId?: string) => void;
}) {
  const [destinationColumnId, setDestinationColumnId] = useState('');
  if (!column) return null;
  const destinationOptions = columns.filter((candidate) => candidate.id !== column.id);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div className="w-full max-w-md rounded-2xl border border-edge bg-panel p-4 shadow-2xl">
        <h4 className="text-sm font-semibold text-white">Delete column</h4>
        <p className="mt-2 text-xs text-muted">
          {itemCount > 0
            ? `Move ${itemCount} card${itemCount === 1 ? '' : 's'} out of ${column.name} before deleting it.`
            : `Delete ${column.name}?`}
        </p>
        {itemCount > 0 && (
          <div className="mt-3">
            <SimpleSelect
              value={destinationColumnId}
              onValueChange={setDestinationColumnId}
              placeholder="Choose destination column…"
              className="bg-background px-3 py-2"
              options={destinationOptions.map((option) => ({ value: option.id, label: option.name }))}
            />
          </div>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-lg border border-edge px-3 py-1.5 text-xs text-muted transition-colors hover:text-white"
          >
            Cancel
          </button>
          <button
            onClick={() => onConfirm(destinationColumnId || undefined)}
            disabled={itemCount > 0 && !destinationColumnId}
            className="rounded-lg bg-red-500 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-red-400 disabled:opacity-50"
          >
            Delete column
          </button>
        </div>
      </div>
    </div>
  );
}

function BoardPicker({
  boards,
  selectedBoardId,
  itemCount,
  onSelect,
  onCreate,
  onRename,
  onDelete,
}: {
  boards: ProjectBoard[];
  selectedBoardId: string | null;
  itemCount: number;
  onSelect: (boardId: string) => void;
  onCreate: () => void;
  onRename: (board: ProjectBoard) => void;
  onDelete: (board: ProjectBoard) => void;
}) {
  const active = boards.find((b) => b.id === selectedBoardId) ?? null;

  return (
    <Select.Root
      value={selectedBoardId ?? ''}
      onValueChange={(value) => {
        if (value === '__create__') {
          onCreate();
          return;
        }
        if (value) onSelect(value);
      }}
    >
      <Select.Trigger
        data-pan-disabled="true"
        className="flex items-center gap-2 rounded-lg border border-edge bg-background px-3 py-1.5 text-sm text-white focus:outline-none focus:border-accent"
      >
        <KanbanSquare size={14} className="text-emerald-400" />
        <Select.Value placeholder="Select board…">
          <span className="font-medium">{active?.name ?? 'Select board…'}</span>
        </Select.Value>
        {active && (
          <span className="rounded bg-edge px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted">
            {active.prefix}
          </span>
        )}
        <span className="text-xs font-normal text-muted">({itemCount})</span>
        <Select.Icon>
          <ChevronDown size={14} className="text-muted" />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Content
          position="popper"
          sideOffset={6}
          className="z-50 overflow-hidden rounded-lg border border-edge bg-panel shadow-xl"
        >
          <Select.Viewport className="min-w-56 p-1">
            {boards.map((board) => (
              <div
                key={board.id}
                className="group flex items-center rounded-md data-[highlighted]:bg-accent/20"
              >
                <Select.Item
                  value={board.id}
                  className="flex flex-1 items-center gap-2 px-2 py-1.5 text-sm text-white outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                >
                  <KanbanSquare size={12} className="text-emerald-400" />
                  <Select.ItemText>{board.name}</Select.ItemText>
                  <span className="ml-auto rounded bg-edge px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted">
                    {board.prefix}
                  </span>
                </Select.Item>
                <DropdownMenu.Root>
                  <DropdownMenu.Trigger asChild>
                    <button
                      type="button"
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={(e) => e.stopPropagation()}
                      className="mr-1 rounded p-1 text-muted opacity-0 transition-colors hover:bg-edge hover:text-white group-hover:opacity-100"
                      aria-label={`Manage ${board.name}`}
                    >
                      <MoreHorizontal size={12} />
                    </button>
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Portal>
                    <DropdownMenu.Content
                      sideOffset={4}
                      className="z-[60] min-w-36 rounded-lg border border-edge bg-panel p-1 shadow-xl"
                    >
                      <DropdownMenu.Item
                        className="cursor-pointer rounded px-2 py-1.5 text-xs text-white outline-none hover:bg-edge"
                        onSelect={() => onRename(board)}
                      >
                        <span className="inline-flex items-center gap-2">
                          <Pencil size={11} />
                          Rename
                        </span>
                      </DropdownMenu.Item>
                      <DropdownMenu.Item
                        className="cursor-pointer rounded px-2 py-1.5 text-xs text-red-300 outline-none hover:bg-red-500/10"
                        onSelect={() => onDelete(board)}
                        disabled={boards.length <= 1}
                      >
                        <span className="inline-flex items-center gap-2">
                          <Trash2 size={11} />
                          Delete
                        </span>
                      </DropdownMenu.Item>
                    </DropdownMenu.Content>
                  </DropdownMenu.Portal>
                </DropdownMenu.Root>
              </div>
            ))}
            <Select.Separator className="my-1 h-px bg-edge" />
            <Select.Item
              value="__create__"
              className="flex items-center gap-2 px-2 py-1.5 text-sm text-accent-hover outline-none cursor-pointer data-[highlighted]:bg-accent/20"
            >
              <Plus size={12} />
              <Select.ItemText>New board…</Select.ItemText>
            </Select.Item>
          </Select.Viewport>
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  );
}

function BoardFormDialog({
  mode,
  board,
  isPending,
  onCancel,
  onSubmit,
}: {
  mode: 'create' | 'edit';
  board: ProjectBoard | null;
  isPending: boolean;
  onCancel: () => void;
  onSubmit: (payload: { name: string; prefix: string }) => void;
}) {
  const [name, setName] = useState(board?.name ?? '');
  const [prefix, setPrefix] = useState(board?.prefix ?? '');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setName(board?.name ?? '');
    setPrefix(board?.prefix ?? '');
    setError(null);
  }, [board]);

  function handleSubmit() {
    const trimmedName = name.trim();
    const normalizedPrefix = prefix.trim().toUpperCase();
    if (!trimmedName) {
      setError('Name is required');
      return;
    }
    if (!/^[A-Z]{2,8}$/.test(normalizedPrefix)) {
      setError('Prefix must be 2–8 uppercase letters');
      return;
    }
    setError(null);
    onSubmit({ name: trimmedName, prefix: normalizedPrefix });
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div className="w-full max-w-md rounded-2xl border border-edge bg-panel p-4 shadow-2xl">
        <h4 className="text-sm font-semibold text-white">
          {mode === 'edit' ? 'Rename board' : 'New board'}
        </h4>
        <div className="mt-3 space-y-3">
          <div>
            <label className="mb-1 block text-[10px] uppercase tracking-wider text-muted">
              Name
            </label>
            <Input
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="e.g. Plugin work"
              className="bg-background px-3 py-2 text-sm"
            />
          </div>
          <div>
            <label className="mb-1 block text-[10px] uppercase tracking-wider text-muted">
              Prefix (2–8 uppercase letters)
            </label>
            <Input
              value={prefix}
              onChange={(event) => setPrefix(event.target.value.toUpperCase())}
              placeholder="e.g. PLUGIN"
              className="bg-background px-3 py-2 font-mono text-sm uppercase tracking-wider"
              maxLength={8}
            />
          </div>
          {error && <p className="text-xs text-red-300">{error}</p>}
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-lg border border-edge px-3 py-1.5 text-xs text-muted transition-colors hover:text-white"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={isPending}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-accent-hover disabled:opacity-50"
          >
            {mode === 'edit' ? 'Save' : 'Create'}
          </button>
        </div>
      </div>
    </div>
  );
}

function DeleteBoardDialog({
  board,
  boards,
  onCancel,
  onConfirm,
}: {
  board: ProjectBoard;
  boards: ProjectBoard[];
  onCancel: () => void;
  onConfirm: (destinationBoardId?: string) => void;
}) {
  const [destinationBoardId, setDestinationBoardId] = useState('');
  const destinationOptions = boards.filter((candidate) => candidate.id !== board.id);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div className="w-full max-w-md rounded-2xl border border-edge bg-panel p-4 shadow-2xl">
        <h4 className="text-sm font-semibold text-white">Delete board</h4>
        <p className="mt-2 text-xs text-muted">
          Move work items and columns out of {board.name} before deleting it. Existing items will
          be re-parented to the selected destination board.
        </p>
        <div className="mt-3">
          <SimpleSelect
            value={destinationBoardId}
            onValueChange={setDestinationBoardId}
            placeholder="Choose destination board…"
            className="bg-background px-3 py-2"
            options={destinationOptions.map((option) => ({ value: option.id, label: option.name }))}
          />
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-lg border border-edge px-3 py-1.5 text-xs text-muted transition-colors hover:text-white"
          >
            Cancel
          </button>
          <button
            onClick={() => onConfirm(destinationBoardId || undefined)}
            className="rounded-lg bg-red-500 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-red-400 disabled:opacity-50"
          >
            Delete board
          </button>
        </div>
      </div>
    </div>
  );
}

function roleLabel(role: WorkItemStatus): string {
  return role.replace(/_/g, ' ');
}

function roleTone(role: WorkItemStatus): string {
  switch (role) {
    case 'todo':
      return 'bg-secondary/10 text-secondary';
    case 'in_progress':
      return 'bg-blue-500/10 text-blue-300';
    case 'blocked':
      return 'bg-red-500/10 text-red-300';
    case 'review':
      return 'bg-amber-500/10 text-amber-300';
    case 'done':
      return 'bg-emerald-500/10 text-emerald-300';
    case 'cancelled':
      return 'bg-edge text-muted';
    default:
      return 'bg-edge text-muted';
  }
}
