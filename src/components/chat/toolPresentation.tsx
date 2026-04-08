import { TextBlock } from './TextBlock';
import { DisplayBlock } from './types';

type ToolCallBlock = Extract<DisplayBlock, { kind: 'tool_call' }>;

type SectionTone = 'default' | 'success' | 'error' | 'warning' | 'muted';

type PresentationSection =
  | {
      type: 'keyValue';
      title: string;
      entries: Array<{ label: string; value: string }>;
      tone?: SectionTone;
    }
  | {
      type: 'code';
      title: string;
      content: string;
      tone?: SectionTone;
    }
  | {
      type: 'markdown';
      title: string;
      content: string;
      tone?: SectionTone;
    }
  | {
      type: 'cards';
      title: string;
      cards: Array<{
        title: string;
        subtitle?: string;
        body?: string;
        badge?: string;
      }>;
      tone?: SectionTone;
    };

interface ToolPresentation {
  requestSections: PresentationSection[];
  resultSections: PresentationSection[];
}

type ToolFormatter = (tool: ToolCallBlock, normalizedResult: string | null) => ToolPresentation;

const CHIP_ONLY_TOOLS = new Set(['remember', 'forget', 'finish', 'activate_skill']);

const TOOL_FORMATTERS: Record<string, ToolFormatter> = {
  shell_command: formatShellCommand,
  read_file: formatReadFile,
  write_file: formatWriteFile,
  edit_file: formatEditFile,
  list_files: formatListFiles,
  web_search: formatWebSearch,
  web_fetch: formatWebFetch,
  search_memory: formatMemoryLookup,
  list_memories: formatMemoryLookup,
};

export function canExpandToolDetails(tool: ToolCallBlock): boolean {
  if (tool.result?.isError) return true;
  if (tool.name === 'spawn_sub_agents') return true;
  if (tool.name === 'send_message' && tool.input.wait_for_result === true && !tool.result) return true;
  if ((tool.name === 'yield_turn' || tool.name === 'ask_user') && !tool.result) return true;
  return !CHIP_ONLY_TOOLS.has(tool.name);
}

export function buildToolPresentation(tool: ToolCallBlock): ToolPresentation {
  const normalizedResult = tool.result ? normalizeToolResult(tool.result.content) : null;
  const formatter = TOOL_FORMATTERS[tool.name] ?? formatGenericTool;
  return formatter(tool, normalizedResult);
}

export function normalizeToolResult(content: string): string {
  const trimmed = content.trim();
  const wrapped = trimmed.match(/^<tool_result\b[^>]*>([\s\S]*)<\/tool_result>$/i);
  return wrapped ? wrapped[1].trim() : content;
}

export function ToolPresentationSections({
  sections,
}: {
  sections: PresentationSection[];
}) {
  return (
    <div className="space-y-3 px-3 py-3">
      {sections.map((section, index) => (
        <section key={`${section.title}-${index}`} className="space-y-1.5">
          <div className="text-[10px] uppercase tracking-wider text-muted">{section.title}</div>
          {renderSection(section)}
        </section>
      ))}
    </div>
  );
}

function renderSection(section: PresentationSection) {
  switch (section.type) {
    case 'keyValue':
      return (
        <div className={`rounded-lg border ${toneClasses(section.tone)}`}>
          {section.entries.map((entry) => (
            <div
              key={entry.label}
              className="flex items-start justify-between gap-4 border-b border-white/5 px-3 py-2 last:border-b-0"
            >
              <div className="text-xs text-muted">{entry.label}</div>
              <div className="text-right text-xs text-secondary break-all whitespace-pre-wrap">
                {entry.value}
              </div>
            </div>
          ))}
        </div>
      );
    case 'code':
      return (
        <pre
          className={`rounded-lg border px-3 py-2 text-xs font-mono whitespace-pre-wrap break-all overflow-x-auto ${toneClasses(
            section.tone
          )}`}
        >
          {section.content}
        </pre>
      );
    case 'markdown':
      return (
        <div className={`rounded-lg border px-3 py-2 ${toneClasses(section.tone)}`}>
          <TextBlock text={section.content} isStreaming={false} />
        </div>
      );
    case 'cards':
      return (
        <div className="space-y-2">
          {section.cards.map((card, index) => (
            <div
              key={`${card.title}-${index}`}
              className={`rounded-lg border px-3 py-2 ${toneClasses(section.tone)}`}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-sm font-medium text-white break-words">{card.title}</div>
                  {card.subtitle && (
                    <div className="mt-0.5 text-xs text-muted break-words">{card.subtitle}</div>
                  )}
                </div>
                {card.badge && (
                  <span className="shrink-0 rounded-full border border-white/10 bg-background/70 px-2 py-0.5 text-[10px] uppercase tracking-wide text-secondary">
                    {card.badge}
                  </span>
                )}
              </div>
              {card.body && (
                <div className="mt-2 text-xs text-secondary whitespace-pre-wrap break-words">
                  {card.body}
                </div>
              )}
            </div>
          ))}
        </div>
      );
  }
}

