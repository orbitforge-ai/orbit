import { RRule } from 'rrule';
import { RecurringConfig } from '../types';

/**
 * Parses a natural-language schedule string or a 5-field cron expression
 * into a RecurringConfig. Returns null if the input can't be understood.
 */
export function parseScheduleInput(raw: string, timezone: string): RecurringConfig | null {
  const input = raw.trim().toLowerCase();
  if (!input) return null;

  // Try cron first (5 space-separated tokens, all look like cron fields)
  const cronResult = parseCron(input, timezone);
  if (cronResult) return cronResult;

  // Natural language via rrule
  return parseNatural(input, timezone);
}

// ─── Cron ────────────────────────────────────────────────────────────────────

const CRON_FIELD = /^[\d*,/\-]+$/;

function parseCron(input: string, timezone: string): RecurringConfig | null {
  const parts = input.split(/\s+/);
  if (parts.length !== 5 || !parts.every((p) => CRON_FIELD.test(p))) return null;

  const [minute, hour, _dom, _month, dow] = parts;

  const timeOfDay =
    parseSimpleNum(hour) != null && parseSimpleNum(minute) != null
      ? { hour: parseSimpleNum(hour)!, minute: parseSimpleNum(minute)! }
      : undefined;

  // Detect step-based intervals: */N in minute or hour field
  const minStep = parseStep(minute);
  if (minStep && hour === '*') {
    return {
      intervalUnit: 'minutes',
      intervalValue: minStep,
      timezone,
      missedRunPolicy: 'skip',
    };
  }
  const hourStep = parseStep(hour);
  if (hourStep && minute !== '*') {
    return {
      intervalUnit: 'hours',
      intervalValue: hourStep,
      timeOfDay: { hour: 0, minute: parseSimpleNum(minute) ?? 0 },
      timezone,
      missedRunPolicy: 'skip',
    };
  }
  if (hour === '*' && parseSimpleNum(minute) != null) {
    return {
      intervalUnit: 'hours',
      intervalValue: 1,
      timeOfDay: { hour: 0, minute: parseSimpleNum(minute) ?? 0 },
      timezone,
      missedRunPolicy: 'skip',
    };
  }

  // Specific days of week
  const daysOfWeek = parseDowField(dow);

  if (daysOfWeek && daysOfWeek.length > 0 && daysOfWeek.length < 7) {
    return {
      intervalUnit: 'weeks',
      intervalValue: 1,
      daysOfWeek,
      timeOfDay,
      timezone,
      missedRunPolicy: 'skip',
    };
  }

  // Daily (dow is * or 0-6 full range)
  if (timeOfDay) {
    return {
      intervalUnit: 'days',
      intervalValue: 1,
      timeOfDay,
      timezone,
      missedRunPolicy: 'skip',
    };
  }

  return null;
}

function parseStep(field: string): number | null {
  const m = field.match(/^\*\/(\d+)$/);
  return m ? Number(m[1]) : null;
}

function parseSimpleNum(field: string): number | null {
  return /^\d+$/.test(field) ? Number(field) : null;
}

function parseDowField(field: string): number[] | undefined {
  if (field === '*') return undefined;

  const DAY_MAP: Record<string, number> = {
    sun: 0,
    mon: 1,
    tue: 2,
    wed: 3,
    thu: 4,
    fri: 5,
    sat: 6,
    '0': 0,
    '1': 1,
    '2': 2,
    '3': 3,
    '4': 4,
    '5': 5,
    '6': 6,
    '7': 0,
  };

  const days = new Set<number>();
  for (const part of field.split(',')) {
    const range = part.match(/^(\w+)-(\w+)$/);
    if (range) {
      const start = DAY_MAP[range[1]];
      const end = DAY_MAP[range[2]];
      if (start == null || end == null) return undefined;
      for (let i = start; i !== (end + 1) % 7; i = (i + 1) % 7) days.add(i);
      days.add(end);
    } else {
      const d = DAY_MAP[part];
      if (d == null) return undefined;
      days.add(d);
    }
  }
  return [...days].sort();
}

