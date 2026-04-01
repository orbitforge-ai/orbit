import { Terminal, Globe, FileCode, Bot, Cpu } from 'lucide-react';

export type TaskKind = 'shell_command' | 'script_file' | 'http_request' | 'agent_step' | 'agent_loop';
export type ScheduleKind = 'none' | 'recurring' | 'one_shot';

export const KIND_OPTIONS: {
  id: TaskKind;
  label: string;
  description: string;
  icon: React.ElementType;
}[] = [
  {
    id: 'shell_command',
    label: 'Shell Command',
    description: 'Run a bash/sh command',
    icon: Terminal,
  },
  {
    id: 'script_file',
    label: 'Script File',
    description: 'Execute a file on disk',
    icon: FileCode,
  },
  { id: 'http_request', label: 'HTTP Request', description: 'Call a URL or webhook', icon: Globe },
  { id: 'agent_step', label: 'Prompt', description: "Send a prompt to the agent's LLM", icon: Bot },
  { id: 'agent_loop', label: 'Agent Loop', description: 'Autonomous LLM-powered agent', icon: Cpu },
];

export const CONCURRENCY_OPTIONS: { value: string; label: string; hint: string }[] = [
  { value: 'allow', label: 'Allow', hint: 'Start a new run even if one is active' },
  { value: 'skip', label: 'Skip', hint: 'Drop the new run if agent is busy' },
  { value: 'queue', label: 'Queue', hint: 'Wait for a free slot before starting' },
  {
    value: 'cancel_previous',
    label: 'Cancel previous',
    hint: 'Stop the active run and start the new one',
  },
];

export const inputCls =
  'w-full px-4 py-2.5 rounded-lg bg-surface border border-edge text-white text-sm placeholder-border-hover focus:outline-none focus:border-accent';
