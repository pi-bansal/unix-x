mod common;
use common::run;
use std::fs;
use tempfile::tempdir;

fn write_temp(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn diffx_clean_merge_exits_zero() {
    let dir = tempdir().unwrap();
    let base   = write_temp(dir.path(), "base.txt",   "A\nB\nC\n");
    let ours   = write_temp(dir.path(), "ours.txt",   "A\nOURS\nC\n");
    let theirs = write_temp(dir.path(), "theirs.txt", "A\nB\nC\n");

    let out = run("diffx", &[
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
        "--out", "json",
    ]);
    assert!(out.status.success(), "clean merge should exit 0");
}

#[test]
fn diffx_conflict_exits_nonzero() {
    let dir = tempdir().unwrap();
    let base   = write_temp(dir.path(), "base.txt",   "X\n");
    let ours   = write_temp(dir.path(), "ours.txt",   "Y\n");
    let theirs = write_temp(dir.path(), "theirs.txt", "Z\n");

    let out = run("diffx", &[
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
        "--out", "json",
    ]);
    assert!(!out.status.success(), "conflict should exit non-zero");
}

#[test]
fn diffx_outputs_valid_json() {
    let dir = tempdir().unwrap();
    let base   = write_temp(dir.path(), "base.txt",   "A\nB\n");
    let ours   = write_temp(dir.path(), "ours.txt",   "A\nB\n");
    let theirs = write_temp(dir.path(), "theirs.txt", "A\nB\n");

    let out = run("diffx", &[
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
        "--out", "json",
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .expect("diffx output should be valid JSON");

    assert!(v["clean"].is_boolean());
    assert!(v["conflict_count"].is_number());
    assert!(v["auto_resolved_count"].is_number());
    assert!(v["hunks"].is_array());
}

#[test]
fn diffx_auto_resolves_identical_changes() {
    let dir = tempdir().unwrap();
    let base   = write_temp(dir.path(), "base.txt",   "line1\noriginal\nline3\n");
    let ours   = write_temp(dir.path(), "ours.txt",   "line1\nSAME_CHANGE\nline3\n");
    let theirs = write_temp(dir.path(), "theirs.txt", "line1\nSAME_CHANGE\nline3\n");

    let out = run("diffx", &[
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
        "--out", "json",
    ]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();

    assert_eq!(v["clean"].as_bool().unwrap(), true);
    assert_eq!(v["conflict_count"].as_u64().unwrap(), 0);
    assert!(v["auto_resolved_count"].as_u64().unwrap() > 0);
}

#[test]
fn diffx_resolved_field_absent_when_conflicts() {
    let dir = tempdir().unwrap();
    let base   = write_temp(dir.path(), "base.txt",   "shared\n");
    let ours   = write_temp(dir.path(), "ours.txt",   "ours\n");
    let theirs = write_temp(dir.path(), "theirs.txt", "theirs\n");

    let out = run("diffx", &[
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
        "--out", "json",
    ]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();

    assert_eq!(v["clean"].as_bool().unwrap(), false);
    // resolved should be null/absent when there are conflicts
    assert!(v["resolved"].is_null() || v.get("resolved").is_none());
}

#[test]
fn diffx_writes_output_file_on_clean_merge() {
    let dir = tempdir().unwrap();
    let base   = write_temp(dir.path(), "base.txt",   "A\nB\nC\n");
    let ours   = write_temp(dir.path(), "ours.txt",   "A\nNEW\nC\n");
    let theirs = write_temp(dir.path(), "theirs.txt", "A\nB\nC\n");
    let output = dir.path().join("merged.txt");

    let out = run("diffx", &[
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
        "--output", output.to_str().unwrap(),
        "--out", "json",
    ]);
    assert!(out.status.success());
    assert!(output.exists(), "output file should be written");
    let content = fs::read_to_string(&output).unwrap();
    assert!(content.contains("NEW"));
}

#[test]
fn diffx_summary_flag_omits_hunks() {
    let dir = tempdir().unwrap();
    let base   = write_temp(dir.path(), "base.txt",   "A\nB\n");
    let ours   = write_temp(dir.path(), "ours.txt",   "A\nX\n");
    let theirs = write_temp(dir.path(), "theirs.txt", "A\nB\n");

    let out = run("diffx", &[
        base.to_str().unwrap(),
        ours.to_str().unwrap(),
        theirs.to_str().unwrap(),
        "--summary", "--out", "json",
    ]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();

    // With --summary, hunks should be empty or absent
    let hunks = v["hunks"].as_array().map(|a| a.len()).unwrap_or(0);
    assert_eq!(hunks, 0, "--summary should produce no hunks");
}
