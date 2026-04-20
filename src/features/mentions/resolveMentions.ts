import { chatApi } from '../../api/chat';
import { workItemsApi } from '../../api/workItems';
import { workspaceApi } from '../../api/workspace';
import { ChatModelOverride, ContentBlock } from '../../types';
import { parseMentions } from './tokenize';
import { MentionKind } from './types';

const FALLBACK_CONTEXT_WINDOW = 200_000;
const CHARS_PER_TOKEN = 4;

interface ResolveCtx {
  sessionId?: string;
  agentId: string | null;
  projectId: string | null;
  modelOverride?: ChatModelOverride | null;
}

interface Attachment {
  kind: MentionKind;
  label: string;
  body: string;
}

async function fetchFile(agentId: string, relPath: string): Promise<string | null> {
  try {
    return await workspaceApi.readFile(agentId, relPath);
  } catch (err) {
    console.warn('Failed to read file for mention:', relPath, err);
    return null;
  }
}

async function fetchItem(id: string): Promise<string | null> {
  try {
    const wi = await workItemsApi.get(id);
    const parts = [
      `Title: ${wi.title}`,
      `Status: ${wi.status}`,
      wi.kind ? `Kind: ${wi.kind}` : null,
      typeof wi.priority === 'number' ? `Priority: ${wi.priority}` : null,
      wi.labels?.length ? `Labels: ${wi.labels.join(', ')}` : null,
      wi.description ? `\n${wi.description}` : null,
    ].filter(Boolean);
    return parts.join('\n');
  } catch (err) {
    console.warn('Failed to fetch work item for mention:', id, err);
    return null;
  }
}

function applyBudget(attachments: Attachment[], budgetChars: number): Attachment[] {
  const totalChars = attachments.reduce((sum, a) => sum + a.body.length, 0);
  if (totalChars <= budgetChars || attachments.length === 0) return attachments;
  const truncated: string[] = [];
  const scale = budgetChars / totalChars;
  const result = attachments.map((a) => {
    const targetLen = Math.max(200, Math.floor(a.body.length * scale));
    if (a.body.length <= targetLen) return a;
    const omitted = a.body.length - targetLen;
    truncated.push(a.label);
    return {
      ...a,
      body: `${a.body.slice(0, targetLen)}\n…[truncated, ${omitted} chars omitted]`,
    };
  });
  if (truncated.length > 0) {
    console.warn(
      `[mentions] truncated attachments to fit 50% context window budget:`,
      truncated,
    );
  }
  return result;
}

function formatAttachment(a: Attachment): string {
  const heading = a.kind === 'file' ? `[File: ${a.label}]` : `[Work item: ${a.label}]`;
  return `---\n${heading}\n${a.body}`;
}

export async function resolveMentionsToContentBlocks(
  blocks: ContentBlock[],
  ctx: ResolveCtx,
): Promise<ContentBlock[]> {
  const attachments: Attachment[] = [];
  const seen = new Set<string>();

  for (const block of blocks) {
    if (block.type !== 'text') continue;
    const parsed = parseMentions(block.text);
    for (const { token } of parsed) {
      const dedupeKey = `${token.kind}:${token.payload}`;
      if (seen.has(dedupeKey)) continue;
      seen.add(dedupeKey);

      if (token.kind === 'file') {
        const firstColon = token.payload.indexOf(':');
        if (firstColon < 0) continue;
        const agentId = token.payload.slice(0, firstColon) || ctx.agentId;
        const relPath = token.payload.slice(firstColon + 1);
        if (!agentId) continue;
        const body = await fetchFile(agentId, relPath);
        if (body != null) {
          attachments.push({ kind: 'file', label: relPath, body });
        }
      } else if (token.kind === 'item') {
        const body = await fetchItem(token.payload);
        if (body != null) {
          attachments.push({ kind: 'item', label: token.label || token.payload, body });
        }
      }
    }
  }

  if (attachments.length === 0) return blocks;

  let contextWindowSize = FALLBACK_CONTEXT_WINDOW;
  if (ctx.sessionId) {
    try {
      const usage = await chatApi.getContextUsage(
        ctx.sessionId,
        ctx.modelOverride ?? undefined,
      );
      if (usage.contextWindowSize > 0) {
        contextWindowSize = usage.contextWindowSize;
      }
    } catch (err) {
      console.warn('[mentions] failed to read context window size; using fallback', err);
    }
  }
  const budgetChars = Math.floor(contextWindowSize * 0.5 * CHARS_PER_TOKEN);
  const trimmed = applyBudget(attachments, budgetChars);

  const attachedText = trimmed.map(formatAttachment).join('\n');

  return [...blocks, { type: 'text', text: attachedText }];
}