function toneClasses(tone: SectionTone = 'default'): string {
  switch (tone) {
    case 'success':
      return 'border-emerald-500/20 bg-emerald-500/5 text-secondary';
    case 'error':
      return 'border-red-500/20 bg-red-500/5 text-red-100';
    case 'warning':
      return 'border-amber-500/20 bg-amber-500/5 text-amber-100';
    case 'muted':
      return 'border-edge bg-background/50 text-secondary';
    default:
      return 'border-edge bg-background/60 text-secondary';
  }
}

function formatGenericTool(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  return {
    requestSections: summarizeInput(tool),
    resultSections: summarizeResult(normalizedResult, tool.result?.isError ?? false),
  };
}

function formatShellCommand(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  const requestEntries = [
    maybeEntry('Command', stringValue(tool.input.command)),
    maybeEntry('Timeout', numberValue(tool.input.timeout_seconds, (value) => `${value}s`)),
    maybeEntry('Background', booleanValue(tool.input.run_in_background)),
    maybeEntry('Action', stringValue(tool.input.process_action)),
    maybeEntry('Process ID', stringValue(tool.input.process_id)),
  ].filter(isDefined);

  const parsed = normalizedResult ? parseJson(normalizedResult) : null;
  if (!parsed) {
    return {
      requestSections: requestEntries.length
        ? [{ type: 'keyValue', title: 'Request', entries: requestEntries }]
        : summarizeInput(tool),
      resultSections: summarizeResult(normalizedResult, tool.result?.isError ?? false),
    };
  }

  const objects = Array.isArray(parsed) ? parsed.filter(isRecord) : isRecord(parsed) ? [parsed] : [];
  const primitiveResult = !Array.isArray(parsed) && isRecord(parsed) ? parsed : null;
  const resultSections: PresentationSection[] = [];

  if (Array.isArray(parsed) && objects.length > 0) {
    resultSections.push({
      type: 'cards',
      title: 'Processes',
      cards: objects.map((item) => ({
        title: firstString(item, ['command', 'processId', 'id']) ?? 'Background process',
        subtitle:
          compactParts(
            [firstString(item, ['processId', 'id']), firstString(item, ['status', 'state'])].filter(
              isDefined
            )
          ) || undefined,
        badge: firstString(item, ['status', 'state']) ?? undefined,
        body:
          compactParts([firstString(item, ['startedAt']), firstString(item, ['logPath'])].filter(isDefined)) ||
          undefined,
      })),
    });
  } else if (primitiveResult) {
    const statusEntries = pickEntries(primitiveResult, [
      ['Process ID', 'processId'],
      ['Status', 'status'],
      ['Exit Code', 'exitCode'],
      ['PID', 'pid'],
      ['Log Path', 'logPath'],
      ['Started', 'startedAt'],
      ['Finished', 'finishedAt'],
    ]);
    if (statusEntries.length > 0) {
      resultSections.push({ type: 'keyValue', title: 'Status', entries: statusEntries });
    }
    const stdoutTail = firstString(primitiveResult, ['stdoutTail', 'stdout']);
    const stderrTail = firstString(primitiveResult, ['stderrTail', 'stderr']);
    if (stdoutTail) {
      resultSections.push({ type: 'code', title: 'Stdout', content: stdoutTail });
    }
    if (stderrTail) {
      resultSections.push({ type: 'code', title: 'Stderr', content: stderrTail, tone: 'warning' });
    }
  }

  if (resultSections.length === 0) {
    resultSections.push(...summarizeResult(normalizedResult, tool.result?.isError ?? false));
  }

  return {
    requestSections: requestEntries.length
      ? [{ type: 'keyValue', title: 'Request', entries: requestEntries }]
      : summarizeInput(tool),
    resultSections,
  };
}

