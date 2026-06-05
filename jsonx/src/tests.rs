/// Tests for the JSONx path query engine

#[cfg(test)]
mod tests {
    use crate::query::{parse_path, query, Step};
    use serde_json::{json, Value};

    fn q<'a>(value: &'a Value, path: &str) -> Vec<&'a Value> {
        let steps = parse_path(path);
        query(value, &steps)
    }

    fn q_first<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
        q(value, path).into_iter().next()
    }

    // ── Key access ────────────────────────────────────────────────────────────

    #[test]
    fn simple_key_access() {
        let v = json!({"name": "alice", "age": 30});
        assert_eq!(q_first(&v, ".name"), Some(&json!("alice")));
        assert_eq!(q_first(&v, ".age"),  Some(&json!(30)));
    }

    #[test]
    fn nested_key_access() {
        let v = json!({"user": {"name": "bob", "role": "admin"}});
        assert_eq!(q_first(&v, ".user.name"), Some(&json!("bob")));
        assert_eq!(q_first(&v, ".user.role"), Some(&json!("admin")));
    }

    #[test]
    fn missing_key_returns_empty() {
        let v = json!({"name": "alice"});
        assert!(q(&v, ".missing").is_empty());
        assert!(q(&v, ".name.nested").is_empty());
    }

    #[test]
    fn root_dot_returns_whole_value() {
        let v = json!({"a": 1});
        let results = q(&v, ".");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], &v);
    }

    // ── Array index ───────────────────────────────────────────────────────────

    #[test]
    fn array_index_zero() {
        let v = json!(["a", "b", "c"]);
        assert_eq!(q_first(&v, ".[0]"), Some(&json!("a")));
    }

    #[test]
    fn array_index_last() {
        let v = json!([1, 2, 3]);
        assert_eq!(q_first(&v, ".[2]"), Some(&json!(3)));
    }

    #[test]
    fn array_index_out_of_bounds_empty() {
        let v = json!([1, 2, 3]);
        assert!(q(&v, ".[99]").is_empty());
    }

    #[test]
    fn nested_array_access() {
        let v = json!({"items": [{"id": 1}, {"id": 2}, {"id": 3}]});
        assert_eq!(q_first(&v, ".items.[1].id"), Some(&json!(2)));
    }

    // ── Wildcard ──────────────────────────────────────────────────────────────

    #[test]
    fn wildcard_on_array() {
        let v = json!([1, 2, 3]);
        let results = q(&v, ".[]");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn wildcard_extracts_field_from_all() {
        let v = json!([
            {"name": "alice"},
            {"name": "bob"},
            {"name": "carol"}
        ]);
        let results = q(&v, ".[].name");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], &json!("alice"));
        assert_eq!(results[1], &json!("bob"));
        assert_eq!(results[2], &json!("carol"));
    }

    #[test]
    fn wildcard_on_object_returns_all_values() {
        let v = json!({"a": 1, "b": 2, "c": 3});
        let results = q(&v, ".*");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn wildcard_on_empty_array() {
        let v = json!([]);
        assert!(q(&v, ".[]").is_empty());
    }

    // ── Slice ─────────────────────────────────────────────────────────────────

    #[test]
    fn slice_basic() {
        let v = json!([0, 1, 2, 3, 4]);
        let results = q(&v, ".[1:3]");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], &json!(1));
        assert_eq!(results[1], &json!(2));
    }

    #[test]
    fn slice_from_zero() {
        let v = json!([10, 20, 30, 40]);
        let results = q(&v, ".[0:2]");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], &json!(10));
    }

    #[test]
    fn slice_beyond_end_is_safe() {
        let v = json!([1, 2, 3]);
        let results = q(&v, ".[1:100]");
        assert_eq!(results.len(), 2); // only 2 and 3
    }

    // ── Filter ────────────────────────────────────────────────────────────────

    #[test]
    fn filter_eq_string() {
        let v = json!([
            {"role": "admin", "name": "alice"},
            {"role": "user",  "name": "bob"},
            {"role": "admin", "name": "carol"}
        ]);
        let results = q(&v, r#".[?(@.role == "admin")]"#);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn filter_gt_number() {
        let v = json!([
            {"price": 5},
            {"price": 15},
            {"price": 25},
            {"price": 3}
        ]);
        let results = q(&v, ".[?(@.price > 10)]");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn filter_lt_number() {
        let v = json!([
            {"n": 1},
            {"n": 5},
            {"n": 10}
        ]);
        let results = q(&v, ".[?(@.n < 6)]");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn filter_no_match_returns_empty() {
        let v = json!([{"x": 1}, {"x": 2}]);
        let results = q(&v, ".[?(@.x > 100)]");
        assert!(results.is_empty());
    }

    #[test]
    fn filter_then_field_access() {
        let v = json!([
            {"active": "true", "name": "alice"},
            {"active": "false", "name": "bob"}
        ]);
        let results = q(&v, r#".[?(@.active == "true")].name"#);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], &json!("alice"));
    }

    // ── Path parsing ──────────────────────────────────────────────────────────

    #[test]
    fn parse_simple_key() {
        let steps = parse_path(".foo");
        assert_eq!(steps.len(), 1);
        assert!(matches!(&steps[0], Step::Key(k) if k == "foo"));
    }

    #[test]
    fn parse_nested_keys() {
        let steps = parse_path(".a.b.c");
        assert_eq!(steps.len(), 3);
    }

    #[test]
    fn parse_array_index() {
        let steps = parse_path(".[0]");
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0], Step::Index(0)));
    }

    #[test]
    fn parse_wildcard() {
        let steps = parse_path(".[]");
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0], Step::Wildcard));
    }

    #[test]
    fn parse_slice() {
        let steps = parse_path(".[1:3]");
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0], Step::Slice(Some(1), Some(3))));
    }

    #[test]
    fn parse_complex_path() {
        let steps = parse_path(".users.[0].name");
        assert_eq!(steps.len(), 3);
        assert!(matches!(&steps[0], Step::Key(k) if k == "users"));
        assert!(matches!(steps[1], Step::Index(0)));
        assert!(matches!(&steps[2], Step::Key(k) if k == "name"));
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn null_value_access() {
        let v = json!({"key": null});
        assert_eq!(q_first(&v, ".key"), Some(&Value::Null));
    }

    #[test]
    fn boolean_value_access() {
        let v = json!({"flag": true});
        assert_eq!(q_first(&v, ".flag"), Some(&json!(true)));
    }

    #[test]
    fn deeply_nested() {
        let v = json!({"a": {"b": {"c": {"d": 42}}}});
        assert_eq!(q_first(&v, ".a.b.c.d"), Some(&json!(42)));
    }

    #[test]
    fn empty_path_on_array() {
        let v = json!([1, 2, 3]);
        let steps = parse_path(".");
        let results = query(&v, &steps);
        assert_eq!(results.len(), 1);
    }
}
