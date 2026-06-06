mod common;
use common::{run, run_json_forced};

#[test]
fn px_outputs_valid_json() {
    let out = run_json_forced("px", &["--limit", "5"]);
    assert!(out["platform"].is_string());
    assert!(out["processes"].is_array());
    assert!(out["count"].is_number());
}

#[test]
fn px_process_has_required_fields() {
    let out = run_json_forced("px", &["--limit", "5"]);
    let procs = out["processes"].as_array().expect("processes array");
    assert!(!procs.is_empty(), "expected at least one process");
    let p = &procs[0];
    assert!(p["pid"].is_number());
    assert!(p["name"].is_string());
    assert!(p["cpu_percent"].is_number());
    assert!(p["memory_bytes"].is_u64(), "memory_bytes must be an integer");
}

#[test]
fn px_can_filter_by_self_pid() {
    let pid = std::process::id();
    let out = run_json_forced("px", &["--pid", &pid.to_string()]);
    let procs = out["processes"].as_array().expect("processes array");
    assert!(
        procs.iter().all(|p| p["pid"].as_u64() == Some(pid as u64)),
        "every returned process should match the requested pid"
    );
}

#[test]
fn px_system_summary() {
    let out = run_json_forced("px", &["--system", "--limit", "1"]);
    let sys = &out["system"];
    assert!(sys["total_memory_bytes"].is_u64());
    assert!(sys["cpu_count"].is_u64());
}

#[test]
fn px_accepts_out_table() {
    // The README advertises `px --out table`; it must not error.
    let out = run("px", &["--out", "table", "--limit", "3"]);
    assert!(out.status.success(), "px --out table should succeed");
}

#[test]
fn px_ndjson_is_line_delimited() {
    let out = run("px", &["--out", "ndjson", "--limit", "3"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("ndjson line is not valid JSON: {} ({})", line, e));
    }
}

#[test]
fn px_network_is_structured() {
    // On Linux this is an array; on other platforms a structured unavailable block.
    let out = run_json_forced("px", &["--network", "--limit", "5"]);
    let conns = &out["connections"];
    assert!(
        conns.is_array() || conns.get("reason").is_some(),
        "connections must be an array or a structured unavailable block, got: {}",
        conns
    );
}
