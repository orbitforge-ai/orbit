import { useCallback, useEffect, useMemo, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { workItemsApi } from '../../../api/workItems';
import { UpdateWorkItem, WorkItem, WorkItemKind } from '../../../types';

export interface UseWorkItemEditor {
  title: string;
  setTitle: (next: string) => void;
  commitTitle: () => void;

  description: string;
  setDescription: (next: string) => void;
  commitDescription: () => void;
  descriptionDirty: boolean;
  resetDescription: () => void;

  labels: string[];
  setLabels: (next: string[]) => void;

  saveKind: (kind: WorkItemKind) => void;
  savePriority: (priority: number) => void;
  saveColumn: (columnId: string) => void;

  isSaving: boolean;
}

/**
 * Centralises dirty tracking + mutation for the draft fields (title,
 * description, labels) and provides thin passthroughs for instant-write
 * fields so callers don't have to wire up individual mutations.
 */
export function useWorkItemEditor(projectId: string, item: WorkItem | undefined): UseWorkItemEditor {
  const queryClient = useQueryClient();
  const workItemId = item?.id ?? '';

  const [title, setTitleRaw] = useState('');
  const [description, setDescriptionRaw] = useState('');
  const [labels, setLabelsRaw] = useState<string[]>([]);

  const [descriptionDirty, setDescriptionDirty] = useState(false);
  const [titleDirty, setTitleDirty] = useState(false);

  useEffect(() => {
    if (!item) return;
    setTitleRaw(item.title);
    setDescriptionRaw(item.description ?? '');
    setLabelsRaw(item.labels);
    setTitleDirty(false);
    setDescriptionDirty(false);
  }, [item?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const invalidate = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ['work-items', projectId] });
    if (workItemId) {
      queryClient.invalidateQueries({ queryKey: ['work-items', projectId, workItemId] });
      queryClient.invalidateQueries({ queryKey: ['work-items', workItemId, 'events'] });
    }
  }, [queryClient, projectId, workItemId]);

  const updateMutation = useMutation({
    mutationFn: (payload: UpdateWorkItem) => workItemsApi.update(workItemId, payload),
    onSuccess: invalidate,
  });

  const commitTitle = useCallback(() => {
    if (!item) return;
    if (!titleDirty) return;
    const next = title.trim();
    if (!next || next === item.title) {
      setTitleRaw(item.title);
      setTitleDirty(false);
      return;
    }
    updateMutation.mutate({ title: next });
    setTitleDirty(false);
  }, [item, title, titleDirty, updateMutation]);

  const commitDescription = useCallback(() => {
    if (!item) return;
    if (!descriptionDirty) return;
    updateMutation.mutate({ description });
    setDescriptionDirty(false);
  }, [item, description, descriptionDirty, updateMutation]);

  const setTitle = useCallback((next: string) => {
    setTitleRaw(next);
    setTitleDirty(true);
  }, []);

  const setDescription = useCallback((next: string) => {
    setDescriptionRaw(next);
    setDescriptionDirty(true);
  }, []);

  const resetDescription = useCallback(() => {
    setDescriptionRaw(item?.description ?? '');
    setDescriptionDirty(false);
  }, [item?.description]);

  const setLabels = useCallback(
    (next: string[]) => {
      const normalised = Array.from(
        new Set(next.map((l) => l.trim().toLowerCase()).filter(Boolean)),
      );
      setLabelsRaw(normalised);
      if (!item) return;
      const same =
        normalised.length === item.labels.length &&
        normalised.every((l, idx) => l === item.labels[idx]);
      if (same) return;
      updateMutation.mutate({ labels: normalised });
    },
    [item, updateMutation],
  );

  const saveKind = useCallback(
    (kind: WorkItemKind) => updateMutation.mutate({ kind }),
    [updateMutation],
  );
  const savePriority = useCallback(
    (priority: number) => updateMutation.mutate({ priority }),
    [updateMutation],
  );
  const saveColumn = useCallback(
    (columnId: string) => updateMutation.mutate({ columnId }),
    [updateMutation],
  );

  return useMemo(
    () => ({
      title,
      setTitle,
      commitTitle,
      description,
      setDescription,
      commitDescription,
      descriptionDirty,
      resetDescription,
      labels,
      setLabels,
      saveKind,
      savePriority,
      saveColumn,
      isSaving: updateMutation.isPending,
    }),
    [
      title,
      setTitle,
      commitTitle,
      description,
      setDescription,
      commitDescription,
      descriptionDirty,
      resetDescription,
      labels,
      setLabels,
      saveKind,
      savePriority,
      saveColumn,
      updateMutation.isPending,
    ],
  );
}
