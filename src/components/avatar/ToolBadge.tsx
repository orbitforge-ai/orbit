import {
  Terminal,
  FileSearch,
  FilePen,
  FolderSearch,
  Globe,
  Send,
  Zap,
  GitBranch,
  Brain,
  Trash2,
  Search,
  BookOpen,
  CheckCircle,
  Hammer,
} from 'lucide-react';

interface ToolBadgeProps {
  toolName: string;
}

interface ToolVisual {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  Icon: React.FC<any>;
  colorClass: string;
}

const TOOL_MAP: Record<string, ToolVisual> = {
  shell_command:    { Icon: Terminal,    colorClass: 'text-warning' },
  read_file:        { Icon: FileSearch,  colorClass: 'text-accent-hover' },
  write_file:       { Icon: FilePen,     colorClass: 'text-accent' },
  list_files:       { Icon: FolderSearch,colorClass: 'text-accent-hover' },
  web_search:       { Icon: Globe,       colorClass: 'text-blue-400' },
  send_message:     { Icon: Send,        colorClass: 'text-blue-400' },
  activate_skill:   { Icon: Zap,         colorClass: 'text-warning' },
  spawn_sub_agents: { Icon: GitBranch,   colorClass: 'text-emerald-400' },
  remember:         { Icon: Brain,       colorClass: 'text-accent-hover' },
  forget:           { Icon: Trash2,      colorClass: 'text-muted' },
  search_memory:    { Icon: Search,      colorClass: 'text-accent-hover' },
  list_memories:    { Icon: BookOpen,    colorClass: 'text-accent-hover' },
  finish:           { Icon: CheckCircle, colorClass: 'text-success' },
};

export function ToolBadge({ toolName }: ToolBadgeProps) {
  const visual = TOOL_MAP[toolName] ?? { Icon: Hammer, colorClass: 'text-muted' };
  const { Icon, colorClass } = visual;

  return (
    <div
      className="absolute -top-3 -right-3 w-7 h-7 rounded-full bg-surface border border-edge flex items-center justify-center shadow-lg"
      title={toolName}
    >
      <Icon
        size={14}
        className={`${colorClass} shrink-0`}
        style={{ animation: 'tool-badge-spin 1.8s linear infinite' }}
      />
    </div>
  );
}
