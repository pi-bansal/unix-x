mod common;
use common::{bin, run, run_json_forced};
use std::process::Command;

/// Write two files into a temp dir and return their paths.
fn make_pair(old: &str, new: &str) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    std::fs::write(&a, old).unwrap();
    std::fs::write(&b, new).unwrap();
    (dir, a, b)
}

#[test]
fn dx_reports_changes() {
    let (_d, a, b) = make_pair("line1\nline2\nline3\n", "line1\nCHANGED\nline3\n");
    let out = run_json_forced("dx", &[a.to_str().unwrap(), b.to_str().unwrap()]);
    assert!(out["total_files"].as_u64().unwrap() >= 1);
    assert!(out["total_added"].is_number());
    assert!(out["total_removed"].is_number());
    let files = out["files"].as_array().expect("files array");
    assert!(!files.is_empty());
    assert!(files[0]["hunks"].is_array());
}

#[test]
fn dx_identical_files_have_no_diff() {
    let (_d, a, b) = make_pair("same\ncontent\n", "same\ncontent\n");
    let out = run_json_forced("dx", &[a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(out["total_files"].as_u64(), Some(0));
}

#[test]
fn dx_summary_drops_hunks() {
    let (_d, a, b) = make_pair("a\nb\n", "a\nc\n");
    let out = run_json_forced("dx", &[a.to_str().unwrap(), b.to_str().unwrap(), "--summary"]);
    let files = out["files"].as_array().expect("files array");
    for f in files {
        assert_eq!(f["hunks"].as_array().map(|h| h.len()), Some(0));
    }
}

#[test]
fn dx_ndjson_is_line_delimited() {
    let (_d, a, b) = make_pair("a\nb\n", "a\nc\n");
    let out = Command::new(bin("dx"))
        .args([a.to_str().unwrap(), b.to_str().unwrap(), "--out", "ndjson"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty());
    for line in lines {
        let v: serde_json::Value = serde_json::from_str(line).expect("valid ndjson line");
        assert!(v["old_path"].is_string());
    }
}

#[test]
fn dx_accepts_out_pretty() {
    let (_d, a, b) = make_pair("a\n", "b\n");
    let out = run("dx", &[a.to_str().unwrap(), b.to_str().unwrap(), "--out", "pretty"]);
    assert!(out.status.success());
}
