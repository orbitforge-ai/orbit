import type { RuleNode, RuleOperator } from '../../types';

const OP_LABELS: Record<RuleOperator, string> = {
  equals: 'equals',
  notEquals: 'does not equal',
  contains: 'contains',
  notContains: 'does not contain',
  startsWith: 'starts with',
  endsWith: 'ends with',
  greaterThan: '>',
  greaterThanOrEqual: '≥',
  lessThan: '<',
  lessThanOrEqual: '≤',
  exists: 'exists',
  notExists: 'does not exist',
  isTrue: 'is true',
  isFalse: 'is false',
  matchesRegex: 'matches /…/',
};

function isGroup(node: RuleNode): node is { combinator: 'and' | 'or'; rules: RuleNode[] } {
  return (node as { combinator?: unknown }).combinator !== undefined;
}

function describeValue(v: unknown): string {
  if (v === undefined || v === null) return '';
  if (typeof v === 'object' && 'field' in (v as object)) {
    return `field ${(v as { field: string }).field}`;
  }
  if (typeof v === 'string') return JSON.stringify(v);
  return String(v);
}

export function ruleToSentence(rule: RuleNode | null | undefined): string {
  if (!rule) return '';
  if (isGroup(rule)) {
    if (rule.rules.length === 0) return '(no conditions)';
    const joiner = ` ${rule.combinator.toUpperCase()} `;
    const parts = rule.rules.map((r) => {
      const text = ruleToSentence(r);
      return isGroup(r) && r.rules.length > 1 ? `(${text})` : text;
    });
    return parts.join(joiner);
  }
  const op = OP_LABELS[rule.operator] ?? rule.operator;
  const valueless = ['exists', 'notExists', 'isTrue', 'isFalse'].includes(rule.operator);
  if (valueless) return `${rule.field} ${op}`;
  return `${rule.field} ${op} ${describeValue(rule.value)}`.trim();
}
