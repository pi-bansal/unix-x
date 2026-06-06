mod common;
use common::{bin, run, run_json};
use std::process::Command;

/// Build a .tar.gz in a temp dir using the system `tar` and return its path.
/// Returns None if `tar` is unavailable (test is skipped in that case).
fn make_targz() -> Option<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("one.txt"), "hello").unwrap();
    std::fs::write(src.join("two.txt"), "world!!").unwrap();

    let archive = dir.path().join("bundle.tar.gz");
    let status = Command::new("tar")
        .arg("czf")
        .arg(&archive)
        .arg("-C")
        .arg(&src)
        .arg(".")
        .status()
        .ok()?;
    if !status.success() || !archive.exists() {
        return None;
    }
    Some((dir, archive))
}

#[test]
fn arcx_inspects_targz() {
    let Some((_d, archive)) = make_targz() else {
        eprintln!("skipping: system `tar` unavailable");
        return;
    };
    let out = run_json("arcx", &[archive.to_str().unwrap(), "--out", "json"]);
    assert_eq!(out["format"], "tar.gz");
    assert!(out["total_entries"].as_u64().unwrap() >= 2);
    assert!(out["total_size_uncompressed"].is_u64());
    let entries = out["entries"].as_array().expect("entries array");
    let e = &entries[0];
    assert!(e["path"].is_string());
    assert!(e["type"].is_string());
    assert!(e["size_uncompressed"].is_u64());
}

#[test]
fn arcx_summary_drops_entries() {
    let Some((_d, archive)) = make_targz() else { return; };
    let out = run_json("arcx", &[archive.to_str().unwrap(), "--summary", "--out", "json"]);
    assert_eq!(out["entries"].as_array().map(|e| e.len()), Some(0));
    assert!(out["total_entries"].as_u64().unwrap() >= 2);
}

#[test]
fn arcx_missing_file_errors() {
    let out = run("arcx", &["/no/such/archive.tar.gz", "--out", "json"]);
    assert!(!out.status.success(), "missing archive should exit non-zero");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error"), "stderr should carry a structured error");
}
