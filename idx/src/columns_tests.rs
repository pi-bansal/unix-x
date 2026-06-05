/// Tests for the columnar index data structure

#[cfg(test)]
mod tests {
    use crate::columns::{ColumnarIndex, GitStatus};

    fn make_index() -> ColumnarIndex {
        let mut idx = ColumnarIndex::new("/test".to_string());

        idx.push("/test/main.rs".to_string(),   4000, 1000, Some("rs"),   GitStatus::Modified, false);
        idx.push("/test/lib.rs".to_string(),    2000, 1100, Some("rs"),   GitStatus::Clean,    false);
        idx.push("/test/build.rs".to_string(),   500, 900,  Some("rs"),   GitStatus::Clean,    false);
        idx.push("/test/README.md".to_string(), 1500, 800,  Some("md"),   GitStatus::Untracked,false);
        idx.push("/test/Cargo.toml".to_string(), 300, 700,  Some("toml"), GitStatus::Clean,    false);
        idx.push("/test/src".to_string(),       6500, 1200, None,         GitStatus::Modified, true);

        idx
    }

    // ── push / len ────────────────────────────────────────────────────────────

    #[test]
    fn push_increments_len() {
        let idx = make_index();
        assert_eq!(idx.len, 6);
        assert_eq!(idx.paths.len(), 6);
        assert_eq!(idx.sizes.len(), 6);
        assert_eq!(idx.mtimes.len(), 6);
        assert_eq!(idx.ext_ids.len(), 6);
        assert_eq!(idx.git_status.len(), 6);
        assert_eq!(idx.dir_flags.len(), 6);
    }

    #[test]
    fn all_columns_same_length() {
        let idx = make_index();
        let n = idx.len;
        assert_eq!(idx.paths.len(),      n);
        assert_eq!(idx.sizes.len(),      n);
        assert_eq!(idx.mtimes.len(),     n);
        assert_eq!(idx.ext_ids.len(),    n);
        assert_eq!(idx.git_status.len(), n);
        assert_eq!(idx.dir_flags.len(),  n);
    }

    // ── Extension interning ───────────────────────────────────────────────────

    #[test]
    fn ext_pool_deduplicates() {
        let idx = make_index();
        // "rs" appears 3 times but should only be in pool once
        let rs_count = idx.ext_pool.iter().filter(|e| e.as_str() == "rs").count();
        assert_eq!(rs_count, 1);
    }

    #[test]
    fn ext_pool_sentinel_at_zero() {
        let idx = make_index();
        assert_eq!(idx.ext_pool[0], "");
    }

    #[test]
    fn no_ext_gets_id_zero() {
        let idx = make_index();
        // src dir has no extension — should have ext_id 0
        let src_pos = idx.paths.iter().position(|p| p.ends_with("/src")).unwrap();
        assert_eq!(idx.ext_ids[src_pos], 0);
    }

    #[test]
    fn ext_for_roundtrip() {
        let idx = make_index();
        let rs_pos = idx.paths.iter().position(|p| p.ends_with("main.rs")).unwrap();
        let ext_id = idx.ext_ids[rs_pos];
        assert_eq!(idx.ext_for(ext_id), "rs");
    }

    // ── Columnar queries ──────────────────────────────────────────────────────

