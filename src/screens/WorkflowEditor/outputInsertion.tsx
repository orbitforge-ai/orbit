import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { ReactNode } from 'react';

export type OutputInsertionMode = 'template' | 'raw';

interface ActiveField {
  id: string;
  insertPath: (path: string) => void;
}

interface OutputInsertionContextValue {
  activeFieldId: string | null;
  clearActiveField: (id: string) => void;
  hasActiveField: boolean;
  insertPath: (path: string) => void;
  setActiveField: (field: ActiveField) => void;
}

const OutputInsertionContext = createContext<OutputInsertionContextValue | null>(null);

export function OutputInsertionProvider({ children }: { children: ReactNode }) {
  const [activeField, setActiveFieldState] = useState<ActiveField | null>(null);

  const setActiveField = useCallback((field: ActiveField) => {
    setActiveFieldState(field);
  }, []);

  const clearActiveField = useCallback((id: string) => {
    setActiveFieldState((current) => (current?.id === id ? null : current));
  }, []);

  const insertPath = useCallback((path: string) => {
    activeField?.insertPath(path);
  }, [activeField]);

  const value = useMemo<OutputInsertionContextValue>(
    () => ({
      activeFieldId: activeField?.id ?? null,
      clearActiveField,
      hasActiveField: activeField !== null,
      insertPath,
      setActiveField,
    }),
    [activeField, clearActiveField, insertPath, setActiveField],
  );

  return (
    <OutputInsertionContext.Provider value={value}>
      {children}
    </OutputInsertionContext.Provider>
  );
}

export function useOutputInsertion() {
  return useContext(OutputInsertionContext);
}

export function useOutputInsertionField<
  TElement extends HTMLInputElement | HTMLTextAreaElement,
>({
  mode,
  onChange,
  value,
}: {
  mode: OutputInsertionMode;
  onChange: (value: string) => void;
  value: string;
}) {
  const context = useOutputInsertion();
  const id = useId();
  const elementRef = useRef<TElement | null>(null);
  const lastSelectionRef = useRef<{ end: number; start: number } | null>(null);
  const onChangeRef = useRef(onChange);
  const valueRef = useRef(value);

  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    valueRef.current = value;
  }, [value]);

  const captureSelection = useCallback(() => {
    const element = elementRef.current;
    if (!element) {
      return;
    }
    const fallback = valueRef.current.length;
    lastSelectionRef.current = {
      end: element.selectionEnd ?? fallback,
      start: element.selectionStart ?? fallback,
    };
  }, []);

  const insertPath = useCallback(
    (path: string) => {
      const insertion = mode === 'template' ? `{{${path}}}` : path;
      const currentValue = valueRef.current;
      const selection = lastSelectionRef.current ?? {
        end: currentValue.length,
        start: currentValue.length,
      };
      const start = Math.max(0, Math.min(selection.start, currentValue.length));
      const end = Math.max(start, Math.min(selection.end, currentValue.length));
      const nextValue = currentValue.slice(0, start) + insertion + currentValue.slice(end);
      const caret = start + insertion.length;
      lastSelectionRef.current = { end: caret, start: caret };
      onChangeRef.current(nextValue);
      requestAnimationFrame(() => {
        const element = elementRef.current;
        if (!element) {
          return;
        }
        element.focus();
        element.setSelectionRange(caret, caret);
      });
    },
    [mode],
  );

  const activateField = useCallback(() => {
    captureSelection();
    context?.setActiveField({ id, insertPath });
  }, [captureSelection, context, id, insertPath]);

  useEffect(() => () => context?.clearActiveField(id), [context, id]);

  return {
    active: context?.activeFieldId === id,
    bind: {
      onClick: activateField,
      onFocus: activateField,
      onKeyUp: captureSelection,
      onSelect: captureSelection,
      ref: elementRef,
    },
  };
}
