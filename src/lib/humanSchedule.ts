import { RecurringConfig } from '../types';

const DAY_NAMES = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const FULL_DAY_NAMES = [
  'Sunday',
  'Monday',
  'Tuesday',
  'Wednesday',
  'Thursday',
  'Friday',
  'Saturday',
];

function formatTime(hour: number, minute: number): string {
  const h = hour % 12 || 12;
  const m = minute.toString().padStart(2, '0');
  const ampm = hour < 12 ? 'AM' : 'PM';
  return `${h}:${m} ${ampm}`;
}

/** Converts a RecurringConfig into a human-readable English string. */
export function humanSchedule(cfg: RecurringConfig): string {
  const { intervalUnit, intervalValue, daysOfWeek, timeOfDay } = cfg;

  const timeStr = timeOfDay ? ` at ${formatTime(timeOfDay.hour, timeOfDay.minute)}` : '';

  const n = intervalValue;

  switch (intervalUnit) {
    case 'minutes':
      return n === 1 ? 'Every minute' : `Every ${n} minutes`;

    case 'hours':
      return n === 1 ? `Every hour${timeStr}` : `Every ${n} hours${timeStr}`;

    case 'days':
      return n === 1 ? `Every day${timeStr}` : `Every ${n} days${timeStr}`;

    case 'weeks': {
      if (daysOfWeek && daysOfWeek.length > 0) {
        const dayList = daysOfWeek.map((d) => FULL_DAY_NAMES[d]).join(' and ');
        return n === 1 ? `Every ${dayList}${timeStr}` : `Every ${n} weeks on ${dayList}${timeStr}`;
      }
      return n === 1 ? `Every week${timeStr}` : `Every ${n} weeks${timeStr}`;
    }

    case 'months':
      return n === 1 ? `Every month${timeStr}` : `Every ${n} months${timeStr}`;

    default:
      return 'Custom schedule';
  }
}

export { DAY_NAMES };
