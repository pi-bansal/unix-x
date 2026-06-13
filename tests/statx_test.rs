mod common;
use common::{run, run_json_forced};

#[test]
fn statx_now_snapshot() {
    // `statx now` takes a single snapshot — no daemon required.
    let out = run_json_forced("statx", &["now"]);
    assert!(out["ts"].is_u64(), "ts must be unix epoch integer seconds");
    assert!(out["mem_total"].is_u64());
    assert!(out["cpu_total"].is_number());
}

#[test]
fn statx_now_accepts_pretty() {
    let out = run("statx", &["now", "--out", "pretty"]);
    assert!(out.status.success());
}

#[test]
fn statx_now_has_documented_fields() {
    let out = run_json_forced("statx", &["now"]);
    // Integer counters/byte-gauges.
    for field in [
        "mem_total",
        "mem_used",
        "mem_free",
        "mem_available",
        "disk_read_bps",
        "disk_write_bps",
        "net_rx_bps",
        "net_tx_bps",
        "procs_running",
        "procs_total",
    ] {
        assert!(out[field].is_u64(), "{field} should be an integer");
    }
    // Floating-point load averages and CPU breakdown.
    for field in ["cpu_total", "cpu_user", "cpu_system", "load_1m", "load_5m", "load_15m"] {
        assert!(out[field].is_number(), "{field} should be numeric");
    }
    assert!(out["cpu_cores"].is_array(), "cpu_cores should be a per-core array");
}

#[test]
fn statx_last_returns_samples_with_count() {
    // With no daemon running, `last N` collects N live samples on the fly.
    let out = run_json_forced("statx", &["last", "1"]);
    assert_eq!(out["count"].as_u64(), Some(1));
    let samples = out["samples"].as_array().expect("samples array");
    assert_eq!(samples.len(), 1);
    assert!(samples[0]["ts"].is_u64(), "each sample carries an epoch ts");
}
