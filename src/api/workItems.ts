import { invoke } from '@tauri-apps/api/core';
import {
  CommentAuthor,
  CreateWorkItem,
  UpdateWorkItem,
  WorkItem,
  WorkItemComment,
  WorkItemStatus,
} from '../types';

export const workItemsApi = {
  list: (projectId: string): Promise<WorkItem[]> =>
    invoke('list_work_items', { projectId }),

  get: (id: string): Promise<WorkItem> => invoke('get_work_item', { id }),

  create: (payload: CreateWorkItem): Promise<WorkItem> =>
    invoke('create_work_item', { payload }),

  update: (id: string, payload: UpdateWorkItem): Promise<WorkItem> =>
    invoke('update_work_item', { id, payload }),

  delete: (id: string): Promise<void> => invoke('delete_work_item', { id }),

  claim: (id: string, agentId: string): Promise<WorkItem> =>
    invoke('claim_work_item', { id, agentId }),

  move: (
    id: string,
    status?: WorkItemStatus,
    columnId?: string,
    position?: number,
  ): Promise<WorkItem> =>
    invoke('move_work_item', { id, status: status ?? null, columnId: columnId ?? null, position }),

  reorder: (
    projectId: string,
    status: WorkItemStatus | null,
    columnId: string | null,
    orderedIds: string[],
  ): Promise<void> =>
    invoke('reorder_work_items', { projectId, status, columnId, orderedIds }),

  block: (id: string, reason: string): Promise<WorkItem> =>
    invoke('block_work_item', { id, reason }),

  complete: (id: string): Promise<WorkItem> =>
    invoke('complete_work_item', { id }),

  // Comments
  listComments: (workItemId: string): Promise<WorkItemComment[]> =>
    invoke('list_work_item_comments', { workItemId }),

  createComment: (
    workItemId: string,
    body: string,
    author: CommentAuthor,
  ): Promise<WorkItemComment> =>
    invoke('create_work_item_comment', { workItemId, body, author }),

  updateComment: (id: string, body: string): Promise<WorkItemComment> =>
    invoke('update_work_item_comment', { id, body }),

  deleteComment: (id: string): Promise<void> =>
    invoke('delete_work_item_comment', { id }),
};
