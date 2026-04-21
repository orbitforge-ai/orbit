import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import { Bot, FileText, CheckSquare, Sparkles } from 'lucide-react';
import { useMentionGroups } from '../dataSources';
import { MentionItem, MentionKind, MentionToken, PickerContext } from '../types';

const ICON_BY_KIND: Record<MentionKind, typeof Bot> = {
  agent: Bot,
  file: FileText,
  item: CheckSquare,
  skill: Sparkles,
};

const POPUP_WIDTH = 320;
const POPUP_MAX_HEIGHT = 280;
const VIEWPORT_PADDING = 12;

export interface InputMentionPopupHandle {
  handleKey(event: KeyboardEvent): boolean;
}

interface Props {
  open: boolean;
  trigger: '@' | null;
  query: string;
  anchorRect: (() => DOMRect | null) | null;
  pickerContext: PickerContext | null;
  onSelect: (token: MentionToken) => void;
}

export const InputMentionPopup = forwardRef<InputMentionPopupHandle, Props>(function InputMentionPopup(
  { open, trigger, query, anchorRect, pickerContext, onSelect },
  ref,
) {
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);

  const groups = useMentionGroups({
    enabled: open,
    query,
    currentAgentId: pickerContext?.agentId ?? null,
    projectId: pickerContext?.projectId ?? null,
  });

  const flat = useMemo<MentionItem[]>(() => groups.flatMap((g) => g.items), [groups]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [trigger, query]);

  useLayoutEffect(() => {
    if (!open || !anchorRect) {
      setPos(null);
      return;
    }
    const update = () => {
      const rect = anchorRect();
      if (!rect) return;
      const viewportWidth = window.innerWidth;
      let left = rect.left;
      let top = rect.top - POPUP_MAX_HEIGHT - 6;
      if (top < VIEWPORT_PADDING) top = rect.bottom + 6;
      if (left + POPUP_WIDTH > viewportWidth - VIEWPORT_PADDING) {
        left = viewportWidth - POPUP_WIDTH - VIEWPORT_PADDING;
      }
      left = Math.max(VIEWPORT_PADDING, left);
      top = Math.max(VIEWPORT_PADDING, top);
      setPos({ left, top });
    };
    update();
    const handler = () => update();
    window.addEventListener('resize', handler);
    window.addEventListener('scroll', handler, true);
    return () => {
      window.removeEventListener('resize', handler);
      window.removeEventListener('scroll', handler, true);
    };
  }, [open, anchorRect, query, groups]);

  useImperativeHandle(
    ref,
    () => ({
      handleKey(event: KeyboardEvent) {
        if (!open || flat.length === 0) {
          if (open && event.key === 'Escape') return true;
          return false;
        }
        if (event.key === 'ArrowDown') {
          event.preventDefault();
          setSelectedIndex((i) => (i + 1) % flat.length);
          return true;
        }
        if (event.key === 'ArrowUp') {
          event.preventDefault();
          setSelectedIndex((i) => (i - 1 + flat.length) % flat.length);
          return true;
        }
        if (event.key === 'Enter' || event.key === 'Tab') {
          const item = flat[selectedIndex];
          if (!item) return false;
          event.preventDefault();
          onSelect(item.token);
          return true;
        }
        if (event.key === 'Escape') {
          return true;
        }
        return false;
      },
    }),
    [open, flat, selectedIndex, onSelect],
  );

  if (!open || !pos) return null;

  const isEmpty = flat.length === 0;
  let runningIndex = 0;

  return createPortal(
    <div
      role="listbox"
      className="fixed z-[70] rounded-xl border border-edge bg-surface/95 shadow-xl backdrop-blur-sm"
      style={{ left: pos.left, top: pos.top, width: POPUP_WIDTH, maxHeight: POPUP_MAX_HEIGHT }}
    >
      <div className="max-h-[280px] overflow-y-auto py-1">
        {isEmpty ? (
          <div className="px-3 py-4 text-center text-xs text-muted">No matches</div>
        ) : (
          groups.map((group) => (
            <div key={group.kind} className="py-1">
              <p className="px-3 py-1 text-[10px] font-semibold uppercase tracking-[0.16em] text-muted">
                {group.title}
              </p>
              {group.items.map((item) => {
                const Icon = ICON_BY_KIND[group.kind];
                const thisIndex = runningIndex++;
                const selected = thisIndex === selectedIndex;
                return (
                  <button
                    key={`${group.kind}:${item.id}`}
                    type="button"
                    role="option"
                    aria-selected={selected}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      onSelect(item.token);
                    }}
                    className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs outline-none transition-colors ${
                      selected ? 'bg-accent/15 text-white' : 'text-secondary hover:bg-surface-hover'
                    }`}
                  >
                    <Icon size={12} className="shrink-0 text-muted" />
                    <span className="flex-1 truncate">{item.label}</span>
                    {item.secondary && (
                      <span className="shrink-0 truncate text-[10px] text-muted max-w-[40%]">
                        {item.secondary}
                      </span>
                    )}
                  </button>
                );
              })}
              {group.truncated && (
                <p className="px-3 py-1 text-[10px] text-muted italic">Showing partial results</p>
              )}
            </div>
          ))
        )}
      </div>
    </div>,
    document.body,
  );
});
