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