function formatReadFile(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  const requestSections: PresentationSection[] = [
    {
      type: 'keyValue',
      title: 'File',
      entries: [{ label: 'Path', value: stringValue(tool.input.path) ?? '(unknown)' }],
    },
  ];
  const path = stringValue(tool.input.path) ?? '';
  const resultSections: PresentationSection[] = normalizedResult
    ? [
        {
          type: path.endsWith('.ipynb') ? 'markdown' : 'code',
          title: path.endsWith('.ipynb') ? 'Notebook' : 'Contents',
          content: normalizedResult,
        },
      ]
    : [];

  return { requestSections, resultSections };
}

function formatWriteFile(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  const requestSections: PresentationSection[] = [
    {
      type: 'keyValue',
      title: 'Write',
      entries: [
        { label: 'Path', value: stringValue(tool.input.path) ?? '(unknown)' },
        {
          label: 'Content',
          value: summarizeContentValue(tool.input.content),
        },
      ],
    },
  ];

  return {
    requestSections,
    resultSections: summarizeResult(normalizedResult, tool.result?.isError ?? false),
  };
}

function formatEditFile(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  const requestEntries = [
    maybeEntry('Path', stringValue(tool.input.path)),
    maybeEntry('Notebook action', stringValue(tool.input.notebook_action)),
    maybeEntry('Cell', numberValue(tool.input.cell_number)),
    maybeEntry('Cell type', stringValue(tool.input.cell_type)),
    maybeEntry('Replace all', booleanValue(tool.input.replace_all)),
  ].filter(isDefined);

  if (!tool.input.notebook_action) {
    requestEntries.push({
      label: 'Old text',
      value: summarizeTextPreview(stringValue(tool.input.old_text)),
    });
    requestEntries.push({
      label: 'New text',
      value: summarizeTextPreview(stringValue(tool.input.new_text)),
    });
  } else if (tool.input.cell_source) {
    requestEntries.push({
      label: 'Cell source',
      value: summarizeTextPreview(stringValue(tool.input.cell_source)),
    });
  }

  return {
    requestSections: [{ type: 'keyValue', title: 'Edit', entries: requestEntries }],
    resultSections: summarizeResult(normalizedResult, tool.result?.isError ?? false),
  };
}

function formatListFiles(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  const requestSections: PresentationSection[] = [
    {
      type: 'keyValue',
      title: 'Request',
      entries: [
        { label: 'Path', value: stringValue(tool.input.path) ?? '(unknown)' },
        ...(tool.input.pattern
          ? [{ label: 'Pattern', value: stringValue(tool.input.pattern) ?? '' }]
          : []),
      ],
    },
  ];

  const lines = splitLines(normalizedResult);
  if (!normalizedResult || lines.length === 0) {
    return { requestSections, resultSections: [] };
  }

  if (lines.every((line) => /^(\s*\d+)\s+(dir|file)\s+/.test(line))) {
    return {
      requestSections,
      resultSections: [
        {
          type: 'cards',
          title: 'Entries',
          cards: lines.map((line) => {
            const match = line.match(/^(\s*\d+)\s+(dir|file)\s+(.*)$/);
            return {
              title: match?.[3]?.trim() ?? line,
              subtitle: match ? `${match[2]} • ${match[1].trim()} bytes` : undefined,
              badge: match?.[2],
            };
          }),
        },
      ],
    };
  }

  return {
    requestSections,
    resultSections: [
      {
        type: 'cards',
        title: 'Matches',
        cards: lines.map((line) => ({
          title: line,
        })),
      },
    ],
  };
}

function formatWebSearch(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  const requestSections: PresentationSection[] = [
    {
      type: 'keyValue',
      title: 'Search',
      entries: [
        { label: 'Query', value: stringValue(tool.input.query) ?? '(unknown)' },
        ...(tool.input.count
          ? [{ label: 'Count', value: numberValue(tool.input.count) ?? '' }]
          : []),
      ],
    },
  ];

  const cards = parseSearchResults(normalizedResult);
  return {
    requestSections,
    resultSections:
      cards.length > 0
        ? [{ type: 'cards', title: 'Results', cards }]
        : summarizeResult(normalizedResult, tool.result?.isError ?? false),
  };
}

