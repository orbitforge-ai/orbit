const LANGUAGE_MAP: Record<string, string> = {
  js: 'javascript',
  ts: 'typescript',
  jsx: 'javascript',
  tsx: 'typescript',
  json: 'json',
  md: 'markdown',
  py: 'python',
  rs: 'rust',
  toml: 'toml',
  yaml: 'yaml',
  yml: 'yaml',
  html: 'html',
  css: 'css',
  sh: 'shell',
  bash: 'shell',
  xml: 'xml',
  sql: 'sql',
  txt: 'plaintext',
};

export function getLanguageFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  return LANGUAGE_MAP[ext] ?? 'plaintext';
}
