import {
  Bot,
  GitBranch,
  Globe,
  LucideIcon,
  Mail,
  MailOpen,
  MessagesSquare,
  Play,
  Timer,
} from 'lucide-react';
import type { WorkflowNodeType } from '../../types';

export interface NodeMeta {
  type: WorkflowNodeType;
  label: string;
  group: 'Triggers' | 'Agents' | 'Logic' | 'Integrations';
  icon: LucideIcon;
  defaultData: Record<string, unknown>;
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
    defaultData: { cron: '0 * * * *' },
  },
  {
    type: 'agent.run',
    label: 'Run agent',
    group: 'Agents',
    icon: Bot,
    defaultData: { agentId: '', promptTemplate: '' },
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
    comingSoon: true,
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
