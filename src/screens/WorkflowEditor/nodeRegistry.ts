import {
  Bot,
  GitBranch,
  Globe,
  KanbanSquare,
  LucideIcon,
  Mail,
  MailOpen,
  MessagesSquare,
  Play,
  Rss,
  Timer,
} from 'lucide-react';
import type { WorkflowNodeType } from '../../types';
import { DEFAULT_WORKFLOW_SCHEDULE } from './scheduleConfig';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function asVariantString(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function normalizeVariantSegment(value: string): string {
  return value.trim().toLowerCase().replace(/\s+/g, '_');
}

export interface WorkflowNodeVariantConfig {
  baseType?: string;
  fields: string[];
}

export interface NodeMeta {
  type: WorkflowNodeType;
  label: string;
  group: 'Triggers' | 'Agents' | 'Logic' | 'Board' | 'Integrations';
  icon: LucideIcon;
  defaultData: Record<string, unknown>;
  historyVariant?: WorkflowNodeVariantConfig;
  comingSoon?: boolean;
}

export const NODE_REGISTRY: NodeMeta[] = [
  {
    type: 'trigger.manual',
    label: 'Run now',
    group: 'Triggers',
    icon: Play,
    defaultData: {},
  },
  {
    type: 'trigger.schedule',
    label: 'Schedule',
    group: 'Triggers',
    icon: Timer,
    defaultData: DEFAULT_WORKFLOW_SCHEDULE as unknown as Record<string, unknown>,
  },
  {
    type: 'agent.run',
    label: 'Run agent',
    group: 'Agents',
    icon: Bot,
    defaultData: {
      agentId: '',
      promptTemplate: '',
      contextTemplate: '',
      outputMode: 'text',
    },
    historyVariant: {
      fields: ['outputMode'],
    },
  },
  {
    type: 'logic.if',
    label: 'If / branch',
    group: 'Logic',
    icon: GitBranch,
    defaultData: {
      rule: { combinator: 'and', rules: [] },
      trueLabel: 'true',
      falseLabel: 'false',
    },
  },
  {
    type: 'board.work_item.create',
    label: 'Board · Work item',
    group: 'Board',
    icon: KanbanSquare,
    defaultData: {
      action: 'create',
      itemIdTemplate: '',
      titleTemplate: '',
      descriptionTemplate: '',
      columnId: '',
      kind: '',
      status: '',
      priority: null,
      labelsText: '',
      assigneeAgentId: '',
      parentWorkItemId: '',
      reasonTemplate: '',
      bodyTemplate: '',
      commentAuthorAgentId: '',
      listColumn: 'all',
      listStatus: 'all',
      listKind: 'all',
      listAssignee: '',
      limit: 25,
    },
    historyVariant: {
      baseType: 'board.work_item',
      fields: ['action'],
    },
  },
  {
    type: 'board.proposal.enqueue',
    label: 'Board · Proposal queue',
    group: 'Board',
    icon: KanbanSquare,
    defaultData: {
      candidatesPath: '',
      reviewColumnId: '',
      kind: 'task',
      priority: 1,
      labelsText: 'proposal-review',
    },
  },
  {
    type: 'integration.feed.fetch',
    label: 'Feed fetch',
    group: 'Integrations',
    icon: Rss,
    defaultData: {
      feedUrlsText: '',
      limit: 50,
    },
  },
  {
    type: 'integration.gmail.read',
    label: 'Gmail · Read',
    group: 'Integrations',
    icon: MailOpen,
    defaultData: {},
    comingSoon: true,
  },
  {
    type: 'integration.gmail.send',
    label: 'Gmail · Send',
    group: 'Integrations',
    icon: Mail,
    defaultData: {},
    comingSoon: true,
  },
  {
    type: 'integration.slack.send',
    label: 'Slack · Send',
    group: 'Integrations',
    icon: MessagesSquare,
    defaultData: {},
    comingSoon: true,
  },
  {
    type: 'integration.http.request',
    label: 'HTTP request',
    group: 'Integrations',
    icon: Globe,
    defaultData: { method: 'GET', url: '' },
    historyVariant: {
      fields: ['method'],
    },
  },
];

export const NODE_META_BY_TYPE: Record<string, NodeMeta> = NODE_REGISTRY.reduce(
  (acc, n) => {
    acc[n.type] = n;
    return acc;
  },
  {} as Record<string, NodeMeta>,
);

export function nodeMeta(type: string): NodeMeta | null {
  return NODE_META_BY_TYPE[type] ?? null;
}

export function resolveWorkflowNodeHistoryLabel(params: {
  nodeId: string;
  nodeType: string;
  graphNodes?: Array<{ id: string; type: string; data: Record<string, unknown> }>;
  input?: unknown;
  output?: unknown;
}): string {
  const { nodeId, nodeType, graphNodes = [], input, output } = params;
  const meta = nodeMeta(nodeType);
  const variant = meta?.historyVariant;
  if (!variant) {
    return nodeType;
  }

  const snapshotNode = graphNodes.find((node) => node.id === nodeId);
  const candidateSources = [
    isRecord(input) ? input : null,
    isRecord(output) ? output : null,
    snapshotNode?.type === nodeType ? snapshotNode.data : null,
  ];

  for (const field of variant.fields) {
    for (const source of candidateSources) {
      const raw = source ? asVariantString(source[field]) : null;
      if (!raw) continue;
      return `${variant.baseType ?? nodeType}.${normalizeVariantSegment(raw)}`;
    }
  }

  return nodeType;
}
