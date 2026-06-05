/// Tests for the bloom filter layer

#[cfg(test)]
mod tests {
    use crate::bloom::{rebuild_bloom, BloomSet};

    fn make_bloom() -> BloomSet {
        let paths = vec![
            "/project/src/main.rs".to_string(),
            "/project/src/lib.rs".to_string(),
            "/project/tests/integration.rs".to_string(),
            "/project/Cargo.toml".to_string(),
            "/project/README.md".to_string(),
            "/project/build.rs".to_string(),
        ];
        rebuild_bloom(&paths, paths.len())
    }

    // ── Extension bloom ───────────────────────────────────────────────────────

    #[test]
    fn ext_present_returns_true() {
        let bloom = make_bloom();
        assert!(bloom.might_have_ext("rs"));
        assert!(bloom.might_have_ext("toml"));
        assert!(bloom.might_have_ext("md"));
    }

    #[test]
    fn ext_absent_returns_false() {
        let bloom = make_bloom();
        // These definitely don't exist — bloom must not false-negative
        assert!(!bloom.might_have_ext("py"));
        assert!(!bloom.might_have_ext("go"));
        assert!(!bloom.might_have_ext("java"));
        assert!(!bloom.might_have_ext("cpp"));
    }

    // ── Filename bloom ────────────────────────────────────────────────────────

    #[test]
    fn filename_present_returns_true() {
        let bloom = make_bloom();
        assert!(bloom.might_have_filename("main.rs"));
        assert!(bloom.might_have_filename("Cargo.toml"));
        assert!(bloom.might_have_filename("README.md"));
    }

    #[test]
    fn filename_absent_returns_false() {
        let bloom = make_bloom();
        assert!(!bloom.might_have_filename("main.go"));
        assert!(!bloom.might_have_filename("package.json"));
        assert!(!bloom.might_have_filename("Makefile"));
    }

    // ── Prefix bloom ─────────────────────────────────────────────────────────

    #[test]
    fn prefix_present_returns_true() {
        let bloom = make_bloom();
        assert!(bloom.might_have_prefix("src"));
        assert!(bloom.might_have_prefix("tests"));
    }

    #[test]
    fn prefix_absent_returns_false() {
        let bloom = make_bloom();
        assert!(!bloom.might_have_prefix("vendor"));
        assert!(!bloom.might_have_prefix("node_modules"));
        assert!(!bloom.might_have_prefix("target"));
    }

    // ── No false negatives ────────────────────────────────────────────────────

    #[test]
    fn no_false_negatives_on_large_set() {
        // Insert 1000 paths and verify all are reported as present
        let paths: Vec<String> = (0..1000)
            .map(|i| format!("/project/src/file_{}.rs", i))
            .collect();
        let bloom = rebuild_bloom(&paths, paths.len());

        for i in 0..1000 {
            assert!(
                bloom.might_have_filename(&format!("file_{}.rs", i)),
                "false negative for file_{}.rs", i
            );
        }
    }

    #[test]
    fn no_false_negatives_for_ext_on_large_set() {
        let paths: Vec<String> = (0..500)
            .flat_map(|i| vec![
                format!("/project/src/a_{}.rs", i),
                format!("/project/src/b_{}.ts", i),
            ])
            .collect();
        let bloom = rebuild_bloom(&paths, paths.len());

        // Must never miss rs or ts
        assert!(bloom.might_have_ext("rs"));
        assert!(bloom.might_have_ext("ts"));
        // Must correctly miss py and go
        assert!(!bloom.might_have_ext("py"));
        assert!(!bloom.might_have_ext("go"));
    }

    // ── Rebuild ───────────────────────────────────────────────────────────────

    #[test]
    fn rebuild_from_empty_paths() {
        let bloom = rebuild_bloom(&[], 100);
        // Empty bloom — nothing should be present
        assert!(!bloom.might_have_ext("rs"));
        assert!(!bloom.might_have_filename("main.rs"));
    }

    #[test]
    fn rebuild_single_path() {
        let paths = vec!["/solo/only_file.json".to_string()];
        let bloom = rebuild_bloom(&paths, 10);
        assert!(bloom.might_have_ext("json"));
        assert!(bloom.might_have_filename("only_file.json"));
        assert!(!bloom.might_have_ext("rs"));
    }
}