function formatWebFetch(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  return {
    requestSections: [
      {
        type: 'keyValue',
        title: 'Fetch',
        entries: [
          { label: 'URL', value: stringValue(tool.input.url) ?? '(unknown)' },
          maybeEntry('Raw body', booleanValue(tool.input.raw)),
          maybeEntry('Max length', numberValue(tool.input.max_length)),
        ].filter(isDefined),
      },
    ],
    resultSections: normalizedResult
      ? [
          {
            type: tool.input.raw === true ? 'code' : 'markdown',
            title: 'Content',
            content: normalizedResult,
          },
        ]
      : [],
  };
}

function formatMemoryLookup(tool: ToolCallBlock, normalizedResult: string | null): ToolPresentation {
  const requestEntries =
    tool.name === 'search_memory'
      ? [
          { label: 'Query', value: stringValue(tool.input.query) ?? '(unknown)' },
          maybeEntry('Type', stringValue(tool.input.memory_type)),
          maybeEntry('Limit', numberValue(tool.input.limit)),
        ].filter(isDefined)
      : [
          maybeEntry('Type', stringValue(tool.input.memory_type)),
          maybeEntry('Limit', numberValue(tool.input.limit)),
        ].filter(isDefined);

  const cards = parseMemoryEntries(normalizedResult);
  return {
    requestSections: requestEntries.length
      ? [{ type: 'keyValue', title: 'Request', entries: requestEntries }]
      : [],
    resultSections:
      cards.length > 0
        ? [{ type: 'cards', title: 'Memories', cards }]
        : summarizeResult(normalizedResult, tool.result?.isError ?? false),
  };
}

function summarizeInput(tool: ToolCallBlock): PresentationSection[] {
  const primitives: Array<{ label: string; value: string }> = [];
  const structured: Array<{ label: string; value: string }> = [];

  for (const [key, value] of Object.entries(tool.input)) {
    if (value == null) continue;
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
      primitives.push({ label: labelize(key), value: String(value) });
      continue;
    }
    structured.push({
      label: labelize(key),
      value: summarizeContentValue(value),
    });
  }

  const sections: PresentationSection[] = [];
  if (primitives.length > 0 || structured.length > 0) {
    sections.push({
      type: 'keyValue',
      title: 'Input',
      entries: [...primitives, ...structured],
    });
  }
  return sections;
}

function summarizeResult(result: string | null, isError: boolean): PresentationSection[] {
  if (!result) return [];
  const parsed = parseJson(result);
  if (parsed) {
    return summarizeJsonValue(parsed, isError);
  }

  return [
    {
      type: shouldRenderAsMarkdown(result) ? 'markdown' : 'code',
      title: isError ? 'Error' : 'Result',
      content: result,
      tone: isError ? 'error' : 'default',
    },
  ];
}

function summarizeJsonValue(value: unknown, isError: boolean): PresentationSection[] {
  const tone: SectionTone = isError ? 'error' : 'default';
  if (Array.isArray(value)) {
    if (value.length === 0) {
      return [{ type: 'markdown', title: 'Result', content: 'No items.', tone: 'muted' }];
    }
    if (value.every(isRecord)) {
      return [
        {
          type: 'cards',
          title: 'Items',
          tone,
          cards: value.map((item, index) => recordToCard(item, index)),
        },
      ];
    }
    return [{ type: 'code', title: 'Result', content: JSON.stringify(value, null, 2), tone }];
  }

  if (isRecord(value)) {
    const entries = Object.entries(value)
      .filter(([, item]) => isPrimitive(item))
      .map(([key, item]) => ({
        label: labelize(key),
        value: String(item),
      }));
    const nested = Object.entries(value).filter(([, item]) => !isPrimitive(item) && item != null);
    const sections: PresentationSection[] = [];
    if (entries.length > 0) {
      sections.push({ type: 'keyValue', title: 'Summary', entries, tone });
    }
    for (const [key, nestedValue] of nested) {
      sections.push({
        type: 'code',
        title: labelize(key),
        content: JSON.stringify(nestedValue, null, 2),
        tone,
      });
    }
    return sections.length > 0
      ? sections
      : [{ type: 'code', title: 'Result', content: JSON.stringify(value, null, 2), tone }];
  }

  return [{ type: 'code', title: 'Result', content: String(value), tone }];
}

