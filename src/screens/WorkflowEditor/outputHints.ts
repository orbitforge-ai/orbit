import { nodeMeta } from './nodeRegistry';
import { getNodeReferenceKey } from './nodeReferences';

export interface OutputHintNode {
  data: Record<string, unknown>;
  id: string;
  type: string;
}

export interface OutputHintEntry {
  description?: string;
  path: string;
  preview?: string;
}

const MAX_EXAMPLE_DEPTH = 2;
const MAX_EXAMPLE_KEYS = 8;

export function getOutputRootPath(node: OutputHintNode): string {
  return node.type.startsWith('trigger.') ? 'trigger' : `${getNodeReferenceKey(node)}.output`;
}

export function getOutputReferenceLabel(node: OutputHintNode): string {
  const label = nodeMeta(node.type)?.label ?? node.type;
  return `${label} · ${getNodeReferenceKey(node)}`;
}

export function getStaticOutputHintEntries(node: OutputHintNode): OutputHintEntry[] {
  const root = getOutputRootPath(node);
  const action = asString(node.data.action);
  const outputMode = asString(node.data.outputMode);

  const entries = [
    hint(root, 'Entire output payload'),
    ...(node.type.startsWith('trigger.')
      ? [
          hint(`${root}.kind`, 'Trigger kind'),
          hint(`${root}.data`, 'Trigger payload'),
        ]
      : []),
    ...(node.type === 'agent.run'
      ? [
          hint(`${root}.agentId`, 'Resolved agent id'),
          hint(`${root}.prompt`, 'Rendered prompt'),
          hint(`${root}.context`, 'Rendered context'),
          hint(`${root}.outputMode`, 'Selected output mode'),
          hint(`${root}.text`, 'Raw model response text'),
          hint(
            `${root}.parsed`,
            outputMode === 'proposal_candidates'
              ? 'Array of candidates with listing, fitScore, fitReason, proposalDraft, shouldReview'
              : outputMode === 'json'
                ? 'Parsed JSON response'
                : 'Parsed JSON when valid, otherwise the response string',
          ),
        ]
      : []),
    ...(node.type === 'logic.if'
      ? [
          hint(`${root}.result`, 'Boolean branch result'),
          hint(`${root}.branch`, 'Selected handle'),
        ]
      : []),
    ...(node.type === 'code.bash.run'
      ? [
          hint(`${root}.cwd`, 'Resolved working directory'),
          hint(`${root}.stdout`, 'Captured standard output'),
          hint(`${root}.stderr`, 'Captured standard error'),
          hint(`${root}.exitCode`, 'Process exit code'),
          hint(`${root}.parsed`, 'Parsed JSON from stdout when valid'),
        ]
      : []),
    ...(node.type === 'code.script.run'
      ? [
          hint(`${root}.language`, 'Selected script language'),
          hint(`${root}.cwd`, 'Resolved working directory'),
          hint(`${root}.result`, 'Returned JSON-serializable value'),
        ]
      : []),
    ...(node.type === 'integration.feed.fetch'
      ? [
          hint(`${root}.sourceUrls`, 'Fetched feed URLs'),
          hint(`${root}.count`, 'Count of unseen items'),
          hint(`${root}.totalFetched`, 'Total fetched items before filtering'),
          hint(
            `${root}.items`,
            'Array of unseen items with source, title, url, summary, content, and timestamps',
          ),
        ]
      : []),
    ...(node.type === 'integration.com_orbit_discord.send_message'
      ? [
          hint(`${root}.pluginId`, 'Plugin id that handled the node'),
          hint(`${root}.tool`, 'Plugin tool invoked by the workflow node'),
          hint(`${root}.input.channelId`, 'Rendered Discord channel id'),
          hint(`${root}.input.threadId`, 'Rendered Discord thread id when provided'),
          hint(`${root}.input.text`, 'Rendered Discord message text'),
          hint(`${root}.result.messageId`, 'Discord message id returned by the bot API'),
        ]
      : []),
    ...(node.type === 'integration.http.request'
      ? [
          hint(`${root}.url`, 'Rendered request URL'),
          hint(`${root}.status`, 'HTTP status code'),
          hint(`${root}.contentType`, 'Response content type'),
          hint(`${root}.bodyText`, 'Normalized response body'),
          hint(`${root}.json`, 'Parsed JSON body when present'),
          hint(`${root}.fetchedAt`, 'Fetch timestamp'),
          hint(`${root}.isNew`, 'Whether content was new to this workflow'),
        ]
      : []),
    ...(node.type === 'board.proposal.enqueue'
      ? [
          hint(`${root}.reviewColumnId`, 'Destination review column'),
          hint(`${root}.count`, 'Number of queued work items'),
          hint(`${root}.workItems`, 'Created review items'),
        ]
      : []),
    ...(node.type === 'board.work_item.create'
      ? getWorkItemActionHints(root, action)
      : []),
  ];

  return dedupe(entries);
}

