import { useState, useRef } from 'react';

interface InlineEditProps {
  value: string;
  placeholder?: string;
  onSave: (value: string) => void | Promise<void>;
  className?: string;
  inputClassName?: string;
  as?: 'h3' | 'span' | 'p';
}

export function InlineEdit({
  value,
  placeholder,
  onSave,
  className = '',
  inputClassName = '',
  as: Tag = 'span',
}: InlineEditProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  function startEditing() {
    setDraft(value);
    setIsEditing(true);
    setTimeout(() => inputRef.current?.select(), 0);
  }

  async function save() {
    const trimmed = draft.trim();
    if (trimmed !== value) {
      await onSave(trimmed);
    }
    setIsEditing(false);
  }

  function cancel() {
    setIsEditing(false);
  }

  if (isEditing) {
    return (
      <input
        ref={inputRef}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={save}
        onKeyDown={(e) => {
          if (e.key === 'Enter') save();
          if (e.key === 'Escape') cancel();
        }}
        placeholder={placeholder}
        className={`bg-background border border-accent rounded px-1.5 py-0.5 focus:outline-none focus:border-accent-hover ${inputClassName}`}
        autoFocus
      />
    );
  }

  return (
    <Tag
      onClick={startEditing}
      className={`rounded px-1.5 py-0.5 -mx-1.5 border border-transparent hover:border-edge cursor-pointer transition-colors ${className}`}
      title="Click to edit"
    >
      {value || <span className="text-muted italic">{placeholder ?? 'Click to edit'}</span>}
    </Tag>
  );
}
