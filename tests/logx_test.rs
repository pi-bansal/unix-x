mod common;
use common::run;
use std::fs;
use tempfile::tempdir;

fn parse(out: &std::process::Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "logx failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap()
}

#[test]
fn logx_line_numbers_are_file_absolute() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("app.log");
    fs::write(&path, "a\nb\nc\nd\ne\n").unwrap();

    let v = parse(&run("logx", &[path.to_str().unwrap(), "--out", "json"]));
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.first().unwrap()["line"].as_u64(), Some(1));
    assert_eq!(entries.last().unwrap()["line"].as_u64(), Some(5));
}

#[test]
fn logx_tail_keeps_absolute_line_numbers() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("app.log");
    fs::write(&path, "a\nb\nc\nd\ne\n").unwrap();

    // --tail 2 returns the last two lines, numbered 4 and 5 (not 1 and 2).
    let v = parse(&run("logx", &[path.to_str().unwrap(), "--tail", "2", "--out", "json"]));
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["line"].as_u64(), Some(4));
    assert_eq!(entries[1]["line"].as_u64(), Some(5));
}
