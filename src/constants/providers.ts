export const LLM_PROVIDERS = [
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'minimax', label: 'MiniMax' },
];

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
};

export const DEFAULT_MODEL_BY_PROVIDER: Record<string, string> = {
  anthropic: 'claude-sonnet-4-6',
  minimax: 'MiniMax-M2.7',
};

export const SEARCH_PROVIDERS = [
  { value: 'brave', label: 'Brave Search' },
  { value: 'tavily', label: 'Tavily' },
];

export const IMAGE_GENERATION_PROVIDERS = [
  { value: 'openai', label: 'OpenAI Images (gpt-image-1)' },
];
