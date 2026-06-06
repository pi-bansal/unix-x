mod common;
use common::{run, run_json_forced};

#[test]
fn termx_outputs_capabilities() {
    let out = run_json_forced("termx", &[]);
    assert!(out["is_tty"].is_boolean());
    assert!(out["color_depth"].is_string());
    assert!(out["interactive"].is_boolean());
    assert!(out["unicode"].is_boolean());
}

#[test]
fn termx_not_a_tty_when_piped() {
    // Under `cargo test`, stdout is captured, so it is not a terminal.
    let out = run_json_forced("termx", &[]);
    assert_eq!(out["is_tty"], false);
    // interactive == is_tty && !ci, so it must be false here too.
    assert_eq!(out["interactive"], false);
}

#[test]
fn termx_table_mode_runs() {
    let out = run("termx", &["--out", "table"]);
    assert!(out.status.success());
}
