mod common;
use common::run;

#[test]
fn memx_inspects_self() {
    // memx requires /proc on Linux; on other platforms it returns a structured
    // unavailable block and exits non-zero. Handle both.
    let pid = std::process::id();
    let out = run("memx", &[&pid.to_string(), "--out", "json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("memx output not JSON: {} ({})", stdout, e));

    if out.status.success() {
        assert_eq!(v["pid"].as_u64(), Some(pid as u64));
        assert!(v["region_count"].is_u64());
        assert!(v["total_rss"].is_u64(), "total_rss should be an integer byte count");
    } else {
        // Non-Linux: structured unavailable.
        assert!(v["unavailable"]["reason"].is_string());
    }
}

#[test]
fn memx_regions_listing() {
    let pid = std::process::id();
    let out = run("memx", &[&pid.to_string(), "--regions", "--out", "json"]);
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert!(v["regions"].is_array(), "--regions should include a regions array");
    }
}

#[test]
fn memx_summary_has_all_byte_totals() {
    let pid = std::process::id();
    let out = run("memx", &[&pid.to_string(), "--out", "json"]);
    if !out.status.success() {
        return; // non-Linux structured-unavailable path covered elsewhere
    }
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    // Every documented summary byte-total must be present and integer-typed.
    for field in [
        "total_vss",
        "total_rss",
        "total_pss",
        "total_private",
        "total_shared",
        "total_swap",
        "heap_bytes",
        "stack_bytes",
        "lib_bytes",
        "anon_bytes",
    ] {
        assert!(v[field].is_u64(), "{field} should be an integer byte count");
    }
    assert!(v["region_count"].is_u64());
}

#[test]
fn memx_kind_filter_only_returns_that_kind() {
    let pid = std::process::id();
    let out = run("memx", &[&pid.to_string(), "--regions", "--kind", "heap", "--out", "json"]);
    if !out.status.success() {
        return;
    }
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    let regions = v["regions"].as_array().expect("regions array");
    // Filter may legitimately return zero regions, but any present must be heap.
    assert!(regions.iter().all(|r| r["kind"] == "heap"));
}

#[test]
fn memx_nonexistent_pid_returns_structured_error() {
    let out = run("memx", &["2147483647", "--out", "json"]);
    let v: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
            .or_else(|_| serde_json::from_str(String::from_utf8_lossy(&out.stderr).trim()))
            .expect("memx should still emit JSON for a bad pid");
    assert!(
        v["error"].is_string() || v["unavailable"]["reason"].is_string(),
        "expected a structured error or unavailable block, got: {v}"
    );
}
