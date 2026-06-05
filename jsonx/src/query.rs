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

/// Very simple filter evaluator: ?(@.key == "val") or ?(@.key > N)
fn eval_filter(item: &Value, expr: &str) -> bool {
    // Strip ?( and )
    let inner = expr
        .trim_start_matches('?')
        .trim_start_matches('(')
        .trim_start_matches('@')
        .trim_end_matches(')');

    // Try == comparison
    if let Some((path, rhs)) = inner.split_once("==") {
        let path = path.trim().trim_start_matches('.');
        let rhs = rhs.trim().trim_matches('"').trim_matches('\'');
        let steps = parse_path(path);
        let vals = query(item, &steps);
        return vals.iter().any(|v| {
            v.as_str() == Some(rhs)
                || v.to_string().trim_matches('"') == rhs
        });
    }

    // Try > comparison
    if let Some((path, rhs)) = inner.split_once('>') {
        let path = path.trim().trim_start_matches('.');
        if let Ok(threshold) = rhs.trim().parse::<f64>() {
            let steps = parse_path(path);
            let vals = query(item, &steps);
            return vals.iter().any(|v| v.as_f64().map(|n| n > threshold).unwrap_or(false));
        }
    }

    // Try < comparison
    if let Some((path, rhs)) = inner.split_once('<') {
        let path = path.trim().trim_start_matches('.');
        if let Ok(threshold) = rhs.trim().parse::<f64>() {
            let steps = parse_path(path);
            let vals = query(item, &steps);
            return vals.iter().any(|v| v.as_f64().map(|n| n < threshold).unwrap_or(false));
        }
    }

    false
}