export function getObservedOutputHintEntries(
  node: OutputHintNode,
  output: unknown,
): OutputHintEntry[] {
  if (output === undefined) {
    return [];
  }
  const entries: OutputHintEntry[] = [];
  collectObservedHints(entries, getOutputRootPath(node), output, 0, true);
  return dedupe(entries);
}

function getWorkItemActionHints(root: string, action: string): OutputHintEntry[] {
  const common = [hint(`${root}.action`, 'Resolved board action')];

  switch (action) {
    case 'list':
      return common.concat([
        hint(`${root}.count`, 'Number of listed items'),
        hint(`${root}.items`, 'Listed work items'),
        hint(`${root}.filters`, 'Applied filters'),
      ]);
    case 'get':
      return common.concat([
        hint(`${root}.itemId`, 'Resolved work item id'),
        hint(`${root}.workItem`, 'Fetched work item'),
      ]);
    case 'delete':
      return common.concat([
        hint(`${root}.itemId`, 'Resolved work item id'),
        hint(`${root}.deleted`, 'Deletion result'),
      ]);
    case 'list_comments':
      return common.concat([
        hint(`${root}.itemId`, 'Resolved work item id'),
        hint(`${root}.count`, 'Number of comments'),
        hint(`${root}.comments`, 'Comment list'),
      ]);
    case 'comment':
      return common.concat([
        hint(`${root}.itemId`, 'Resolved work item id'),
        hint(`${root}.body`, 'Rendered comment body'),
        hint(`${root}.comment`, 'Created comment'),
      ]);
    case 'claim':
      return common.concat([
        hint(`${root}.itemId`, 'Resolved work item id'),
        hint(`${root}.agentId`, 'Resolved agent id'),
        hint(`${root}.workItem`, 'Updated work item'),
      ]);
    case 'block':
      return common.concat([
        hint(`${root}.itemId`, 'Resolved work item id'),
        hint(`${root}.reason`, 'Rendered blocked reason'),
        hint(`${root}.workItem`, 'Updated work item'),
      ]);
    case 'move':
      return common.concat([
        hint(`${root}.itemId`, 'Resolved work item id'),
        hint(`${root}.columnId`, 'Resolved destination column'),
        hint(`${root}.status`, 'Resolved destination status'),
        hint(`${root}.workItem`, 'Updated work item'),
      ]);
    case 'create':
      return common.concat([
        hint(`${root}.title`, 'Rendered title'),
        hint(`${root}.description`, 'Rendered description'),
        hint(`${root}.kind`, 'Resolved kind'),
        hint(`${root}.columnId`, 'Resolved column'),
        hint(`${root}.status`, 'Resolved status'),
        hint(`${root}.priority`, 'Resolved priority'),
        hint(`${root}.labels`, 'Rendered labels'),
        hint(`${root}.assigneeAgentId`, 'Resolved assignee'),
        hint(`${root}.parentWorkItemId`, 'Resolved parent id'),
        hint(`${root}.workItem`, 'Created work item'),
      ]);
    default:
      return common.concat([
        hint(`${root}.itemId`, 'Resolved work item id'),
        hint(`${root}.workItem`, 'Updated work item'),
      ]);
  }
}

function collectObservedHints(
  entries: OutputHintEntry[],
  path: string,
  value: unknown,
  depth: number,
  includeCurrent: boolean,
) {
  if (includeCurrent) {
    entries.push({
      path,
      preview: summarizeValue(value),
    });
  }

  if (!isRecord(value) || depth >= MAX_EXAMPLE_DEPTH) {
    return;
  }

  let count = 0;
  for (const [key, child] of Object.entries(value)) {
    if (count >= MAX_EXAMPLE_KEYS) {
      break;
    }
    const childPath = `${path}.${key}`;
    entries.push({
      path: childPath,
      preview: summarizeValue(child),
    });
    if (isRecord(child)) {
      collectObservedHints(entries, childPath, child, depth + 1, false);
    }
    count += 1;
  }
}

function dedupe(entries: OutputHintEntry[]): OutputHintEntry[] {
  const seen = new Set<string>();
  return entries.filter((entry) => {
    if (seen.has(entry.path)) {
      return false;
    }
    seen.add(entry.path);
    return true;
  });
}

function hint(path: string, description?: string): OutputHintEntry {
  return { description, path };
}

function asString(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function summarizeValue(value: unknown): string {
  if (typeof value === 'string') {
    return value.length > 60 ? `${JSON.stringify(value.slice(0, 57))}...` : JSON.stringify(value);
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  if (value === null) {
    return 'null';
  }
  if (Array.isArray(value)) {
    return value.length === 0 ? '[]' : `${value.length} item${value.length === 1 ? '' : 's'}`;
  }
  if (isRecord(value)) {
    const keys = Object.keys(value);
    if (keys.length === 0) {
      return '{}';
    }
    const preview = keys.slice(0, 3).join(', ');
    return keys.length > 3 ? `{ ${preview}, ... }` : `{ ${preview} }`;
  }
  if (value === undefined) {
    return 'undefined';
  }
  return String(value);
}
