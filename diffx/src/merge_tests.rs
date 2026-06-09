/// Tests for the three-way merge engine

#[cfg(test)]
mod tests {
    use crate::merge::{three_way_merge, HunkKind};

    // ── Clean merges ──────────────────────────────────────────────────────────

    #[test]
    fn identical_files_is_clean() {
        let base  = "fn foo() { 1 }\nfn bar() { 2 }\n";
        let result = three_way_merge(base, base, base);
        assert!(result.clean);
        assert_eq!(result.conflict_count, 0);
    }

    #[test]
    fn preserves_trailing_newline() {
        // Inputs end with a newline — a clean merge must round-trip it.
        let result = three_way_merge("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\n");
        assert!(result.clean);
        let resolved = result.resolved.unwrap();
        assert_eq!(resolved, "a\nB\nc\n", "trailing newline should be preserved");
    }

    #[test]
    fn no_trailing_newline_when_inputs_lack_one() {
        let result = three_way_merge("a\nb\nc", "a\nB\nc", "a\nb\nc");
        assert!(result.clean);
        let resolved = result.resolved.unwrap();
        assert_eq!(resolved, "a\nB\nc", "should not invent a trailing newline");
    }

    #[test]
    fn ours_only_change_is_clean() {
        let base   = "line1\nline2\nline3\n";
        let ours   = "line1\nMODIFIED\nline3\n";
        let theirs = "line1\nline2\nline3\n";

        let result = three_way_merge(base, ours, theirs);
        assert!(result.clean);
        assert_eq!(result.conflict_count, 0);
        assert_eq!(result.ours_only_count, 1);
        assert_eq!(result.theirs_only_count, 0);
    }

    #[test]
    fn theirs_only_change_is_clean() {
        let base   = "line1\nline2\nline3\n";
        let ours   = "line1\nline2\nline3\n";
        let theirs = "line1\nTHEIRS\nline3\n";

        let result = three_way_merge(base, ours, theirs);
        assert!(result.clean);
        assert_eq!(result.conflict_count, 0);
        assert_eq!(result.theirs_only_count, 1);
    }

    #[test]
    fn both_same_change_is_auto_resolved() {
        let base   = "line1\nline2\nline3\n";
        let ours   = "line1\nSAME\nline3\n";
        let theirs = "line1\nSAME\nline3\n";

        let result = three_way_merge(base, ours, theirs);
        assert!(result.clean);
        assert_eq!(result.conflict_count, 0);
        assert_eq!(result.auto_resolved_count, 1);

        // Should have a BothSame hunk
        let both_same = result.hunks.iter()
            .filter(|h| h.kind == HunkKind::BothSame)
            .count();
        assert_eq!(both_same, 1);
    }

    #[test]
    fn non_overlapping_changes_both_accepted() {
        let base   = "A\nB\nC\nD\n";
        let ours   = "OURS_A\nB\nC\nD\n";
        let theirs = "A\nB\nC\nTHEIRS_D\n";

        let result = three_way_merge(base, ours, theirs);
        assert!(result.clean);
        assert_eq!(result.conflict_count, 0);
        assert_eq!(result.ours_only_count, 1);
        assert_eq!(result.theirs_only_count, 1);

        let resolved = result.resolved.unwrap();
        assert!(resolved.contains("OURS_A"));
        assert!(resolved.contains("THEIRS_D"));
    }

    #[test]
    fn resolved_text_contains_all_clean_changes() {
        let base   = "start\nmiddle\nend\n";
        let ours   = "START\nmiddle\nend\n";
        let theirs = "start\nmiddle\nEND\n";

        let result = three_way_merge(base, ours, theirs);
        assert!(result.clean);
        let resolved = result.resolved.unwrap();
        assert!(resolved.contains("START"));
        assert!(resolved.contains("END"));
        assert!(resolved.contains("middle"));
    }

    // ── Conflicts ─────────────────────────────────────────────────────────────

    #[test]
    fn overlapping_changes_produce_conflict() {
        let base   = "line1\nshared_line\nline3\n";
        let ours   = "line1\nOURS_VERSION\nline3\n";
        let theirs = "line1\nTHEIRS_VERSION\nline3\n";

        let result = three_way_merge(base, ours, theirs);
        assert!(!result.clean);
        assert_eq!(result.conflict_count, 1);
        assert!(result.resolved.is_none());
    }

    #[test]
    fn conflict_hunk_has_all_three_versions() {
        let base   = "A\nCONFLICT\nZ\n";
        let ours   = "A\nOURS\nZ\n";
        let theirs = "A\nTHEIRS\nZ\n";

        let result = three_way_merge(base, ours, theirs);
        let conflict = result.hunks.iter()
            .find(|h| h.kind == HunkKind::Conflict)
            .expect("expected a conflict hunk");

        assert!(!conflict.base_lines.is_empty());
        assert!(!conflict.ours_lines.is_empty());
        assert!(!conflict.theirs_lines.is_empty());
        assert!(conflict.resolved_lines.is_none());
        assert!(conflict.resolution_hint.is_some());
    }

    #[test]
    fn multiple_conflicts_counted() {
        let base   = "A\nB\nC\nD\nE\n";
        let ours   = "A1\nB\nC1\nD\nE\n";
        let theirs = "A2\nB\nC2\nD\nE\n";

        let result = three_way_merge(base, ours, theirs);
        assert!(!result.clean);
        assert_eq!(result.conflict_count, 2);
    }

    // ── Hunk structure ────────────────────────────────────────────────────────

    #[test]
    fn unchanged_lines_produce_unchanged_hunks() {
        let base   = "A\nB\nC\n";
        let result = three_way_merge(base, base, base);

        let unchanged_count = result.hunks.iter()
            .filter(|h| h.kind == HunkKind::Unchanged)
            .count();
        assert!(unchanged_count > 0);
    }

    #[test]
    fn hunk_start_lines_are_one_indexed() {
        let base   = "line1\nline2\nline3\n";
        let ours   = "line1\nMODIFIED\nline3\n";
        let result = three_way_merge(base, ours, base);

        for hunk in &result.hunks {
            assert!(hunk.start_line >= 1, "start_line must be 1-indexed");
        }
    }

    #[test]
    fn clean_hunks_have_resolved_lines() {
        let base   = "A\nB\nC\n";
        let ours   = "A\nOURS\nC\n";
        let result = three_way_merge(base, ours, base);

        for hunk in &result.hunks {
            if hunk.kind != HunkKind::Conflict {
                assert!(
                    hunk.resolved_lines.is_some(),
                    "non-conflict hunk {:?} should have resolved_lines", hunk.kind
                );
            }
        }
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn empty_base_ours_adds_lines() {
        let base   = "";
        let ours   = "new line\n";
        let theirs = "";
        let result = three_way_merge(base, ours, theirs);
        assert!(result.clean);
    }

    #[test]
    fn all_empty_is_clean() {
        let result = three_way_merge("", "", "");
        assert!(result.clean);
        assert_eq!(result.conflict_count, 0);
    }

    #[test]
    fn single_line_conflict() {
        let result = three_way_merge("X\n", "Y\n", "Z\n");
        assert!(!result.clean);
        assert_eq!(result.conflict_count, 1);
    }

    #[test]
    fn resolution_hint_set_on_conflict() {
        let result = three_way_merge("base\n", "ours\n", "theirs\n");
        let conflict = result.hunks.iter()
            .find(|h| h.kind == HunkKind::Conflict)
            .unwrap();
        assert_eq!(
            conflict.resolution_hint.as_deref(),
            Some("both_modified_same_region")
        );
    }
}
