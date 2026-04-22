import { useState } from 'react';
import type { Agent, WorkItem } from '../../../types';
import { DescriptionEditor } from './DescriptionEditor';
import { TabBar, type WorkItemModalTab } from './TabBar';
import { CommentsTab } from './CommentsTab';
import { ActivityTab } from './ActivityTab';

interface Props {
  item: WorkItem;
  projectId: string;
  agents: Agent[];
  description: string;
  descriptionDirty: boolean;
  onDescriptionChange: (next: string) => void;
  onDescriptionSave: () => void;
  onDescriptionReset: () => void;
  isEditingDescription: boolean;
  onEditingDescriptionChange: (next: boolean) => void;
}

export function MainColumn({
  item,
  projectId,
  agents,
  description,
  descriptionDirty,
  onDescriptionChange,
  onDescriptionSave,
  onDescriptionReset,
  isEditingDescription,
  onEditingDescriptionChange,
}: Props) {
  const [tab, setTab] = useState<WorkItemModalTab>('comments');
  const [commentCount, setCommentCount] = useState<number | undefined>(undefined);

  return (
    <div className="flex flex-1 min-w-0 flex-col gap-4 overflow-y-auto px-5 py-4">
      <DescriptionEditor
        value={description}
        dirty={descriptionDirty}
        onChange={onDescriptionChange}
        onSave={onDescriptionSave}
        onReset={onDescriptionReset}
        isEditing={isEditingDescription}
        onEditingChange={onEditingDescriptionChange}
      />

      <div>
        <TabBar value={tab} onChange={setTab} commentCount={commentCount} />
        <div className="pt-3">
          {tab === 'comments' ? (
            <CommentsTab
              workItemId={item.id}
              projectId={projectId}
              agents={agents}
              onCountChange={setCommentCount}
            />
          ) : (
            <ActivityTab workItemId={item.id} agents={agents} />
          )}
        </div>
      </div>
    </div>
  );
}