    #[test]
    fn query_by_ext_rs() {
        let idx = make_index();
        let matches = idx.query_by_ext("rs");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn query_by_ext_md() {
        let idx = make_index();
        let matches = idx.query_by_ext("md");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn query_by_ext_nonexistent() {
        let idx = make_index();
        let matches = idx.query_by_ext("py");
        assert!(matches.is_empty());
    }

    #[test]
    fn query_by_size_gt() {
        let idx = make_index();
        let matches = idx.query_by_size_gt(1000);
        // main.rs(4000), lib.rs(2000), README.md(1500), src(6500) = 4 entries
        assert_eq!(matches.len(), 4);
    }

    #[test]
    fn query_by_size_gt_none() {
        let idx = make_index();
        let matches = idx.query_by_size_gt(99999);
        assert!(matches.is_empty());
    }

    #[test]
    fn query_by_mtime_gt() {
        let idx = make_index();
        let matches = idx.query_by_mtime_gt(1000);
        // lib.rs(1100), src(1200) → 2
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn query_by_git_status_modified() {
        let idx = make_index();
        let matches = idx.query_by_git_status(GitStatus::Modified);
        assert_eq!(matches.len(), 2); // main.rs + src
    }

    #[test]
    fn query_by_git_status_clean() {
        let idx = make_index();
        let matches = idx.query_by_git_status(GitStatus::Clean);
        assert_eq!(matches.len(), 3); // lib.rs, build.rs, Cargo.toml
    }

    #[test]
    fn query_dirs_only() {
        let idx = make_index();
        let matches = idx.query_dirs_only();
        assert_eq!(matches.len(), 1); // src
    }

    // ── Intersection ──────────────────────────────────────────────────────────

    #[test]
    fn intersect_two_sets() {
        let idx = make_index();
        let rs_rows    = idx.query_by_ext("rs");
        let modified   = idx.query_by_git_status(GitStatus::Modified);
        let result     = ColumnarIndex::intersect(&[rs_rows, modified]);
        // Only main.rs is both .rs AND modified
        assert_eq!(result.len(), 1);
        assert!(idx.paths[result[0]].ends_with("main.rs"));
    }

    #[test]
    fn intersect_empty_set_returns_empty() {
        let idx = make_index();
        let rs_rows = idx.query_by_ext("rs");
        let py_rows = idx.query_by_ext("py"); // empty
        let result  = ColumnarIndex::intersect(&[rs_rows, py_rows]);
        assert!(result.is_empty());
    }

    #[test]
    fn intersect_single_set_returns_same() {
        let idx = make_index();
        let rs_rows = idx.query_by_ext("rs");
        let result  = ColumnarIndex::intersect(&[rs_rows.clone()]);
        assert_eq!(result, rs_rows);
    }

    #[test]
    fn intersect_empty_input() {
        let result = ColumnarIndex::intersect(&[]);
        assert!(result.is_empty());
    }

    // ── Projection ────────────────────────────────────────────────────────────

    #[test]
    fn project_returns_correct_entries() {
        let idx = make_index();
        let rs_rows = idx.query_by_ext("rs");
        let entries = idx.project(&rs_rows);
        assert_eq!(entries.len(), 3);
        for e in &entries {
            assert_eq!(e.extension.as_deref(), Some("rs"));
        }
    }

    #[test]
    fn project_empty_indices() {
        let idx = make_index();
        let entries = idx.project(&[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn project_preserves_size() {
        let idx = make_index();
        let all = idx.all_indices();
        let entries = idx.project(&all);
        let main = entries.iter().find(|e| e.path.ends_with("main.rs")).unwrap();
        assert_eq!(main.size, 4000);
    }

    // ── Serialization ─────────────────────────────────────────────────────────

    #[test]
    fn serialize_deserialize_roundtrip() {
        let idx = make_index();
        let bytes = idx.to_bytes();
        assert!(!bytes.is_empty());

        let idx2 = ColumnarIndex::from_bytes(&bytes).unwrap();
        assert_eq!(idx2.len, 6);
        assert_eq!(idx2.paths, idx.paths);
        assert_eq!(idx2.sizes, idx.sizes);
        assert_eq!(idx2.ext_pool, idx.ext_pool);
    }

    // ── GitStatus ─────────────────────────────────────────────────────────────

    #[test]
    fn git_status_roundtrip() {
        for status in [
            GitStatus::Clean,
            GitStatus::Modified,
            GitStatus::Added,
            GitStatus::Deleted,
            GitStatus::Renamed,
            GitStatus::Untracked,
            GitStatus::Ignored,
            GitStatus::Unknown,
        ] {
            let byte = status as u8;
            let back = GitStatus::from_u8(byte);
            assert_eq!(back as u8, byte);
        }
    }

    #[test]
    fn git_status_as_str() {
        assert_eq!(GitStatus::Clean.as_str(),     "clean");
        assert_eq!(GitStatus::Modified.as_str(),  "modified");
        assert_eq!(GitStatus::Untracked.as_str(), "untracked");
        assert_eq!(GitStatus::Unknown.as_str(),   "unknown");
    }
}
