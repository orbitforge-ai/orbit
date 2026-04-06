import {
  BarChart3,
  BookOpen,
  Brain,
  CheckCircle,
  CornerDownRight,
  FilePen,
  FileSearch,
  FolderSearch,
  GitBranch,
  GitFork,
  Globe,
  Hammer,
  HelpCircle,
  History,
  Link,
  List,
  PauseCircle,
  PlusCircle,
  Pencil,
  Search,
  Send,
  Settings,
  Terminal,
  Trash2,
  Zap,
  GitBranchPlus,
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
  edit_file: { Icon: Pencil, colorClass: 'text-accent' },
  list_files: { Icon: FolderSearch, colorClass: 'text-accent-hover' },
  grep: { Icon: Search, colorClass: 'text-accent-hover' },
  web_search: { Icon: Globe, colorClass: 'text-blue-400' },
  web_fetch: { Icon: Link, colorClass: 'text-blue-400' },
  config: { Icon: Settings, colorClass: 'text-muted' },
  worktree: { Icon: GitBranchPlus, colorClass: 'text-emerald-400' },
  session_history: { Icon: History, colorClass: 'text-muted' },
  session_status: { Icon: BarChart3, colorClass: 'text-muted' },
  sessions_list: { Icon: List, colorClass: 'text-muted' },
  session_send: { Icon: CornerDownRight, colorClass: 'text-blue-400' },
  sessions_spawn: { Icon: PlusCircle, colorClass: 'text-emerald-400' },
  send_message: { Icon: Send, colorClass: 'text-blue-400' },
  ask_user: { Icon: HelpCircle, colorClass: 'text-blue-400' },
  activate_skill: { Icon: Zap, colorClass: 'text-warning' },
  spawn_sub_agents: { Icon: GitBranch, colorClass: 'text-emerald-400' },
  subagents: { Icon: GitFork, colorClass: 'text-emerald-400' },
  yield_turn: { Icon: PauseCircle, colorClass: 'text-warning' },
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