function recordToCard(value: Record<string, unknown>, index: number) {
  const title =
    firstString(value, ['subject', 'name', 'title', 'id', 'taskName']) ?? `Item ${index + 1}`;
  const subtitle = compactParts(
    [
      firstString(value, ['status', 'state', 'executionState']),
      firstString(value, ['memoryType', 'sessionType', 'kind']),
      firstString(value, ['updatedAt', 'createdAt', 'lastMessageAt']),
    ].filter(Boolean) as string[]
  );
  const body = Object.entries(value)
    .filter(([key]) => !['subject', 'name', 'title', 'id', 'taskName'].includes(key))
    .slice(0, 5)
    .map(([key, item]) => `${labelize(key)}: ${summarizeContentValue(item)}`)
    .join('\n');

  return {
    title,
    subtitle: subtitle || undefined,
    body: body || undefined,
    badge: firstString(value, ['status', 'state', 'kind', 'memoryType']) ?? undefined,
  };
}

function parseSearchResults(result: string | null) {
  if (!result) return [];
  const matches = result.matchAll(/(?:^|\n\n)(\d+)\.\s(.+)\n\s+(\S+)\n\s+([\s\S]*?)(?=\n\n\d+\.|\s*$)/g);
  return Array.from(matches).map((match) => ({
    title: match[2].trim(),
    subtitle: match[3].trim(),
    body: match[4].trim(),
    badge: `#${match[1]}`,
  }));
}

function parseMemoryEntries(result: string | null) {
  if (!result) return [];
  return splitLines(result)
    .map((line) => {
      const match = line.match(/^\[([^\]]+)\]\s+(.+?)\s+\(([^)]+)\)$/);
      if (!match) return null;
      return {
        title: match[2],
        subtitle: match[3],
        badge: match[1],
      };
    })
    .filter(isDefined);
}

function shouldRenderAsMarkdown(result: string) {
  const trimmed = result.trim();
  if (!trimmed) return false;
  if (trimmed.includes('\n') && (trimmed.includes('# ') || trimmed.includes('* ') || trimmed.includes('- '))) {
    return true;
  }
  return trimmed.length < 600 && !trimmed.includes('[stderr]') && !trimmed.includes('{') && !trimmed.includes('[');
}

function summarizeContentValue(value: unknown): string {
  if (value == null) return '(none)';
  if (typeof value === 'string') return summarizeTextPreview(value);
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (Array.isArray(value)) return `${value.length} item${value.length === 1 ? '' : 's'}`;
  if (isRecord(value)) {
    const keys = Object.keys(value);
    return keys.length === 0 ? 'Object' : `Object with ${keys.length} field${keys.length === 1 ? '' : 's'}`;
  }
  return String(value);
}

function summarizeTextPreview(value: string | null | undefined): string {
  if (!value) return '(none)';
  const compact = value.replace(/\s+/g, ' ').trim();
  return compact.length > 120 ? `${compact.slice(0, 117)}...` : compact;
}

function pickEntries(value: Record<string, unknown>, mapping: Array<[string, string]>) {
  return mapping
    .map(([label, key]) => maybeEntry(label, primitiveString(value[key])))
    .filter(isDefined);
}

function parseJson(value: string): unknown | null {
  const trimmed = value.trim();
  if (!(trimmed.startsWith('{') || trimmed.startsWith('['))) return null;
  try {
    return JSON.parse(trimmed);
  } catch {
    return null;
  }
}

function splitLines(value: string | null) {
  return (value ?? '')
    .split('\n')
    .map((line) => line.trimEnd())
    .filter(Boolean);
}

function firstString(value: Record<string, unknown>, keys: string[]) {
  for (const key of keys) {
    const candidate = value[key];
    if (typeof candidate === 'string' && candidate.trim()) return candidate.trim();
  }
  return null;
}

function compactParts(parts: string[]) {
  return parts.filter(Boolean).join(' • ');
}

function labelize(key: string) {
  return key
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .replace(/_/g, ' ')
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function primitiveString(value: unknown): string | null {
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  return null;
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' ? value : null;
}

function booleanValue(value: unknown): string | null {
  return typeof value === 'boolean' ? (value ? 'Yes' : 'No') : null;
}

function numberValue(value: unknown, format?: (value: number) => string): string | null {
  return typeof value === 'number' ? (format ? format(value) : String(value)) : null;
}

function maybeEntry(label: string, value: string | null) {
  return value ? { label, value } : null;
}

function isPrimitive(value: unknown): value is string | number | boolean {
  return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isDefined<T>(value: T | null | undefined): value is T {
  return value != null;
}
