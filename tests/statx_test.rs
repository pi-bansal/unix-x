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
