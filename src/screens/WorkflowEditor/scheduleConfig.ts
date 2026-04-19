import { humanSchedule } from '../../lib/humanSchedule';
import { parseScheduleInput } from '../../lib/parseScheduleInput';
import { RecurringConfig } from '../../types';

export const DEFAULT_WORKFLOW_SCHEDULE: RecurringConfig = {
  intervalUnit: 'hours',
  intervalValue: 1,
  timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  missedRunPolicy: 'skip',
};

export function getWorkflowScheduleConfig(
  data: Record<string, unknown>,
): RecurringConfig {
  if (isRecurringConfig(data)) {
    return {
      ...DEFAULT_WORKFLOW_SCHEDULE,
      ...data,
      timezone:
        typeof data.timezone === 'string' && data.timezone
          ? data.timezone
          : DEFAULT_WORKFLOW_SCHEDULE.timezone,
      missedRunPolicy:
        data.missedRunPolicy === 'run_once' ? 'run_once' : 'skip',
    };
  }

  const timezone =
    typeof data.timezone === 'string' && data.timezone
      ? data.timezone
      : DEFAULT_WORKFLOW_SCHEDULE.timezone;

  const cron = typeof data.cron === 'string' ? data.cron.trim() : '';
  if (cron) {
    const parsed = parseScheduleInput(cron, timezone);
    if (parsed) {
      return {
        ...DEFAULT_WORKFLOW_SCHEDULE,
        ...parsed,
        expression: parsed.expression ?? cron,
      };
    }
  }

  return {
    ...DEFAULT_WORKFLOW_SCHEDULE,
    timezone,
  };
}

export function describeWorkflowSchedule(data: Record<string, unknown>): string {
  const config = getWorkflowScheduleConfig(data);
  return humanSchedule(config);
}

function isRecurringConfig(value: unknown): value is RecurringConfig {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.intervalUnit === 'string' &&
    typeof candidate.intervalValue === 'number'
  );
}
