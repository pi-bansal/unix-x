use serde_json::Value;

/// A single step in a path expression
#[derive(Debug, Clone)]
pub enum Step {
    Key(String),          // .foo
    Index(usize),         // [0]
    Wildcard,             // []  or  .*
    Filter(String),       // [?(@.key == "val")]  simplified
    Slice(Option<usize>, Option<usize>), // [1:3]
}

/// Turn a bare dot-segment into a step. A `*` segment is a wildcard (`.*`),
/// everything else is a key.
fn make_step(segment: &str) -> Step {
    if segment == "*" {
        Step::Wildcard
    } else {
        Step::Key(segment.to_string())
    }
}

/// Parse a dot-path expression into steps.
/// Supports: .key, .key.nested, .[0], .[], .*, .key[0].other
pub fn parse_path(expr: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    let expr = expr.trim_start_matches('.');

    let mut chars = expr.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !current.is_empty() {
                    steps.push(make_step(&current));
                    current.clear();
                }
            }
            '[' => {
                if !current.is_empty() {
                    steps.push(make_step(&current));
                    current.clear();
                }
                let mut bracket = String::new();
                for bc in chars.by_ref() {
                    if bc == ']' {
                        break;
                    }
                    bracket.push(bc);
                }
                let step = parse_bracket(&bracket);
                steps.push(step);
            }
            _ => current.push(c),
        }
    }

    if !current.is_empty() {
        steps.push(make_step(&current));
    }

    steps
}

fn parse_bracket(inner: &str) -> Step {
    let inner = inner.trim();

    if inner.is_empty() || inner == "*" {
        return Step::Wildcard;
    }

    if let Ok(idx) = inner.parse::<usize>() {
        return Step::Index(idx);
    }

    // Slice: 1:3
    if inner.contains(':') {
        let parts: Vec<&str> = inner.split(':').collect();
        let start = parts.first().and_then(|s| s.parse().ok());
        let end = parts.get(1).and_then(|s| s.parse().ok());
        return Step::Slice(start, end);
    }

    // Filter: ?(@.key == "value")
    if inner.starts_with('?') {
        return Step::Filter(inner.to_string());
    }

    // String key in brackets
    Step::Key(inner.trim_matches('"').trim_matches('\'').to_string())
}

/// Apply a path to a value, returning all matching values
pub fn query<'a>(value: &'a Value, steps: &[Step]) -> Vec<&'a Value> {
    if steps.is_empty() {
        return vec![value];
    }

    let step = &steps[0];
    let rest = &steps[1..];

    match step {
        Step::Key(k) => {
            if let Some(v) = value.get(k) {
                query(v, rest)
            } else {
                vec![]
            }
        }
        Step::Index(i) => {
            if let Some(v) = value.get(i) {
                query(v, rest)
            } else {
                vec![]
            }
        }
        Step::Wildcard => {
            let mut results = Vec::new();
            match value {
                Value::Array(arr) => {
                    for item in arr {
                        results.extend(query(item, rest));
                    }
                }
                Value::Object(obj) => {
                    for v in obj.values() {
                        results.extend(query(v, rest));
                    }
                }
                _ => {}
            }
            results
        }
        Step::Slice(start, end) => {
            let mut results = Vec::new();
            if let Value::Array(arr) = value {
                let s = start.unwrap_or(0);
                let e = end.unwrap_or(arr.len());
                for item in arr.iter().skip(s).take(e.saturating_sub(s)) {
                    results.extend(query(item, rest));
                }
            }
            results
        }
        Step::Filter(expr) => {
            let mut results = Vec::new();
            if let Value::Array(arr) = value {
                for item in arr {
                    if eval_filter(item, expr) {
                        results.extend(query(item, rest));
                    }
                }
            }
            results
        }
    }
}

/// Filter evaluator for `?( ... )` expressions.
///
/// Supports boolean composition with `&&` and `||` (no nested parentheses),
/// and the comparison operators `==`, `!=`, `>=`, `<=`, `>`, `<`.
/// `||` binds looser than `&&`, matching the conventional precedence:
///   `a && b || c && d`  ==  `(a && b) || (c && d)`
fn eval_filter(item: &Value, expr: &str) -> bool {
    // Strip the leading `?` and the surrounding `( )`.
    let inner = expr.trim().trim_start_matches('?').trim();
    let inner = inner
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(inner)
        .trim();

    // OR over AND-groups.
    split_top(inner, "||")
        .iter()
        .any(|or_term| split_top(or_term, "&&").iter().all(|factor| eval_comparison(item, factor)))
}

/// Split on a two-char logical operator. We don't support parentheses inside a
/// filter, so a simple substring split is sufficient and avoids splitting the
/// operators that share characters (`>`, `<` never collide with `&&`/`||`).
fn split_top<'a>(s: &'a str, op: &str) -> Vec<&'a str> {
    s.split(op).map(|p| p.trim()).collect()
}

/// Evaluate a single `@.path <op> <literal>` comparison.
fn eval_comparison(item: &Value, factor: &str) -> bool {
    // Two-char operators must be tested before their single-char prefixes.
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some((lhs, rhs)) = factor.split_once(op) {
            let path = lhs.trim().trim_start_matches('@').trim().trim_start_matches('.');
            let rhs = rhs.trim();
            let steps = parse_path(path);
            let vals = query(item, &steps);
            return vals.iter().any(|v| compare(v, op, rhs));
        }
    }
    false
}

/// Compare a JSON value against a literal using the given operator.
fn compare(val: &Value, op: &str, rhs: &str) -> bool {
    let rhs_unquoted = rhs.trim_matches('"').trim_matches('\'');
    let rhs_num = rhs.parse::<f64>().ok();

    match op {
        "==" | "!=" => {
            let eq = match (val, rhs_num) {
                (Value::Number(_), Some(n)) => val.as_f64() == Some(n),
                _ => {
                    val.as_str() == Some(rhs_unquoted)
                        || val.to_string().trim_matches('"') == rhs_unquoted
                }
            };
            if op == "==" { eq } else { !eq }
        }
        ">" | ">=" | "<" | "<=" => match (val.as_f64(), rhs_num) {
            (Some(v), Some(n)) => match op {
                ">" => v > n,
                ">=" => v >= n,
                "<" => v < n,
                _ => v <= n,
            },
            _ => false,
        },
        _ => false,
    }
}
