mod common;
use common::{run, run_json_forced};

/// Write a source file into a temp dir and return its path.
fn write_src(name: &str, body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join(name);
    std::fs::write(&p, body).unwrap();
    (dir, p)
}

#[test]
fn astx_parses_rust_ast() {
    let (_d, p) = write_src("a.rs", "fn main() { let x = 1; }");
    let out = run_json_forced("astx", &[p.to_str().unwrap()]);
    assert_eq!(out["kind"], "source_file");
    assert!(out["children"].is_array());
    assert!(out["start"]["row"].is_u64());
}

#[test]
fn astx_symbols_rust() {
    let (_d, p) = write_src("b.rs", "fn alpha() {}\nstruct Beta;\nfn gamma() {}");
    let out = run_json_forced("astx", &[p.to_str().unwrap(), "--symbols"]);
    let syms = out.as_array().expect("symbols array");
    let names: Vec<&str> = syms.iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"Beta"));
    assert!(names.contains(&"gamma"));
}

#[test]
fn astx_symbols_python() {
    let (_d, p) = write_src("c.py", "def foo():\n    pass\n\nclass Bar:\n    pass\n");
    let out = run_json_forced("astx", &[p.to_str().unwrap(), "--symbols"]);
    let names: Vec<String> = out
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str().map(String::from))
        .collect();
    assert!(names.iter().any(|n| n == "foo"));
    assert!(names.iter().any(|n| n == "Bar"));
}

#[test]
fn astx_kind_filter_flattens() {
    let (_d, p) = write_src("d.rs", "fn a() {}\nfn b() {}\nfn c() {}");
    let out = run_json_forced("astx", &[p.to_str().unwrap(), "--kind", "function_item"]);
    assert_eq!(out.as_array().map(|a| a.len()), Some(3));
}

#[test]
fn astx_lang_override() {
    // No recognizable extension, but --lang forces python.
    let (_d, p) = write_src("script", "def hello():\n    pass\n");
    let out = run_json_forced("astx", &[p.to_str().unwrap(), "--lang", "python", "--symbols"]);
    let names: Vec<&str> = out.as_array().unwrap().iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"hello"));
}

#[test]
fn astx_unsupported_extension_is_structured() {
    let (_d, p) = write_src("thing.zig", "const x = 1;");
    let out = run("astx", &[p.to_str().unwrap()]);
    assert!(!out.status.success(), "unsupported language should exit non-zero");
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("structured unavailable JSON on stdout");
    assert!(v["unavailable"]["reason"].is_string());
    assert!(v["unavailable"]["suggestion"].is_string());
}

#[test]
fn astx_missing_file_errors() {
    let out = run("astx", &["/no/such/file.rs"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error"), "stderr should carry a structured error");
}
