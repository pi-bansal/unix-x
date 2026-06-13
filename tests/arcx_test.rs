mod common;
use common::{run, run_json};
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

#[test]
fn arcx_reports_compression_ratio() {
    let Some((_d, archive)) = make_targz() else { return; };
    let out = run_json("arcx", &[archive.to_str().unwrap(), "--out", "json"]);
    assert!(out["compression_ratio"].is_number());
    assert!(out["total_size_compressed"].is_u64());
}

#[test]
fn arcx_sort_size_orders_largest_first() {
    let Some((_d, archive)) = make_targz() else { return; };
    // one.txt = "hello" (5 bytes), two.txt = "world!!" (7 bytes).
    let out = run_json(
        "arcx",
        &[archive.to_str().unwrap(), "--sort", "size", "--files-only", "--out", "json"],
    );
    let entries = out["entries"].as_array().unwrap();
    assert!(entries.len() >= 2);
    let sizes: Vec<u64> = entries
        .iter()
        .map(|e| e["size_uncompressed"].as_u64().unwrap())
        .collect();
    assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "sizes should be descending: {sizes:?}");
}

#[test]
fn arcx_filter_restricts_entries() {
    let Some((_d, archive)) = make_targz() else { return; };
    let out = run_json(
        "arcx",
        &[archive.to_str().unwrap(), "--filter", "one", "--out", "json"],
    );
    let entries = out["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    assert!(entries.iter().all(|e| e["path"].as_str().unwrap().contains("one")));
}

#[test]
fn arcx_files_only_excludes_directories() {
    let Some((_d, archive)) = make_targz() else { return; };
    let out = run_json(
        "arcx",
        &[archive.to_str().unwrap(), "--files-only", "--out", "json"],
    );
    let entries = out["entries"].as_array().unwrap();
    assert!(entries.iter().all(|e| e["type"] != "dir"));
}

#[test]
fn arcx_ndjson_is_line_delimited() {
    let Some((_d, archive)) = make_targz() else { return; };
    let out = run("arcx", &[archive.to_str().unwrap(), "--files-only", "--out", "ndjson"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(lines.len() >= 2, "expected one object per file entry");
    for line in lines {
        let v: serde_json::Value = serde_json::from_str(line).expect("each line is valid JSON");
        assert!(v["path"].is_string());
    }
}

/// Build a .zip with the `zip` CLI (falling back to python3); returns None if
/// neither is available so the test can skip cleanly.
fn make_zip() -> Option<(tempfile::TempDir, std::path::PathBuf)> {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("bundle.zip");
    let script = format!(
        "import zipfile; z=zipfile.ZipFile(r'{}', 'w'); z.writestr('a.txt', 'x'*50); z.writestr('b.txt', 'y'); z.close()",
        archive.display()
    );
    let status = Command::new("python3").arg("-c").arg(&script).status().ok()?;
    if !status.success() || !archive.exists() {
        return None;
    }
    Some((dir, archive))
}

#[test]
fn arcx_inspects_zip() {
    let Some((_d, archive)) = make_zip() else {
        eprintln!("skipping: python3 unavailable for zip fixture");
        return;
    };
    let out = run_json("arcx", &[archive.to_str().unwrap(), "--out", "json"]);
    assert_eq!(out["format"], "zip");
    assert!(out["total_entries"].as_u64().unwrap() >= 2);
}
