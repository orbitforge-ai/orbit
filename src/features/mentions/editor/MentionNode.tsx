import { Node, mergeAttributes } from '@tiptap/core';
import { NodeViewWrapper, ReactNodeViewRenderer, ReactNodeViewProps } from '@tiptap/react';
import { Bot, FileText, CheckSquare } from 'lucide-react';
import { MentionKind } from '../types';
import { encodeMention } from '../tokenize';

const STYLES: Record<MentionKind, string> = {
  agent: 'bg-violet-500/15 text-violet-300 border-violet-500/30',
  file: 'bg-sky-500/15 text-sky-300 border-sky-500/30',
  item: 'bg-amber-500/15 text-amber-300 border-amber-500/30',
};

const ICONS: Record<MentionKind, typeof Bot> = {
  agent: Bot,
  file: FileText,
  item: CheckSquare,
};

function MentionView(props: ReactNodeViewProps) {
  const kind = (props.node.attrs.kind ?? 'agent') as MentionKind;
  const label = (props.node.attrs.label ?? '') as string;
  const Icon = ICONS[kind] ?? Bot;
  return (
    <NodeViewWrapper
      as="span"
      className={`inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] font-medium align-baseline mx-0.5 select-none ${STYLES[kind] ?? STYLES.agent}`}
      data-mention-kind={kind}
      contentEditable={false}
    >
      <Icon size={11} />
      <span>{label}</span>
    </NodeViewWrapper>
  );
}

export const MentionNode = Node.create({
  name: 'mention',
  group: 'inline',
  inline: true,
  atom: true,
  selectable: false,
  draggable: false,

  addAttributes() {
    return {
      kind: { default: 'agent' as MentionKind },
      payload: { default: '' },
      label: { default: '' },
    };
  },

  parseHTML() {
    return [{ tag: 'span[data-mention-kind]' }];
  },

  renderHTML({ node, HTMLAttributes }) {
    return [
      'span',
      mergeAttributes(HTMLAttributes, {
        'data-mention-kind': node.attrs.kind,
        'data-mention-payload': node.attrs.payload,
      }),
      node.attrs.label,
    ];
  },

  renderText({ node }) {
    return encodeMention({
      kind: node.attrs.kind,
      label: node.attrs.label,
      payload: node.attrs.payload,
    });
  },

  addNodeView() {
    return ReactNodeViewRenderer(MentionView);
  },
});
