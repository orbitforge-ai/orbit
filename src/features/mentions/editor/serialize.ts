import { JSONContent } from '@tiptap/core';
import { MENTION_REGEX, encodeMention } from '../tokenize';
import { MentionKind } from '../types';

export function stringToDoc(text: string): JSONContent {
  const paragraphs = text.split(/\n\n+/);
  const content = paragraphs.map(paragraphToNode);
  return { type: 'doc', content };
}

function paragraphToNode(text: string): JSONContent {
  if (text.length === 0) return { type: 'paragraph' };
  const lines = text.split('\n');
  const nodes: JSONContent[] = [];
  lines.forEach((line, idx) => {
    if (idx > 0) nodes.push({ type: 'hardBreak' });
    nodes.push(...inlineToNodes(line));
  });
  const filtered = nodes.filter((n) => !(n.type === 'text' && (!n.text || n.text.length === 0)));
  return filtered.length > 0 ? { type: 'paragraph', content: filtered } : { type: 'paragraph' };
}

function inlineToNodes(line: string): JSONContent[] {
  if (line.length === 0) return [];
  const nodes: JSONContent[] = [];
  let cursor = 0;
  MENTION_REGEX.lastIndex = 0;
  for (const match of line.matchAll(MENTION_REGEX)) {
    const groups = match.groups;
    if (!groups) continue;
    const start = match.index ?? 0;
    if (start > cursor) nodes.push({ type: 'text', text: line.slice(cursor, start) });
    nodes.push({
      type: 'mention',
      attrs: {
        kind: groups.kind as MentionKind,
        payload: groups.payload,
        label: groups.label,
      },
    });
    cursor = start + match[0].length;
  }
  if (cursor < line.length) nodes.push({ type: 'text', text: line.slice(cursor) });
  return nodes;
}

export function docToString(doc: JSONContent): string {
  if (!doc.content) return '';
  return doc.content.map(blockToString).join('\n\n');
}

function blockToString(block: JSONContent): string {
  if (block.type !== 'paragraph') return '';
  if (!block.content) return '';
  return block.content.map(inlineToString).join('');
}

function inlineToString(node: JSONContent): string {
  if (node.type === 'text') return node.text ?? '';
  if (node.type === 'hardBreak') return '\n';
  if (node.type === 'mention' && node.attrs) {
    return encodeMention({
      kind: node.attrs.kind as MentionKind,
      label: (node.attrs.label as string) ?? '',
      payload: (node.attrs.payload as string) ?? '',
    });
  }
  return '';
}
