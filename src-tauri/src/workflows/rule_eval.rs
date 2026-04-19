use serde_json::Value;

use crate::models::project_workflow::{RuleGroup, RuleLeaf, RuleNode};

const OUTPUT_ALIASES_KEY: &str = "__aliases";

/// Evaluate a rule tree against a `serde_json::Value` representing the
/// outputs of upstream nodes (typically `{ "<nodeId>": { "output": ... }, ... }`).
///
/// Pure function — no I/O, no panics, no `eval`. Unknown operators or missing
/// fields evaluate to `false`. The orchestrator uses this for `logic.if`.
pub fn eval_rule(rule: &RuleNode, outputs: &Value) -> bool {
    match rule {
        RuleNode::Group(g) => eval_group(g, outputs),
        RuleNode::Leaf(l) => eval_leaf(l, outputs),
    }
}

fn eval_group(group: &RuleGroup, outputs: &Value) -> bool {
    match group.combinator.as_str() {
        "and" => group.rules.iter().all(|r| eval_rule(r, outputs)),
        "or" => group.rules.iter().any(|r| eval_rule(r, outputs)),
        _ => false,
    }
}

fn eval_leaf(leaf: &RuleLeaf, outputs: &Value) -> bool {
    let lhs = lookup_field(&leaf.field, outputs);
    let rhs = resolve_value(&leaf.value, outputs);

    match leaf.operator.as_str() {
        "equals" => values_equal(&lhs, &rhs),
        "notEquals" => !values_equal(&lhs, &rhs),
        "contains" => contains(&lhs, &rhs),
        "notContains" => !contains(&lhs, &rhs),
        "startsWith" => string_op(&lhs, &rhs, |a, b| a.starts_with(b)),
        "endsWith" => string_op(&lhs, &rhs, |a, b| a.ends_with(b)),
        "greaterThan" => number_op(&lhs, &rhs, |a, b| a > b),
        "greaterThanOrEqual" => number_op(&lhs, &rhs, |a, b| a >= b),
        "lessThan" => number_op(&lhs, &rhs, |a, b| a < b),
        "lessThanOrEqual" => number_op(&lhs, &rhs, |a, b| a <= b),
        "exists" => lhs.is_some(),
        "notExists" => lhs.is_none(),
        "isTrue" => matches!(lhs, Some(Value::Bool(true))),
        "isFalse" => matches!(lhs, Some(Value::Bool(false))),
        "matchesRegex" => match (&lhs, &rhs) {
            (Some(Value::String(s)), Some(Value::String(pat))) => regex::Regex::new(pat)
                .map(|r| r.is_match(s))
                .unwrap_or(false),
            _ => false,
        },
        _ => false,
    }
}

fn lookup_field(path: &str, outputs: &Value) -> Option<Value> {
    let mut segments = path.split('.');
    let first = segments.next()?;
    let resolved_first = resolve_output_root_segment(first, outputs)?;
    let mut cur = outputs.get(resolved_first)?;
    for segment in segments {
        match cur.get(segment) {
            Some(next) => cur = next,
            None => return None,
        }
    }
    Some(cur.clone())
}

fn resolve_output_root_segment<'a>(segment: &'a str, outputs: &'a Value) -> Option<&'a str> {
    if outputs.get(segment).is_some() {
        return Some(segment);
    }
    outputs.get(OUTPUT_ALIASES_KEY)?.get(segment)?.as_str()
}

/// If `value` is a `{ "field": "..." }` reference, resolve it; otherwise return as-is.
fn resolve_value(value: &Value, outputs: &Value) -> Option<Value> {
    if let Some(obj) = value.as_object() {
        if obj.len() == 1 {
            if let Some(Value::String(path)) = obj.get("field") {
                return lookup_field(path, outputs);
            }
        }
    }
    Some(value.clone())
}

fn values_equal(a: &Option<Value>, b: &Option<Value>) -> bool {
    matches!((a, b), (Some(x), Some(y)) if x == y)
}

