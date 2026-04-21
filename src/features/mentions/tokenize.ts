import { MentionKind, MentionToken } from './types';

export const MENTION_REGEX =
  /(?<prefix>[@#])\[(?<label>[^\]]+)\]\(mention:(?<kind>agent|file|item|skill):(?<payload>[^)]+)\)/g;

const PREFIX_BY_KIND: Record<MentionKind, '@'> = {
  agent: '@',
  file: '@',
  item: '@',
  skill: '@',
};

function sanitizeLabel(label: string): string {
  return label.replace(/[\]()]/g, '·').replace(/\s+/g, ' ').trim() || 'mention';
}

function sanitizePayload(payload: string): string {
  return payload.replace(/[)]/g, '');
}

export function encodeMention(token: MentionToken): string {
  const prefix = PREFIX_BY_KIND[token.kind];
  return `${prefix}[${sanitizeLabel(token.label)}](mention:${token.kind}:${sanitizePayload(token.payload)})`;
}

export interface ParsedMention {
  token: MentionToken;
  start: number;
  end: number;
  match: string;
}

export function parseMentions(text: string): ParsedMention[] {
  const results: ParsedMention[] = [];
  for (const m of text.matchAll(MENTION_REGEX)) {
    const groups = m.groups;
    if (!groups) continue;
    const kind = groups.kind as MentionKind;
    const start = m.index ?? 0;
    results.push({
      token: { kind, label: groups.label, payload: groups.payload },
      start,
      end: start + m[0].length,
      match: m[0],
    });
  }
  return results;
}

export function parseMentionHref(href: string): MentionToken | null {
  if (!href.startsWith('mention:')) return null;
  const rest = href.slice('mention:'.length);
  const firstColon = rest.indexOf(':');
  if (firstColon < 0) return null;
  const kind = rest.slice(0, firstColon) as MentionKind;
  const payload = rest.slice(firstColon + 1);
  if (kind !== 'agent' && kind !== 'file' && kind !== 'item' && kind !== 'skill') return null;
  return { kind, label: '', payload };
}
