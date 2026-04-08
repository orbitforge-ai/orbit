// Shared tool catalog used by the global settings screen (for the shared
// allowed-tools editor) and the per-agent config tab (for the disabled-tools
// multi-select). Must stay in sync with `DEFAULT_ALLOWED_TOOLS` in
// `src-tauri/src/executor/global_settings.rs`.

export interface ToolDescriptor {
  id: string;
  label: string;
}

export interface ToolCategory {
  label: string;
  tools: ToolDescriptor[];
}

export const TOOL_CATEGORIES: ToolCategory[] = [
  {
    label: 'File System',
    tools: [
      { id: 'read_file', label: 'Read Files' },
      { id: 'write_file', label: 'Write Files' },
      { id: 'edit_file', label: 'Edit Files' },
      { id: 'list_files', label: 'List Files' },
      { id: 'grep', label: 'Content Search' },
    ],
  },
  {
    label: 'Execution',
    tools: [
      { id: 'shell_command', label: 'Shell Commands' },
      { id: 'worktree', label: 'Git Worktree' },
    ],
  },
  {
    label: 'Communication',
    tools: [
      { id: 'message', label: 'External Messages' },
      { id: 'ask_user', label: 'Ask User' },
      { id: 'web_search', label: 'Web Search' },
      { id: 'web_fetch', label: 'Web Fetch' },
    ],
  },
  {
    label: 'Vision',
    tools: [
      { id: 'image_analysis', label: 'Image Analysis' },
      { id: 'image_generation', label: 'Image Generation' },
    ],
  },
  {
    label: 'Sessions',
    tools: [
      { id: 'session_history', label: 'Session History' },
      { id: 'session_status', label: 'Session Status' },
      { id: 'sessions_list', label: 'List Sessions' },
      { id: 'session_send', label: 'Session Send' },
      { id: 'sessions_spawn', label: 'Spawn Session' },
    ],
  },
  {
    label: 'Agent Control',
    tools: [
      { id: 'config', label: 'Self-Config' },
      { id: 'subagents', label: 'Manage Sub-Agents' },
      { id: 'yield_turn', label: 'Yield Turn' },
      { id: 'activate_skill', label: 'Activate Skill' },
    ],
  },
  {
    label: 'Task Management',
    tools: [{ id: 'task', label: 'Agent Task Tracking' }],
  },
  {
    label: 'Scheduling',
    tools: [{ id: 'schedule', label: 'Schedules & Pulse' }],
  },
  {
    label: 'Memory',
    tools: [
      { id: 'remember', label: 'Remember' },
      { id: 'search_memory', label: 'Search Memory' },
    ],
  },
];

export const ALL_TOOL_IDS: string[] = TOOL_CATEGORIES.flatMap((c) =>
  c.tools.map((t) => t.id),
);

export const TOOL_LABEL_BY_ID: Record<string, string> = TOOL_CATEGORIES.reduce(
  (acc, cat) => {
    for (const tool of cat.tools) acc[tool.id] = tool.label;
    return acc;
  },
  {} as Record<string, string>,
);
