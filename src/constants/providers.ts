export const LLM_PROVIDERS = [
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'minimax', label: 'MiniMax' },
  { value: 'claude-cli', label: 'Claude CLI (local)' },
  { value: 'codex-cli', label: 'Codex CLI (local, experimental)' },
];

export const CLI_PROVIDERS = new Set(['claude-cli', 'codex-cli']);

export const isCliProvider = (provider: string) => CLI_PROVIDERS.has(provider);

export const MODEL_OPTIONS: Record<string, { label: string; value: string }[]> = {
  anthropic: [
    { label: 'Claude Opus 4.7', value: 'claude-opus-4-7' },
    { label: 'Claude Opus 4.6', value: 'claude-opus-4-6' },
    { label: 'Claude Sonnet 4.6', value: 'claude-sonnet-4-6' },
    { label: 'Claude Haiku 4.5', value: 'claude-haiku-4-5-20251001' },
  ],
  minimax: [
    { label: 'MiniMax M2.7', value: 'MiniMax-M2.7' },
    { label: 'MiniMax M2.7 Highspeed', value: 'MiniMax-M2.7-highspeed' },
    { label: 'MiniMax M2.5', value: 'MiniMax-M2.5' },
    { label: 'MiniMax M2.5 Highspeed', value: 'MiniMax-M2.5-highspeed' },
  ],
  'claude-cli': [
    { label: 'Claude Sonnet 4.6 (via CLI)', value: 'claude-sonnet-4-6' },
    { label: 'Claude Opus 4.7 (via CLI)', value: 'claude-opus-4-7' },
    { label: 'Claude Haiku 4.5 (via CLI)', value: 'claude-haiku-4-5-20251001' },
  ],
  'codex-cli': [{ label: 'Codex (via CLI)', value: 'gpt-5-codex' }],
};

export const DEFAULT_MODEL_BY_PROVIDER: Record<string, string> = {
  anthropic: 'claude-sonnet-4-6',
  minimax: 'MiniMax-M2.7',
  'claude-cli': 'claude-sonnet-4-6',
  'codex-cli': 'gpt-5-codex',
};

export const SEARCH_PROVIDERS = [
  { value: 'brave', label: 'Brave Search' },
  { value: 'tavily', label: 'Tavily' },
];

export const IMAGE_GENERATION_PROVIDERS = [
  { value: 'openai', label: 'OpenAI Images (gpt-image-1)' },
];
