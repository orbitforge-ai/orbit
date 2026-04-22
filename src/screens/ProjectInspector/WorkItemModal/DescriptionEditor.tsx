import { useEffect, useRef, useState } from 'react';
import { Pencil } from 'lucide-react';
import { confirm } from '@tauri-apps/plugin-dialog';
import { RichTextEditor } from '../../../components/RichTextEditor/RichTextEditor';
import { RichTextViewer } from '../../../components/RichTextEditor/RichTextViewer';
import { cn } from '../../../lib/cn';

interface Props {
  value: string;
  dirty: boolean;
  onChange: (next: string) => void;
  onSave: () => void;
  onReset: () => void;
  /** Parent controls the editing flag so it can block modal close while open. */
  isEditing: boolean;
  onEditingChange: (next: boolean) => void;
}

export function DescriptionEditor({
  value,
  dirty,
  onChange,
  onSave,
  onReset,
  isEditing,
  onEditingChange,
}: Props) {
  const [editorKey, setEditorKey] = useState(0);
  const savedValueRef = useRef(value);

  useEffect(() => {
    if (!isEditing) {
      savedValueRef.current = value;
    }
  }, [isEditing, value]);

  async function handleCancel() {
    if (dirty) {
      const ok = await confirm('Discard unsaved description changes?', {
        kind: 'warning',
      });
      if (!ok) return;
    }
    onReset();
    onEditingChange(false);
    setEditorKey((k) => k + 1);
  }

  function handleSave() {
    onSave();
    onEditingChange(false);
  }

  return (
    <div>
      <div className="mb-1.5 flex items-center justify-between">
        <h4 className="text-[10px] uppercase tracking-wide text-muted">Description</h4>
        {!isEditing && (
          <button
            type="button"
            onClick={() => onEditingChange(true)}
            className="inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-[11px] text-muted transition-colors hover:bg-edge hover:text-white"
          >
            <Pencil size={11} />
            Edit
          </button>
        )}
      </div>

      {isEditing ? (
        <div className="space-y-2">
          <RichTextEditor
            key={editorKey}
            initialValue={savedValueRef.current}
            onChange={onChange}
            placeholder="Describe the work…"
            autoFocus
            minHeight={220}
          />
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleSave}
              disabled={!dirty}
              className={cn(
                'rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors',
                'hover:bg-accent-hover disabled:opacity-50',
              )}
            >
              Save
            </button>
            <button
              type="button"
              onClick={handleCancel}
              className="text-xs text-muted transition-colors hover:text-white"
            >
              Cancel
            </button>
            <span className="ml-auto text-[10px] text-muted">Esc to cancel</span>
          </div>
        </div>
      ) : (
        <RichTextViewer
          value={value}
          onClick={() => onEditingChange(true)}
          emptyFallback="Click to add a description…"
          className="min-h-[60px] border border-transparent hover:border-edge"
        />
      )}
    </div>
  );
}