// ─── Natural language (via rrule) ───────────────────────────────────────────

const FREQ_MAP: Record<number, RecurringConfig['intervalUnit'] | null> = {
  [RRule.MINUTELY]: 'minutes',
  [RRule.HOURLY]: 'hours',
  [RRule.DAILY]: 'days',
  [RRule.WEEKLY]: 'weeks',
  [RRule.MONTHLY]: 'months',
  [RRule.YEARLY]: null,
};

function parseNatural(input: string, timezone: string): RecurringConfig | null {
  const text = normalizeNaturalScheduleText(input);

  let rule: RRule;
  try {
    rule = RRule.fromText(text);
  } catch {
    return null;
  }

  const opts = rule.options;
  const unit = FREQ_MAP[opts.freq];
  if (unit == null) return null;

  // Day-of-week mapping: rrule uses MO=0…SU=6, RecurringConfig uses SU=0…SA=6
  let daysOfWeek: number[] | undefined;
  if (opts.byweekday && opts.byweekday.length > 0) {
    daysOfWeek = opts.byweekday.map((wd) => (wd + 1) % 7).sort((a, b) => a - b);
  }

  // Prefer parsing explicit times from the input text directly because rrule
  // may inherit the current minute for phrases like "9am".
  const explicitTime = extractExplicitTime(input);
  let timeOfDay: { hour: number; minute: number } | undefined;

  if (explicitTime) {
    timeOfDay = explicitTime;
  } else if (['days', 'weeks', 'months'].includes(unit)) {
    timeOfDay = { hour: 9, minute: 0 };
  }

  return {
    intervalUnit: unit,
    intervalValue: opts.interval ?? 1,
    ...(daysOfWeek && { daysOfWeek }),
    ...(timeOfDay && { timeOfDay }),
    timezone,
    missedRunPolicy: 'skip',
  };
}

function normalizeNaturalScheduleText(input: string): string {
  let text = input.trim().toLowerCase().replace(/\s+/g, ' ');

  const prefixAliases: Array<[RegExp, string]> = [
    [/^daily\b/, 'every day'],
    [/^weekly\b/, 'every week'],
    [/^monthly\b/, 'every month'],
    [/^hourly\b/, 'every hour'],
    [/^minutely\b/, 'every minute'],
    [/^weekdays?\b/, 'every weekday'],
    [/^weekends?\b/, 'every weekend'],
  ];

  for (const [pattern, replacement] of prefixAliases) {
    if (pattern.test(text)) {
      text = text.replace(pattern, replacement);
      break;
    }
  }

  // Expand shorthand phrases that rrule's NLP parser either rejects or
  // silently misinterprets as a yearly rule anchored to today.
  if (/every\s+weekday/i.test(text)) {
    text = text.replace(
      /every\s+weekday(s)?/i,
      'every week on monday, tuesday, wednesday, thursday and friday'
    );
  }
  if (/every\s+weekend/i.test(text)) {
    text = text.replace(/every\s+weekend(s)?/i, 'every week on saturday and sunday');
  }

  return text;
}

function extractExplicitTime(input: string): { hour: number; minute: number } | null {
  const amPmMatch = input.match(/\b(\d{1,2})(?::(\d{2}))?\s*(am|pm)\b/i);
  if (amPmMatch) {
    const hour12 = Number(amPmMatch[1]);
    const minute = Number(amPmMatch[2] ?? '0');
    if (hour12 < 1 || hour12 > 12 || minute < 0 || minute > 59) return null;
    const meridiem = amPmMatch[3].toLowerCase();
    const hour = meridiem === 'pm' ? (hour12 % 12) + 12 : hour12 % 12;
    return { hour, minute };
  }

  const atTimeMatch = input.match(/\bat\s+(\d{1,2})(?::(\d{2}))?\b/i);
  if (atTimeMatch) {
    const hour = Number(atTimeMatch[1]);
    const minute = Number(atTimeMatch[2] ?? '0');
    if (hour < 0 || hour > 23 || minute < 0 || minute > 59) return null;
    return { hour, minute };
  }

  return null;
}
