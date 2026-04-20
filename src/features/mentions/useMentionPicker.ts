import { useCallback, useEffect, useMemo, useState } from 'react';
import { encodeMention } from './tokenize';
import { useMentionGroups } from './dataSources';
import { MentionGroup, MentionToken, PickerContext } from './types';

export type MentionTrigger = '@' | '#';

interface ActiveMention {
  trigger: MentionTrigger;
  query: string;
  anchor: number;
  caret: number;
}

interface Args {
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
  text: string;
  setText: (value: string) => void;
  pickerContext: PickerContext | null;
}

interface FlatOption {
  groupIndex: number;
  itemIndex: number;
  token: MentionToken;
}

function detectActive(text: string, caret: number): ActiveMention | null {
  let i = caret - 1;
  while (i >= 0) {
    const ch = text[i];
    if (ch === '@' || ch === '#') {
      if (i > 0) {
        const prev = text[i - 1];
        if (prev && !/\s/.test(prev) && prev !== '(' && prev !== '[') {
          return null;
        }
      }
      const query = text.slice(i + 1, caret);
      if (/\s/.test(query)) return null;
      if (query.includes('(') || query.includes(')') || query.includes('[') || query.includes(']')) {
        return null;
      }
      return { trigger: ch as MentionTrigger, query, anchor: i, caret };
    }
    if (/\s/.test(ch)) return null;
    i -= 1;
  }
  return null;
}

export function useMentionPicker({ textareaRef, text, setText, pickerContext }: Args) {
  const [caret, setCaret] = useState(0);
  const [manuallyClosed, setManuallyClosed] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(0);

  const active = useMemo(() => detectActive(text, caret), [text, caret]);
  const open = Boolean(active) && !manuallyClosed;

  const groups = useMentionGroups({
    trigger: open && active ? active.trigger : null,
    query: active?.query ?? '',
    currentAgentId: pickerContext?.agentId ?? null,
    projectId: pickerContext?.projectId ?? null,
  });

  const flatOptions = useMemo<FlatOption[]>(() => {
    const flat: FlatOption[] = [];
    groups.forEach((group, groupIndex) => {
      group.items.forEach((item, itemIndex) => {
        flat.push({ groupIndex, itemIndex, token: item.token });
      });
    });
    return flat;
  }, [groups]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [active?.trigger, active?.query]);

  useEffect(() => {
    if (!active) setManuallyClosed(false);
  }, [active]);

  const syncCaret = useCallback(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    setCaret(ta.selectionStart ?? 0);
  }, [textareaRef]);

  useEffect(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    const handler = () => syncCaret();
    ta.addEventListener('keyup', handler);
    ta.addEventListener('click', handler);
    ta.addEventListener('select', handler);
    ta.addEventListener('focus', handler);
    return () => {
      ta.removeEventListener('keyup', handler);
      ta.removeEventListener('click', handler);
      ta.removeEventListener('select', handler);
      ta.removeEventListener('focus', handler);
    };
  }, [syncCaret, textareaRef]);

  const insertMention = useCallback(
    (token: MentionToken) => {
      if (!active) return;
      const encoded = encodeMention(token);
      const before = text.slice(0, active.anchor);
      const after = text.slice(active.caret);
      const inserted = `${before}${encoded} ${after}`;
      setText(inserted);
      setManuallyClosed(false);
      requestAnimationFrame(() => {
        const ta = textareaRef.current;
        if (!ta) return;
        const newCaret = before.length + encoded.length + 1;
        ta.focus();
        ta.setSelectionRange(newCaret, newCaret);
        setCaret(newCaret);
      });
    },
    [active, setText, text, textareaRef],
  );

  const close = useCallback(() => {
    setManuallyClosed(true);
  }, []);

  const cycleSelection = useCallback(
    (delta: number) => {
      if (flatOptions.length === 0) return;
      setSelectedIndex((prev) => {
        const next = (prev + delta + flatOptions.length) % flatOptions.length;
        return next;
      });
    },
    [flatOptions.length],
  );

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (!open) return false;
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        cycleSelection(1);
        return true;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        cycleSelection(-1);
        return true;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        const option = flatOptions[selectedIndex];
        if (!option) return false;
        e.preventDefault();
        insertMention(option.token);
        return true;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        close();
        return true;
      }
      return false;
    },
    [close, cycleSelection, flatOptions, insertMention, open, selectedIndex],
  );

  const selectedFlatIndex = flatOptions.length > 0 ? selectedIndex % flatOptions.length : -1;

  const decoratedGroups: MentionGroup[] = useMemo(() => {
    return groups.map((group, groupIndex) => ({
      ...group,
      items: group.items.map((item, itemIndex) => {
        const flatIdx = flatOptions.findIndex(
          (opt) => opt.groupIndex === groupIndex && opt.itemIndex === itemIndex,
        );
        return {
          ...item,
          // attach a selection flag for the popup
          __selected: flatIdx === selectedFlatIndex,
        } as typeof item & { __selected: boolean };
      }),
    }));
  }, [groups, flatOptions, selectedFlatIndex]);

  return {
    open,
    trigger: active?.trigger ?? null,
    query: active?.query ?? '',
    groups: decoratedGroups,
    onKeyDown,
    onSelect: insertMention,
    close,
    syncCaret,
  };
}

export type MentionPickerApi = ReturnType<typeof useMentionPicker>;
