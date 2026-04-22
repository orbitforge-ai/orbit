import { useMemo, useRef, useState } from 'react';
import { Tag, X } from 'lucide-react';
import { cn } from '../../../lib/cn';

interface Props {
  value: string[];
  onChange: (next: string[]) => void;
  suggestions?: string[];
  placeholder?: string;
}

export function LabelChipPicker({
  value,
  onChange,
  suggestions = [],
  placeholder = 'Add label…',
}: Props) {
  const [draft, setDraft] = useState('');
  const [open, setOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const filteredSuggestions = useMemo(() => {
    const taken = new Set(value);
    const q = draft.trim().toLowerCase();
    return suggestions
      .filter((s) => !taken.has(s) && (q === '' || s.includes(q)))
      .slice(0, 6);
  }, [suggestions, value, draft]);

  function commit(raw: string) {
    const next = raw.trim().toLowerCase();
    if (!next) return;
    if (value.includes(next)) {
      setDraft('');
      return;
    }
    onChange([...value, next]);
    setDraft('');
  }

  function remove(label: string) {
    onChange(value.filter((l) => l !== label));
  }

  return (
    <div className="relative">
      <div
        className={cn(
          'flex flex-wrap items-center gap-1 rounded-lg border border-edge bg-background/50 px-2 py-1.5 text-xs transition-colors',
          'focus-within:border-accent',
        )}
        onClick={() => inputRef.current?.focus()}
      >
        <Tag size={11} className="text-muted shrink-0" />
        {value.map((label) => (
          <span
            key={label}
            className="inline-flex items-center gap-1 rounded-full bg-accent/15 px-2 py-0.5 text-[11px] font-medium text-accent-light"
          >
            {label}
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                remove(label);
              }}
              className="rounded-full text-accent-light/70 hover:text-white"
              aria-label={`Remove ${label}`}
            >
              <X size={10} />
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => {
            // Delay close so clicks on suggestions register.
            window.setTimeout(() => setOpen(false), 120);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ',') {
              e.preventDefault();
              commit(draft);
            } else if (e.key === 'Backspace' && draft === '' && value.length > 0) {
              e.preventDefault();
              onChange(value.slice(0, -1));
            } else if (e.key === 'Escape') {
              setOpen(false);
            }
          }}
          placeholder={value.length === 0 ? placeholder : ''}
          className="flex-1 min-w-[80px] bg-transparent text-xs text-white outline-none placeholder:text-muted"
        />
      </div>

      {open && filteredSuggestions.length > 0 && (
        <div className="absolute z-10 mt-1 w-full overflow-hidden rounded-lg border border-edge bg-panel shadow-xl">
          {filteredSuggestions.map((s) => (
            <button
              key={s}
              type="button"
              onMouseDown={(e) => {
                e.preventDefault();
                commit(s);
              }}
              className="block w-full px-3 py-1.5 text-left text-xs text-secondary transition-colors hover:bg-edge hover:text-white"
            >
              {s}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
