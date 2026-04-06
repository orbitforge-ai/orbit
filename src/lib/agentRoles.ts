export interface AgentRole {
  roleId: string;
  label: string;
  description: string;
  icon: string;
  color: string;
  defaultTools: string[];
  systemInstructions: string;
}

export const AGENT_ROLES: AgentRole[] = [
  {
    roleId: 'general_assistant',
    label: 'General Assistant',
    description: 'All-purpose agent with full tool access.',
    icon: 'Bot',
    color: 'text-muted-foreground',
    defaultTools: [],
    systemInstructions: '',
  },
  {
    roleId: 'coder',
    label: 'Coder',
    description: 'Expert software engineer focused on writing, reviewing, and debugging code.',
    icon: 'Code2',
    color: 'text-blue-400',
    defaultTools: [
      'shell_command',
      'read_file',
      'write_file',
      'edit_file',
      'list_files',
      'grep',
      'activate_skill',
      'remember',
      'search_memory',
      'forget',
      'list_memories',
    ],
    systemInstructions:
      'You are an expert software engineer. Prioritize writing clean, correct, well-tested code. Read existing code carefully before editing. Always prefer minimal diffs. When debugging, reason about the root cause before proposing fixes. Explain your technical decisions concisely.',
  },
  {
    roleId: 'qa_tester',
    label: 'QA Tester',
    description: 'Quality assurance engineer focused on testing, bug-finding, and code reliability.',
    icon: 'Bug',
    color: 'text-rose-400',
    defaultTools: [
      'shell_command',
      'read_file',
      'write_file',
      'edit_file',
      'list_files',
      'grep',
      'activate_skill',
      'remember',
      'search_memory',
      'forget',
      'list_memories',
    ],
    systemInstructions:
      'You are a quality assurance engineer. Your primary goal is to find bugs, edge cases, and regressions. Write tests before confirming behavior. Think adversarially — what inputs could break this? Document failures clearly with reproduction steps.',
  },
  {
    roleId: 'researcher',
    label: 'Researcher',
    description: 'Research specialist that gathers and synthesizes information from multiple sources.',
    icon: 'Search',
    color: 'text-amber-400',
    defaultTools: [
      'web_search',
      'web_fetch',
      'read_file',
      'write_file',
      'edit_file',
      'list_files',
      'grep',
      'spawn_sub_agents',
      'remember',
      'search_memory',
      'forget',
      'list_memories',
    ],
    systemInstructions:
      'You are a research specialist. Gather information from multiple sources before drawing conclusions. Cite your sources. Distinguish clearly between established facts and your own inference. Summarize findings in a structured format. When uncertain, say so.',
  },
  {
    roleId: 'social_media_manager',
    label: 'Social Media',
    description: 'Social media strategist for content creation, scheduling, and platform engagement.',
    icon: 'Share2',
    color: 'text-pink-400',
    defaultTools: [
      'web_search',
      'web_fetch',
      'read_file',
      'write_file',
      'edit_file',
      'list_files',
      'grep',
      'remember',
      'search_memory',
      'forget',
      'list_memories',
    ],
    systemInstructions:
      "You are a social media strategist. Write content that is engaging, on-brand, and tailored to the target platform. Understand tone differences between platforms (LinkedIn = professional, X/Twitter = punchy, Instagram = visual storytelling). Propose hashtags, posting times, and engagement tactics when relevant.",
  },
  {
    roleId: 'data_analyst',
    label: 'Data Analyst',
    description: 'Data analyst focused on processing, interpreting, and visualizing data with rigor.',
    icon: 'BarChart2',
    color: 'text-emerald-400',
    defaultTools: [
      'shell_command',
      'read_file',
      'write_file',
      'edit_file',
      'list_files',
      'grep',
      'web_search',
      'web_fetch',
      'remember',
      'search_memory',
      'forget',
      'list_memories',
    ],
    systemInstructions:
      'You are a data analyst. Approach problems with statistical rigor. When processing data, validate assumptions about structure and types first. Produce clear summaries with key findings highlighted. Prefer reproducible analysis — write code that can be re-run. Visualize results when useful.',
  },
];

export const DEFAULT_ROLE_ID = 'general_assistant';

export function resolveRole(roleId: string | undefined): AgentRole {
  return AGENT_ROLES.find((r) => r.roleId === roleId) ?? AGENT_ROLES[0];
}

export function getRoleDefaultTools(roleId: string): string[] {
  return resolveRole(roleId).defaultTools;
}

export function getRoleSystemInstructions(roleId: string): string {
  return resolveRole(roleId).systemInstructions;
}
