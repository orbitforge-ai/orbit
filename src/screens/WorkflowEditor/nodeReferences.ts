import { Node } from '@xyflow/react';
import type { WorkflowEdge, WorkflowNode } from '../../types';
import { nodeMeta } from './nodeRegistry';

export const RESERVED_REFERENCE_KEYS = new Set(['trigger', '__aliases']);

export function slugifyReferenceKey(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .replace(/--+/g, '-');
}

export function getNodeReferenceKey(
  node: Pick<WorkflowNode, 'id' | 'type' | 'data'> | Pick<Node, 'id' | 'type' | 'data'>,
): string {
  const stored = normalizeStoredReferenceKey(node.data);
  return stored || node.id;
}

export function getNodeReferenceLabel(
  node: Pick<WorkflowNode, 'id' | 'type' | 'data'> | Pick<Node, 'id' | 'type' | 'data'>,
): string {
  return node.type?.startsWith('trigger.') ? 'trigger' : getNodeReferenceKey(node);
}

export function ensureFlowNodeReferenceKeys(nodes: Node[]): Node[] {
  return assignNodeReferenceKeys(nodes, []);
}

export function ensureFlowNodeReferenceKeysForGraph(nodes: Node[], edges: { source: string }[]): Node[] {
  return assignNodeReferenceKeys(nodes, edges);
}

export function ensureWorkflowNodeReferenceKeys(
  nodes: WorkflowNode[],
  edges: WorkflowEdge[],
): WorkflowNode[] {
  return assignNodeReferenceKeys(nodes, edges);
}

export function generateUniqueReferenceKey(
  type: string,
  used: Set<string>,
  preferred?: string | null,
): string {
  const base = getAutoReferenceBase(type, preferred);
  let suffix = 1;
  let key = `${base}-${suffix}`;
  while (used.has(key)) {
    suffix += 1;
    key = `${base}-${suffix}`;
  }
  used.add(key);
  return key;
}

export function generateReferenceKeyForNewNode<
  T extends Pick<WorkflowNode, 'type' | 'data'> | Pick<Node, 'type' | 'data'>,
>(type: string, nodes: T[]): string {
  const base = getAutoReferenceBase(type);
  let maxSuffix = 0;
  let sameTypeCount = 0;

  for (const node of nodes) {
    if ((node.type ?? 'node') !== type) {
      continue;
    }
    sameTypeCount += 1;
    const existing = normalizeStoredReferenceKey(node.data);
    if (!existing) {
      continue;
    }
    const match = existing.match(new RegExp(`^${escapeRegExp(base)}-(\\d+)$`));
    if (!match) {
      continue;
    }
    const suffix = Number(match[1]);
    if (Number.isFinite(suffix) && suffix > maxSuffix) {
      maxSuffix = suffix;
    }
  }

  return `${base}-${Math.max(maxSuffix + 1, sameTypeCount + 1)}`;
}

function assignNodeReferenceKeys<T extends { data: unknown; id: string; type?: string }>(
  nodes: T[],
  edges: { source: string }[],
): T[] {
  const used = new Set<string>(RESERVED_REFERENCE_KEYS);
  const nodesWithDownstreamLinks = new Set(edges.map((edge) => edge.source));

  for (const node of nodes) {
    if (!nodeHasLinkedOutputs(node, nodesWithDownstreamLinks)) {
      continue;
    }
    const data = normalizeData(node.data);
    const existing = normalizeStoredReferenceKey(data);
    if (existing && !isGeneratedReferenceKey(node.type ?? 'node', existing)) {
      used.add(existing);
    }
  }

  return nodes.map((node) => {
    const data = normalizeData(node.data);
    const existing = normalizeStoredReferenceKey(data);
    if (!nodeHasLinkedOutputs(node, nodesWithDownstreamLinks)) {
      return data === node.data ? node : ({ ...node, data } as T);
    }
    if (existing && !isGeneratedReferenceKey(node.type ?? 'node', existing)) {
      return data === node.data ? node : ({ ...node, data } as T);
    }

    const referenceKey = generateUniqueReferenceKey(node.type ?? 'node', used);
    return {
      ...node,
      data: {
        ...data,
        referenceKey,
      },
    } as T;
  });
}

export function nodeHasLinkedOutputs<T extends { id: string; type?: string }>(
  node: T,
  nodesWithDownstreamLinks: Set<string>,
): boolean {
  return Boolean(node.type?.startsWith('trigger.')) || nodesWithDownstreamLinks.has(node.id);
}

function normalizeStoredReferenceKey(data: unknown): string | null {
  const record = normalizeData(data);
  const value = record.referenceKey;
  if (typeof value !== 'string') {
    return null;
  }
  return normalizeCandidateReferenceKey(value);
}

function normalizeCandidateReferenceKey(value: string | null | undefined): string | null {
  const normalized = slugifyReferenceKey(value ?? '');
  if (!normalized || RESERVED_REFERENCE_KEYS.has(normalized)) {
    return null;
  }
  return normalized;
}

function getAutoReferenceBase(type: string, preferred?: string | null): string {
  return (
    normalizeCandidateReferenceKey(preferred) ||
    slugifyReferenceKey(nodeMeta(type)?.label ?? type.replace(/\./g, ' ')) ||
    'node'
  );
}

function isGeneratedReferenceKey(type: string, value: string): boolean {
  const base = getAutoReferenceBase(type);
  return value === base || new RegExp(`^${escapeRegExp(base)}-\\d+$`).test(value);
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function normalizeData(data: unknown): Record<string, unknown> {
  return data && typeof data === 'object' && !Array.isArray(data)
    ? (data as Record<string, unknown>)
    : {};
}
