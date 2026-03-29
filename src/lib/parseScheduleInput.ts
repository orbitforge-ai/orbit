import { RecurringConfig } from "../types";

/**
 * Parses a natural-language schedule string or a 5-field cron expression
 * into a RecurringConfig. Returns null if the input can't be understood.
 */
export function parseScheduleInput(
  raw: string,
  timezone: string,
): RecurringConfig | null {
  const input = raw.trim().toLowerCase();
  if (!input) return null;

  // Try cron first (5 space-separated tokens, all look like cron fields)
  const cronResult = parseCron(input, timezone);
  if (cronResult) return cronResult;

  // Natural language
  return parseNatural(input, timezone);
}

// ─── Cron ────────────────────────────────────────────────────────────────────

const CRON_FIELD = /^[\d*,/\-]+$/;

function parseCron(input: string, timezone: string): RecurringConfig | null {
  const parts = input.split(/\s+/);
  if (parts.length !== 5 || !parts.every((p) => CRON_FIELD.test(p))) return null;

  const [minute, hour, _dom, _month, dow] = parts;

  const timeOfDay = parseSimpleNum(hour) != null && parseSimpleNum(minute) != null
    ? { hour: parseSimpleNum(hour)!, minute: parseSimpleNum(minute)! }
    : undefined;

  // Detect step-based intervals: */N in minute or hour field
  const minStep = parseStep(minute);
  if (minStep && hour === "*") {
    return {
      intervalUnit: "minutes",
      intervalValue: minStep,
      timezone,
      missedRunPolicy: "skip",
    };
  }
  const hourStep = parseStep(hour);
  if (hourStep && minute !== "*") {
    return {
      intervalUnit: "hours",
      intervalValue: hourStep,
      timeOfDay: { hour: 0, minute: parseSimpleNum(minute) ?? 0 },
      timezone,
      missedRunPolicy: "skip",
    };
  }

  // Specific days of week
  const daysOfWeek = parseDowField(dow);

  if (daysOfWeek && daysOfWeek.length > 0 && daysOfWeek.length < 7) {
    return {
      intervalUnit: "weeks",
      intervalValue: 1,
      daysOfWeek,
      timeOfDay,
      timezone,
      missedRunPolicy: "skip",
    };
  }

  // Daily (dow is * or 0-6 full range)
  if (timeOfDay) {
    return {
      intervalUnit: "days",
      intervalValue: 1,
      timeOfDay,
      timezone,
      missedRunPolicy: "skip",
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
  if (field === "*") return undefined;

  const DAY_MAP: Record<string, number> = {
    sun: 0, mon: 1, tue: 2, wed: 3, thu: 4, fri: 5, sat: 6,
    "0": 0, "1": 1, "2": 2, "3": 3, "4": 4, "5": 5, "6": 6, "7": 0,
  };

  const days = new Set<number>();
  for (const part of field.split(",")) {
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

// ─── Natural language ────────────────────────────────────────────────────────

const DAY_WORDS: Record<string, number> = {
  sunday: 0, sun: 0,
  monday: 1, mon: 1,
  tuesday: 2, tue: 2, tues: 2,
  wednesday: 3, wed: 3,
  thursday: 4, thu: 4, thur: 4, thurs: 4,
  friday: 5, fri: 5,
  saturday: 6, sat: 6,
};

const UNIT_WORDS: Record<string, RecurringConfig["intervalUnit"]> = {
  minute: "minutes", minutes: "minutes", min: "minutes", mins: "minutes",
  hour: "hours", hours: "hours", hr: "hours", hrs: "hours",
  day: "days", days: "days",
  week: "weeks", weeks: "weeks",
  month: "months", months: "months",
};

function parseNatural(input: string, timezone: string): RecurringConfig | null {
  // Extract time if present: "at 9am", "at 9:30pm", "at 14:00", "at 9:30 am"
  let timeOfDay: { hour: number; minute: number } | undefined;
  const timeMatch = input.match(
    /at\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?/i,
  );
  if (timeMatch) {
    let hour = Number(timeMatch[1]);
    const minute = timeMatch[2] ? Number(timeMatch[2]) : 0;
    const ampm = timeMatch[3]?.toLowerCase();
    if (ampm === "pm" && hour < 12) hour += 12;
    if (ampm === "am" && hour === 12) hour = 0;
    if (hour > 23 || minute > 59) return null;
    timeOfDay = { hour, minute };
  }

  // "hourly" / "daily" / "weekly" / "monthly"
  if (/^hourly/.test(input)) {
    return cfg("hours", 1, timezone, undefined, timeOfDay);
  }
  if (/^daily/.test(input)) {
    return cfg("days", 1, timezone, undefined, timeOfDay ?? { hour: 9, minute: 0 });
  }
  if (/^weekly/.test(input)) {
    const days = extractDays(input);
    return cfg("weeks", 1, timezone, days.length ? days : undefined, timeOfDay ?? { hour: 9, minute: 0 });
  }
  if (/^monthly/.test(input)) {
    return cfg("months", 1, timezone, undefined, timeOfDay ?? { hour: 9, minute: 0 });
  }

  // "every weekday(s) …"
  if (/every\s+weekday/.test(input)) {
    return cfg("weeks", 1, timezone, [1, 2, 3, 4, 5], timeOfDay ?? { hour: 9, minute: 0 });
  }

  // "every weekend …"
  if (/every\s+weekend/.test(input)) {
    return cfg("weeks", 1, timezone, [0, 6], timeOfDay ?? { hour: 9, minute: 0 });
  }

  // "every monday and wednesday at 3pm"
  const dayListMatch = input.match(
    /every\s+((?:(?:sunday|monday|tuesday|wednesday|thursday|friday|saturday|sun|mon|tue|tues|wed|thu|thur|thurs|fri|sat)(?:\s*(?:,|and)\s*)?)+)/i,
  );
  if (dayListMatch) {
    const days = extractDays(dayListMatch[1]);
    if (days.length > 0) {
      return cfg("weeks", 1, timezone, days, timeOfDay ?? { hour: 9, minute: 0 });
    }
  }

  // "every N <unit>" or "every <unit>"
  const everyMatch = input.match(
    /every\s+(?:(\d+)\s+)?(\w+)/,
  );
  if (everyMatch) {
    const n = everyMatch[1] ? Number(everyMatch[1]) : 1;
    const unit = UNIT_WORDS[everyMatch[2]];
    if (unit) {
      const needsTime = ["days", "weeks", "months"].includes(unit);
      return cfg(unit, n, timezone, undefined, needsTime ? (timeOfDay ?? { hour: 9, minute: 0 }) : timeOfDay);
    }
  }

  return null;
}

function extractDays(text: string): number[] {
  const days = new Set<number>();
  for (const [word, num] of Object.entries(DAY_WORDS)) {
    if (text.includes(word)) days.add(num);
  }
  return [...days].sort();
}

function cfg(
  intervalUnit: RecurringConfig["intervalUnit"],
  intervalValue: number,
  timezone: string,
  daysOfWeek?: number[],
  timeOfDay?: { hour: number; minute: number },
): RecurringConfig {
  return {
    intervalUnit,
    intervalValue,
    ...(daysOfWeek && { daysOfWeek }),
    ...(timeOfDay && { timeOfDay }),
    timezone,
    missedRunPolicy: "skip",
  };
}
