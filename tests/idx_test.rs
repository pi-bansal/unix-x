mod common;
use common::run;
use std::fs;
use tempfile::tempdir;

fn json(out: &std::process::Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "idx failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap()
}

// Guards the clap arg collision (`-f` bound to both --filter and --files) that
// used to panic on every `idx once` / `idx query` invocation.
#[test]
fn idx_once_filters_by_extension() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "fn main(){}").unwrap();
    fs::write(dir.path().join("b.txt"), "x").unwrap();

    let v = json(&run("idx", &["once", dir.path().to_str().unwrap(), "--ext", "rs"]));
    assert_eq!(v["count"].as_u64(), Some(1));
    assert_eq!(v["entries"][0]["extension"], "rs");
}

#[test]
fn idx_once_files_and_filter_flags_coexist() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("a.rs"), "fn main(){}").unwrap();

    // --files (-f) must parse without a clap panic...
    let v = json(&run("idx", &["once", dir.path().to_str().unwrap(), "--files"]));
    assert!(v["count"].as_u64().unwrap() >= 1);

    // ...and so must --filter, which used to share -f with --files.
    let v2 = json(&run("idx", &["once", dir.path().to_str().unwrap(), "--filter", "a.rs"]));
    assert!(v2["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["path"].as_str().unwrap().ends_with("a.rs")));
}

#[test]
fn idx_once_entries_have_documented_fields() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "fn main(){}").unwrap();

    let v = json(&run("idx", &["once", dir.path().to_str().unwrap(), "--ext", "rs"]));
    let e = &v["entries"][0];
    assert!(e["path"].is_string());
    assert!(e["size"].is_u64());
    assert!(e["mtime"].is_u64(), "mtime should be an epoch integer");
    assert_eq!(e["extension"], "rs");
    assert_eq!(e["is_dir"], false);
}

#[test]
fn idx_once_sort_size_orders_largest_first() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("big.rs"), "x".repeat(500)).unwrap();
    fs::write(dir.path().join("small.rs"), "x").unwrap();

    let v = json(&run(
        "idx",
        &["once", dir.path().to_str().unwrap(), "--ext", "rs", "--sort", "size", "--files"],
    ));
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries[0]["path"].as_str().unwrap().ends_with("big.rs"));
    assert!(entries[0]["size"].as_u64() >= entries[1]["size"].as_u64());
}

#[test]
fn idx_once_size_gt_filters() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("big.rs"), "x".repeat(500)).unwrap();
    fs::write(dir.path().join("small.rs"), "x").unwrap();

    let v = json(&run(
        "idx",
        &["once", dir.path().to_str().unwrap(), "--ext", "rs", "--size-gt", "100", "--files"],
    ));
    let entries = v["entries"].as_array().unwrap();
    assert!(entries.iter().all(|e| e["size"].as_u64().unwrap() > 100));
    assert!(entries.iter().any(|e| e["path"].as_str().unwrap().ends_with("big.rs")));
}

#[test]
fn idx_once_nonexistent_extension_is_empty() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "x").unwrap();

    let v = json(&run("idx", &["once", dir.path().to_str().unwrap(), "--ext", "zzz"]));
    assert_eq!(v["count"].as_u64(), Some(0));
}
