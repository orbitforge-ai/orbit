import { useMemo } from 'react';
import { generateHTML } from '@tiptap/html';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { baseExtensions } from './extensions';
import { cn } from '../../lib/cn';

interface RichTextViewerProps {
  value: string | null | undefined;
  emptyFallback?: string;
  className?: string;
  onClick?: () => void;
}

/** Returns the Tiptap doc parsed from `value`, or null if the value isn't
 *  Tiptap JSON. Plain strings (including markdown) are handled by the
 *  markdown renderer branch below. */
function tryParseTiptapDoc(value: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(value);
    if (
      parsed &&
      typeof parsed === 'object' &&
      (parsed as { type?: unknown }).type === 'doc'
    ) {
      return parsed as Record<string, unknown>;
    }
  } catch {
    // not JSON
  }
  return null;
}

export function RichTextViewer({
  value,
  emptyFallback = 'No description.',
  className,
  onClick,
}: RichTextViewerProps) {
  const extensions = useMemo(() => baseExtensions(), []);
  const trimmed = (value ?? '').trim();

  const html = useMemo(() => {
    if (!trimmed) return null;
    const doc = tryParseTiptapDoc(trimmed);
    if (!doc) return null;
    try {
      return generateHTML(doc as Parameters<typeof generateHTML>[0], extensions);
    } catch {
      return null;
    }
  }, [trimmed, extensions]);

  if (!trimmed) {
    return (
      <div
        className={cn(
          'prose-orbit px-3 py-3 text-sm text-muted italic',
          onClick && 'cursor-text rounded-lg hover:bg-edge/30 transition-colors',
          className,
        )}
        onClick={onClick}
      >
        {emptyFallback}
      </div>
    );
  }

  if (html) {
    return (
      <div
        className={cn(
          'prose-orbit px-3 py-3 text-sm text-white',
          onClick && 'cursor-text rounded-lg hover:bg-edge/30 transition-colors',
          className,
        )}
        onClick={onClick}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    );
  }

  return (
    <div
      className={cn(
        'prose-orbit px-3 py-3 text-sm text-white',
        onClick && 'cursor-text rounded-lg hover:bg-edge/30 transition-colors',
        className,
      )}
      onClick={onClick}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{trimmed}</ReactMarkdown>
    </div>
  );
}
