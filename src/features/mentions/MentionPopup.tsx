import { useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Bot, FileText, CheckSquare } from 'lucide-react';
import { getCaretCoords } from './caret';
import { MentionGroup, MentionItem, MentionToken } from './types';

const ICON_BY_KIND = {
  agent: Bot,
  file: FileText,
  item: CheckSquare,
} as const;

interface Props {
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
  open: boolean;
  groups: (MentionGroup & { items: (MentionItem & { __selected?: boolean })[] })[];
  onSelect: (token: MentionToken) => void;
  onClose: () => void;
}

const POPUP_WIDTH = 320;
const POPUP_MAX_HEIGHT = 280;
const VIEWPORT_PADDING = 12;

export function MentionPopup({ textareaRef, open, groups, onSelect, onClose }: Props) {
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const popupRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const ta = textareaRef.current;
    if (!ta) return;
    const update = () => {
      const coords = getCaretCoords(ta);
      const viewportWidth = window.innerWidth;
      let left = coords.left;
      let top = coords.top - POPUP_MAX_HEIGHT - 6;
      if (top < VIEWPORT_PADDING) {
        top = coords.top + coords.lineHeight + 6;
      }
      if (left + POPUP_WIDTH > viewportWidth - VIEWPORT_PADDING) {
        left = viewportWidth - POPUP_WIDTH - VIEWPORT_PADDING;
      }
      left = Math.max(VIEWPORT_PADDING, left);
      top = Math.max(VIEWPORT_PADDING, top);
      setPos({ left, top });
    };
    update();
    const onScroll = () => update();
    window.addEventListener('resize', update);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [open, textareaRef, groups]);

  useLayoutEffect(() => {
    if (!open) return;
    const handleDocClick = (e: MouseEvent) => {
      const target = e.target as Node;
      if (popupRef.current?.contains(target)) return;
      if (textareaRef.current?.contains(target)) return;
      onClose();
    };
    document.addEventListener('mousedown', handleDocClick);
    return () => document.removeEventListener('mousedown', handleDocClick);
  }, [open, onClose, textareaRef]);

  if (!open || !pos) return null;

  const isEmpty = groups.every((g) => g.items.length === 0);

  return createPortal(
    <div
      ref={popupRef}
      role="listbox"
      className="fixed z-[70] rounded-xl border border-edge bg-surface/95 shadow-xl backdrop-blur-sm"
      style={{
        left: pos.left,
        top: pos.top,
        width: POPUP_WIDTH,
        maxHeight: POPUP_MAX_HEIGHT,
      }}
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
                const selected = item.__selected;
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
}
