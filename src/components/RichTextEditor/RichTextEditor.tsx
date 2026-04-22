import { useEffect, useMemo } from 'react';
import { EditorContent, useEditor } from '@tiptap/react';
import type { JSONContent } from '@tiptap/core';
import { baseExtensions } from './extensions';
import { Toolbar } from './Toolbar';
import { cn } from '../../lib/cn';

interface RichTextEditorProps {
  /** Initial Tiptap JSON (stringified) or plain text — editor seeds from this once. */
  initialValue: string;
  /** Called whenever content changes with the current JSON-stringified doc. */
  onChange: (json: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  className?: string;
  minHeight?: number;
}

/** Try to parse a stored description value as Tiptap JSON; fall back to a
 *  paragraph doc containing the raw string. This preserves pre-Tiptap rows. */
export function parseRichTextValue(value: string | null | undefined): JSONContent {
  const trimmed = (value ?? '').trim();
  if (!trimmed) return { type: 'doc', content: [{ type: 'paragraph' }] };
  try {
    const parsed = JSON.parse(trimmed);
    if (parsed && typeof parsed === 'object' && parsed.type === 'doc') {
      return parsed as JSONContent;
    }
  } catch {
    // not JSON — fall through
  }
  return {
    type: 'doc',
    content: trimmed.split(/\n{2,}/).map((para) => ({
      type: 'paragraph',
      content: para
        ? para
            .split('\n')
            .flatMap((line, idx) =>
              idx === 0
                ? [{ type: 'text', text: line }]
                : [{ type: 'hardBreak' }, { type: 'text', text: line }],
            )
        : undefined,
    })),
  };
}

export function RichTextEditor({
  initialValue,
  onChange,
  placeholder,
  autoFocus,
  className,
  minHeight = 200,
}: RichTextEditorProps) {
  const extensions = useMemo(() => baseExtensions(placeholder), [placeholder]);
  const initialDoc = useMemo(() => parseRichTextValue(initialValue), [initialValue]);

  const editor = useEditor({
    extensions,
    content: initialDoc,
    autofocus: autoFocus ? 'end' : false,
    editorProps: {
      attributes: {
        class: cn(
          'prose-orbit focus:outline-none text-sm text-white px-3 py-3',
        ),
      },
    },
    onUpdate: ({ editor: ed }) => {
      onChange(JSON.stringify(ed.getJSON()));
    },
  });

  useEffect(() => {
    return () => {
      editor?.destroy();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      className={cn(
        'flex flex-col rounded-lg border border-edge bg-background/40 focus-within:border-accent transition-colors',
        className,
      )}
    >
      <Toolbar editor={editor} />
      <div className="overflow-y-auto" style={{ minHeight }}>
        <EditorContent editor={editor} />
      </div>
    </div>
  );
}
