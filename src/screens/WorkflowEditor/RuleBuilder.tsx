import { Plus, Trash2 } from 'lucide-react';
import {
  RULE_OPERATORS,
  RuleCombinator,
  RuleGroup,
  RuleLeaf,
  RuleNode,
  RuleOperator,
} from '../../types';
import { Input, SimpleSelect } from '../../components/ui';
import { useOutputInsertionField } from './outputInsertion';

const VALUELESS_OPS: RuleOperator[] = ['exists', 'notExists', 'isTrue', 'isFalse'];
const BOOL_OPS: RuleOperator[] = [];
const NUMERIC_OPS: RuleOperator[] = ['greaterThan', 'greaterThanOrEqual', 'lessThan', 'lessThanOrEqual'];

function isGroup(node: RuleNode): node is RuleGroup {
  return (node as RuleGroup).combinator !== undefined;
}

function emptyLeaf(): RuleLeaf {
  return { field: '', operator: 'equals', value: '' };
}

function emptyGroup(combinator: RuleCombinator = 'and'): RuleGroup {
  return { combinator, rules: [] };
}

function defaultRoot(): RuleGroup {
  return { combinator: 'and', rules: [] };
}

interface Props {
  rule: RuleNode | null | undefined;
  onChange: (rule: RuleGroup) => void;
}

export function RuleBuilder({ rule, onChange }: Props) {
  const root: RuleGroup = rule && isGroup(rule) ? rule : defaultRoot();
  return (
    <GroupView
      group={root}
      depth={0}
      onChange={(g) => onChange(g)}
    />
  );
}

function GroupView({
  group,
  depth,
  onChange,
  onDelete,
}: {
  group: RuleGroup;
  depth: number;
  onChange: (group: RuleGroup) => void;
  onDelete?: () => void;
}) {
  const setCombinator = (c: RuleCombinator) => onChange({ ...group, combinator: c });
  const updateRule = (idx: number, child: RuleNode) =>
    onChange({ ...group, rules: group.rules.map((r, i) => (i === idx ? child : r)) });
  const removeRule = (idx: number) =>
    onChange({ ...group, rules: group.rules.filter((_, i) => i !== idx) });
  const addLeaf = () => onChange({ ...group, rules: [...group.rules, emptyLeaf()] });
  const addGroup = () => onChange({ ...group, rules: [...group.rules, emptyGroup()] });

  return (
    <div
      className={
        'rounded-lg border border-edge p-2.5 space-y-2 ' +
        (depth === 0 ? 'bg-background' : 'bg-surface')
      }
    >
      <div className="flex items-center gap-2">
        <SimpleSelect
          value={group.combinator}
          onValueChange={(v) => setCombinator(v as RuleCombinator)}
          className="w-auto bg-background rounded px-2 py-1 text-xs"
          options={[
            { value: 'and', label: 'AND' },
            { value: 'or', label: 'OR' },
          ]}
        />
        <span className="text-[11px] text-muted">
          {group.rules.length === 0
            ? '(no conditions)'
            : group.rules.length === 1
              ? '1 condition'
              : `${group.rules.length} conditions`}
        </span>
        <div className="flex-1" />
        {onDelete && (
          <button
            onClick={onDelete}
            className="p-1 rounded text-muted hover:text-red-400 hover:bg-red-400/10 transition-colors"
            title="Delete group"
          >
            <Trash2 size={12} />
          </button>
        )}
      </div>

      {group.rules.length > 0 && (
        <div className="space-y-2">
          {group.rules.map((child, idx) =>
            isGroup(child) ? (
              <GroupView
                key={idx}
                group={child}
                depth={depth + 1}
                onChange={(g) => updateRule(idx, g)}
                onDelete={() => removeRule(idx)}
              />
            ) : (
              <LeafView
                key={idx}
                leaf={child}
                onChange={(l) => updateRule(idx, l)}
                onDelete={() => removeRule(idx)}
              />
            ),
          )}
        </div>
      )}

      <div className="flex gap-2 pt-1">
        <button
          onClick={addLeaf}
          className="flex items-center gap-1 px-2 py-1 rounded text-[11px] text-muted hover:text-white border border-edge hover:border-edge-hover transition-colors"
        >
          <Plus size={10} />
          Add condition
        </button>
        <button
          onClick={addGroup}
          className="flex items-center gap-1 px-2 py-1 rounded text-[11px] text-muted hover:text-white border border-edge hover:border-edge-hover transition-colors"
        >
          <Plus size={10} />
          Add group
        </button>
      </div>
    </div>
  );
}

function LeafView({
  leaf,
  onChange,
  onDelete,
}: {
  leaf: RuleLeaf;
  onChange: (leaf: RuleLeaf) => void;
  onDelete: () => void;
}) {
  const setField = (field: string) => onChange({ ...leaf, field });
  const setOperator = (operator: RuleOperator) => {
    const next: RuleLeaf = { ...leaf, operator };
    if (VALUELESS_OPS.includes(operator)) next.value = undefined;
    onChange(next);
  };
  const setValue = (value: unknown) => onChange({ ...leaf, value });

  const valueless = VALUELESS_OPS.includes(leaf.operator);
  const numeric = NUMERIC_OPS.includes(leaf.operator);
  const boolean = BOOL_OPS.includes(leaf.operator);
  const fieldBinding = useOutputInsertionField<HTMLInputElement>({
    mode: 'raw',
    onChange: setField,
    value: leaf.field,
  });

  return (
    <div className="flex flex-wrap items-center gap-2 p-2 rounded border border-edge bg-background">
      <Input
        {...fieldBinding.bind}
        value={leaf.field}
        onChange={(e) => setField(e.target.value)}
        placeholder="field (e.g. triage-agent.output.category)"
        className="flex-1 min-w-[140px] rounded px-2 py-1 text-xs placeholder-muted font-mono"
      />
      <SimpleSelect
        value={leaf.operator}
        onValueChange={(v) => setOperator(v as RuleOperator)}
        className="w-auto rounded px-2 py-1 text-xs"
        options={RULE_OPERATORS.map((op) => ({ value: op, label: op }))}
      />
      {!valueless && (
        <Input
          value={typeof leaf.value === 'string' || typeof leaf.value === 'number' ? String(leaf.value) : ''}
          onChange={(e) => {
            const raw = e.target.value;
            if (numeric) {
              const num = Number(raw);
              setValue(Number.isFinite(num) && raw !== '' ? num : raw);
            } else if (boolean) {
              setValue(raw === 'true');
            } else {
              setValue(raw);
            }
          }}
          placeholder={numeric ? 'number' : 'value'}
          className="flex-1 min-w-[100px] rounded px-2 py-1 text-xs placeholder-muted"
        />
      )}
      <button
        onClick={onDelete}
        className="p-1 rounded text-muted hover:text-red-400 hover:bg-red-400/10 transition-colors"
        title="Delete condition"
      >
        <Trash2 size={12} />
      </button>
    </div>
  );
}
