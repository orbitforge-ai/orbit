import { ReactNode } from 'react';
import { Bot, FileText, CheckSquare } from 'lucide-react';
import { parseMentionHref } from './tokenize';

interface MentionPillProps {
  href: string;
  children: ReactNode;
}

const STYLES = {
  agent: 'bg-violet-500/15 text-violet-300 border-violet-500/30',
  file: 'bg-sky-500/15 text-sky-300 border-sky-500/30',
  item: 'bg-amber-500/15 text-amber-300 border-amber-500/30',
} as const;

export function MentionPill({ href, children }: MentionPillProps) {
  const token = parseMentionHref(href);
  const kind = token?.kind ?? 'agent';
  const Icon = kind === 'agent' ? Bot : kind === 'file' ? FileText : CheckSquare;

  return (
    <span
      className={`inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] font-medium align-baseline ${STYLES[kind]}`}
      data-mention-kind={kind}
    >
      <Icon size={11} />
      <span>{children}</span>
    </span>
  );
}
