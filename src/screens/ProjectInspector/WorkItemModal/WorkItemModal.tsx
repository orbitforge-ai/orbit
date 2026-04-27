import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { confirm } from '../../../lib/dialog';
import { workItemsApi } from '../../../api/workItems';
import { projectsApi } from '../../../api/projects';
import type { Agent, ProjectBoardColumn, WorkItem } from '../../../types';
import { Modal } from '../../../components/ui/Modal';
import { ModalHeader } from './ModalHeader';
import { MainColumn } from './MainColumn';
import { Sidebar } from './Sidebar';
import { useWorkItemEditor } from './useWorkItemEditor';

interface Props {
  projectId: string;
  workItemId: string;
  boardPrefix: string | null;
  agents: Agent[];
  onClose: () => void;
}

export function WorkItemModal({
  projectId,
  workItemId,
  boardPrefix,
  agents,
  onClose,
}: Props) {
  const queryClient = useQueryClient();

  const { data: item } = useQuery<WorkItem>({
    queryKey: ['work-items', projectId, workItemId],
    queryFn: () => workItemsApi.get(workItemId),
  });

  const { data: columns = [] } = useQuery<ProjectBoardColumn[]>({
    queryKey: ['project-board-columns', projectId],
    queryFn: () => projectsApi.listBoardColumns(projectId),
  });

  const { data: allItems = [] } = useQuery<WorkItem[]>({
    queryKey: ['work-items', projectId],
    queryFn: () => workItemsApi.list(projectId),
  });

  const labelSuggestions = useMemo(() => {
    const set = new Set<string>();
    for (const wi of allItems) for (const l of wi.labels) set.add(l);
    return Array.from(set).sort();
  }, [allItems]);

  const editor = useWorkItemEditor(projectId, item);
  const [isEditingDescription, setIsEditingDescription] = useState(false);

  function invalidate() {
    queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    queryClient.invalidateQueries({ queryKey: ['work-items', projectId, workItemId] });
    queryClient.invalidateQueries({ queryKey: ['work-items', workItemId, 'events'] });
  }

  const moveMutation = useMutation({
    mutationFn: ({ columnId }: { columnId: string }) => workItemsApi.move(workItemId, columnId),
    onSuccess: invalidate,
  });
  const blockMutation = useMutation({
    mutationFn: ({ reason }: { reason: string }) => workItemsApi.block(workItemId, reason),
    onSuccess: invalidate,
  });
  const completeMutation = useMutation({
    mutationFn: () => workItemsApi.complete(workItemId),
    onSuccess: invalidate,
  });

  async function handleColumnChange(nextColumnId: string) {
    const nextColumn = columns.find((c) => c.id === nextColumnId);
    if (!nextColumn) return;
    if (nextColumn.role === 'blocked') {
      const reason = window.prompt('Why is this card blocked?');
      if (!reason || !reason.trim()) return;
      blockMutation.mutate({ reason: reason.trim() });
      return;
    }
    if (nextColumn.role === 'done') {
      completeMutation.mutate();
      return;
    }
    moveMutation.mutate({ columnId: nextColumnId });
  }

  async function guardedClose(): Promise<boolean> {
    if (isEditingDescription && editor.descriptionDirty) {
      const ok = await confirm('Discard unsaved description changes?', {
        kind: 'warning',
      });
      return ok;
    }
    return true;
  }

  return (
    <Modal
      open
      onClose={onClose}
      size="xl"
      ariaTitle={item?.title ?? 'Work item'}
      onBeforeClose={guardedClose}
    >
      {!item ? (
        <div className="flex min-h-[400px] items-center justify-center text-sm text-muted">
          Loading…
        </div>
      ) : (
        <>
          <ModalHeader
            item={item}
            boardPrefix={boardPrefix}
            title={editor.title}
            onTitleChange={editor.setTitle}
            onTitleCommit={editor.commitTitle}
            onComplete={() => completeMutation.mutate()}
            onClose={async () => {
              if (await guardedClose()) onClose();
            }}
          />
          <div className="flex min-h-0 flex-1 overflow-hidden">
            <MainColumn
              item={item}
              projectId={projectId}
              agents={agents}
              description={editor.description}
              descriptionDirty={editor.descriptionDirty}
              onDescriptionChange={editor.setDescription}
              onDescriptionSave={editor.commitDescription}
              onDescriptionReset={editor.resetDescription}
              isEditingDescription={isEditingDescription}
              onEditingDescriptionChange={setIsEditingDescription}
            />
            <Sidebar
              item={item}
              projectId={projectId}
              columns={columns}
              agents={agents}
              labels={editor.labels}
              labelSuggestions={labelSuggestions}
              onLabelsChange={editor.setLabels}
              onKindChange={editor.saveKind}
              onPriorityChange={editor.savePriority}
              onColumnChange={handleColumnChange}
              onDeleted={onClose}
            />
          </div>
        </>
      )}
    </Modal>
  );
}
