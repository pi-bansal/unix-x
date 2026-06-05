/// Tests for ux-output shared library

#[cfg(test)]
mod tests {
    use super::*;

    // ── OutMode ───────────────────────────────────────────────────────────────

    #[test]
    fn outmode_from_str_known() {
        assert_eq!(OutMode::from_str("json"),   OutMode::Json);
        assert_eq!(OutMode::from_str("pretty"), OutMode::Pretty);
        assert_eq!(OutMode::from_str("table"),  OutMode::Table);
        assert_eq!(OutMode::from_str("ndjson"), OutMode::Ndjson);
    }

    #[test]
    fn outmode_from_str_case_insensitive() {
        assert_eq!(OutMode::from_str("JSON"),   OutMode::Json);
        assert_eq!(OutMode::from_str("PRETTY"), OutMode::Pretty);
        assert_eq!(OutMode::from_str("NDJSON"), OutMode::Ndjson);
    }

    #[test]
    fn outmode_from_str_unknown_is_auto() {
        assert_eq!(OutMode::from_str(""),        OutMode::Auto);
        assert_eq!(OutMode::from_str("garbage"), OutMode::Auto);
        assert_eq!(OutMode::from_str("auto"),    OutMode::Auto);
    }

    #[test]
    fn outmode_json_resolves_compact() {
        assert_eq!(OutMode::Json.resolve(), ResolvedMode::Compact);
    }

    #[test]
    fn outmode_pretty_resolves_pretty() {
        assert_eq!(OutMode::Pretty.resolve(), ResolvedMode::Pretty);
    }

    // ── MaybeAvailable ────────────────────────────────────────────────────────

    #[test]
    fn maybe_available_with_items() {
        let m: MaybeAvailable<u32> = MaybeAvailable::available(vec![1, 2, 3]);
        assert_eq!(m.items.len(), 3);
        assert!(m.unavailable.is_none());
    }

    #[test]
    fn maybe_unavailable_has_empty_items() {
        let m: MaybeAvailable<u32> = MaybeAvailable::unavailable(
            "test_feature", "not supported", Some("try X instead")
        );
        assert!(m.items.is_empty());
        assert!(m.unavailable.is_some());
        let u = m.unavailable.unwrap();
        assert_eq!(u.feature, "test_feature");
        assert!(u.suggestion.is_some());
    }

    #[test]
    fn maybe_unavailable_from_preset() {
        let m: MaybeAvailable<u32> = MaybeAvailable::unavailable_from(unavail::launchd());
        assert!(m.items.is_empty());
        let u = m.unavailable.unwrap();
        assert_eq!(u.feature, "launchd_jobs");
        assert!(u.suggestion.is_some());
    }

    // ── Platform helpers ──────────────────────────────────────────────────────

    #[test]
    fn current_platform_nonempty() {
        let p = current_platform();
        assert!(!p.is_empty());
        assert!(p.contains('-'));
    }

    #[test]
    fn has_command_true_for_sh() {
        // sh should exist everywhere we test
        #[cfg(unix)]
        assert!(has_command("sh"));
    }

    #[test]
    fn has_command_false_for_nonexistent() {
        assert!(!has_command("this_command_definitely_does_not_exist_xyz123"));
    }

    // ── Unavailability presets ────────────────────────────────────────────────

    #[test]
    fn unavail_proc_net_has_suggestion() {
        let u = unavail::proc_net();
        assert!(!u.reason.is_empty());
        assert!(u.suggestion.is_some());
    }

    #[test]
    fn unavail_systemd_has_platform_specific_suggestion() {
        let u = unavail::systemd();
        assert_eq!(u.feature, "systemd_units");
        assert!(u.suggestion.is_some());
    }

    #[test]
    fn unavail_git_has_suggestion() {
        let u = unavail::git_unavailable();
        assert!(u.suggestion.is_some());
        assert!(u.reason.contains("git"));
    }
}
