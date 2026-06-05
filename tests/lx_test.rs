mod common;
use common::{run, run_json_forced, temp_tree};

#[test]
fn lx_outputs_valid_json() {
    let dir = temp_tree();
    let out = run_json_forced("lx", &[dir.path().to_str().unwrap(), "--no-git"]);
    assert!(out["entries"].is_array());
    assert!(out["count"].is_number());
    assert!(out["root"].is_string());
}

#[test]
fn lx_count_matches_entries_length() {
    let dir = temp_tree();
    let out = run_json_forced("lx", &[dir.path().to_str().unwrap(), "--no-git"]);
    let count   = out["count"].as_u64().unwrap();
    let entries = out["entries"].as_array().unwrap();
    assert_eq!(count as usize, entries.len());
}

#[test]
fn lx_entry_has_required_fields() {
    let dir = temp_tree();
    let out = run_json_forced("lx", &[dir.path().to_str().unwrap(), "--no-git"]);
    let entries = out["entries"].as_array().unwrap();
    assert!(!entries.is_empty());

    for entry in entries {
        assert!(entry["name"].is_string(),   "missing name");
        assert!(entry["path"].is_string(),   "missing path");
        assert!(entry["type"].is_string(),   "missing type");
        assert!(entry["size"].is_number(),   "missing size");
        assert!(entry["modified"].is_number(), "missing modified");
    }
}

#[test]
fn lx_ext_filter_only_returns_matching() {
    let dir = temp_tree();
    let out = run_json_forced("lx", &[
        dir.path().to_str().unwrap(),
        "--no-git", "--ext", "rs",
    ]);
    let entries = out["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    for entry in entries {
        assert_eq!(entry["extension"].as_str().unwrap(), "rs");
    }
}

#[test]
fn lx_ext_filter_nonexistent_returns_empty() {
    let dir = temp_tree();
    let out = run_json_forced("lx", &[
        dir.path().to_str().unwrap(),
        "--no-git", "--ext", "py",
    ]);
    assert_eq!(out["count"].as_u64().unwrap(), 0);
}

#[test]
fn lx_files_only_has_no_dirs() {
    let dir = temp_tree();
    let out = run_json_forced("lx", &[
        dir.path().to_str().unwrap(),
        "--no-git", "--files-only",
    ]);
    let entries = out["entries"].as_array().unwrap();
    for entry in entries {
        assert_ne!(entry["type"].as_str().unwrap(), "dir", "files-only returned a dir");
    }
}

#[test]
fn lx_timestamps_are_integers() {
    let dir = temp_tree();
    let out = run_json_forced("lx", &[dir.path().to_str().unwrap(), "--no-git"]);
    let entries = out["entries"].as_array().unwrap();
    for entry in entries {
        let mtime = &entry["modified"];
        assert!(mtime.is_number(), "modified should be a number");
        // Must not be a string timestamp
        assert!(!mtime.is_string(), "modified must not be a string");
    }
}

#[test]
fn lx_depth_limits_traversal() {
    let dir = temp_tree();
    let shallow = run_json_forced("lx", &[
        dir.path().to_str().unwrap(),
        "--no-git", "--depth", "1",
    ]);
    let deep = run_json_forced("lx", &[
        dir.path().to_str().unwrap(),
        "--no-git", "--depth", "3",
    ]);
    let shallow_count = shallow["count"].as_u64().unwrap();
    let deep_count    = deep["count"].as_u64().unwrap();
    // Deep should find at least as many entries as shallow
    assert!(deep_count >= shallow_count);
}

#[test]
fn lx_ndjson_emits_one_entry_per_line() {
    let dir = temp_tree();
    let output = run("lx", &[
        dir.path().to_str().unwrap(),
        "--no-git", "--out", "ndjson",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|_| panic!("invalid ndjson line: {}", line));
        assert!(v["name"].is_string());
    }
}

#[test]
fn lx_nonexistent_path_exits_nonzero() {
    let out = run("lx", &["/this/path/does/not/exist/xyz123"]);
    assert!(!out.status.success());
    // Error should be JSON on stderr
    let stderr = String::from_utf8_lossy(&out.stderr);
    let err: serde_json::Value = serde_json::from_str(&stderr)
        .unwrap_or_else(|_| panic!("stderr not valid JSON: {}", stderr));
    assert!(err["error"].is_string());
}