fn contains(a: &Option<Value>, b: &Option<Value>) -> bool {
    match (a, b) {
        (Some(Value::String(s)), Some(Value::String(n))) => s.contains(n.as_str()),
        (Some(Value::Array(arr)), Some(needle)) => arr.iter().any(|item| item == needle),
        _ => false,
    }
}

fn string_op<F: Fn(&str, &str) -> bool>(a: &Option<Value>, b: &Option<Value>, op: F) -> bool {
    match (a, b) {
        (Some(Value::String(s)), Some(Value::String(t))) => op(s, t),
        _ => false,
    }
}

fn to_number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_i64().map(|n| n as f64))
}

fn number_op<F: Fn(f64, f64) -> bool>(a: &Option<Value>, b: &Option<Value>, op: F) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => match (to_number(x), to_number(y)) {
            (Some(p), Some(q)) => op(p, q),
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn outputs() -> Value {
        json!({
            "__aliases": {
                "triage-agent": "n2"
            },
            "n2": {
                "output": {
                    "category": "reply_needed",
                    "priority": 3,
                    "labels": ["urgent", "ops"],
                    "approved": true,
                }
            }
        })
    }

    #[test]
    fn equals_works() {
        let leaf = RuleLeaf {
            field: "n2.output.category".into(),
            operator: "equals".into(),
            value: json!("reply_needed"),
        };
        assert!(eval_leaf(&leaf, &outputs()));
    }

    #[test]
    fn nested_and_or_groups() {
        let rule = RuleNode::Group(RuleGroup {
            combinator: "and".into(),
            rules: vec![
                RuleNode::Leaf(RuleLeaf {
                    field: "n2.output.category".into(),
                    operator: "equals".into(),
                    value: json!("reply_needed"),
                }),
                RuleNode::Group(RuleGroup {
                    combinator: "or".into(),
                    rules: vec![
                        RuleNode::Leaf(RuleLeaf {
                            field: "n2.output.priority".into(),
                            operator: "greaterThanOrEqual".into(),
                            value: json!(3),
                        }),
                        RuleNode::Leaf(RuleLeaf {
                            field: "n2.output.labels".into(),
                            operator: "contains".into(),
                            value: json!("urgent"),
                        }),
                    ],
                }),
            ],
        });
        assert!(eval_rule(&rule, &outputs()));
    }

    #[test]
    fn missing_field_returns_false() {
        let leaf = RuleLeaf {
            field: "n2.output.does_not_exist".into(),
            operator: "equals".into(),
            value: json!("anything"),
        };
        assert!(!eval_leaf(&leaf, &outputs()));
    }

    #[test]
    fn unknown_operator_returns_false() {
        let leaf = RuleLeaf {
            field: "n2.output.category".into(),
            operator: "spaceship".into(),
            value: json!("x"),
        };
        assert!(!eval_leaf(&leaf, &outputs()));
    }

    #[test]
    fn field_to_field_reference() {
        let leaf = RuleLeaf {
            field: "n2.output.priority".into(),
            operator: "equals".into(),
            value: json!({ "field": "n2.output.priority" }),
        };
        assert!(eval_leaf(&leaf, &outputs()));
    }

    #[test]
    fn is_true_and_is_false() {
        let t = RuleLeaf {
            field: "n2.output.approved".into(),
            operator: "isTrue".into(),
            value: Value::Null,
        };
        assert!(eval_leaf(&t, &outputs()));
        let f = RuleLeaf {
            field: "n2.output.approved".into(),
            operator: "isFalse".into(),
            value: Value::Null,
        };
        assert!(!eval_leaf(&f, &outputs()));
    }

    #[test]
    fn reference_key_aliases_work() {
        let leaf = RuleLeaf {
            field: "triage-agent.output.category".into(),
            operator: "equals".into(),
            value: json!("reply_needed"),
        };
        assert!(eval_leaf(&leaf, &outputs()));
    }
}
