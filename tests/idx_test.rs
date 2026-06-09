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
