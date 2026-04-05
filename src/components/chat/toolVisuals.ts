import {
  BookOpen,
  Brain,
  CheckCircle,
  FilePen,
  FileSearch,
  FolderSearch,
  GitBranch,
  Globe,
  Hammer,
  Search,
  Send,
  Terminal,
  Trash2,
  Zap,
  type LucideIcon,
} from 'lucide-react';

export interface ToolVisual {
  Icon: LucideIcon;
  colorClass: string;
}

const DEFAULT_TOOL_VISUAL: ToolVisual = {
  Icon: Hammer,
  colorClass: 'text-muted',
};

const TOOL_VISUALS: Record<string, ToolVisual> = {
  shell_command: { Icon: Terminal, colorClass: 'text-warning' },
  read_file: { Icon: FileSearch, colorClass: 'text-accent-hover' },
  write_file: { Icon: FilePen, colorClass: 'text-accent' },
  list_files: { Icon: FolderSearch, colorClass: 'text-accent-hover' },
  web_search: { Icon: Globe, colorClass: 'text-blue-400' },
  send_message: { Icon: Send, colorClass: 'text-blue-400' },
  activate_skill: { Icon: Zap, colorClass: 'text-warning' },
  spawn_sub_agents: { Icon: GitBranch, colorClass: 'text-emerald-400' },
  remember: { Icon: Brain, colorClass: 'text-accent-hover' },
  forget: { Icon: Trash2, colorClass: 'text-muted' },
  search_memory: { Icon: Search, colorClass: 'text-accent-hover' },
  list_memories: { Icon: BookOpen, colorClass: 'text-accent-hover' },
  finish: { Icon: CheckCircle, colorClass: 'text-success' },
};

export function getToolVisual(toolName: string): ToolVisual {
  return TOOL_VISUALS[toolName] ?? DEFAULT_TOOL_VISUAL;
}

export function formatToolName(toolName: string): string {
  return toolName
    .split('_')
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}
