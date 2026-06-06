/// Integration test helpers
/// These tests run the compiled binaries and assert on their JSON output.

use serde_json::Value;
use std::process::{Command, Output};
use std::path::PathBuf;

/// Path to a compiled binary in target/debug or target/release
pub fn bin(name: &str) -> PathBuf {
    // Try debug first (faster to build), then release
    let debug   = PathBuf::from(format!("../target/debug/{}", name));
    let release = PathBuf::from(format!("../target/release/{}", name));
    if debug.exists() { debug } else { release }
}

/// Run a binary with args, return stdout parsed as JSON
pub fn run_json(name: &str, args: &[&str]) -> Value {
    let out = run(name, args);
    assert!(
        out.status.success(),
        "{} failed with stderr: {}",
        name,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("failed to parse {} output as JSON: {}\noutput: {}", name, e, stdout))
}

/// Run a binary and return raw Output
pub fn run(name: &str, args: &[&str]) -> Output {
    Command::new(bin(name))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {} — did you run `cargo build --workspace`?", name, e))
}

/// Run a binary with its working directory set to `dir` (for tools that read
/// files relative to the current directory, e.g. envx scanning `.env`).
#[allow(dead_code)]
pub fn run_in_dir(dir: &std::path::Path, name: &str, args: &[&str]) -> Output {
    Command::new(bin(name))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {}", name, e))
}

/// Run a binary in `dir` and parse stdout as JSON, asserting success.
#[allow(dead_code)]
pub fn run_json_in_dir(dir: &std::path::Path, name: &str, args: &[&str]) -> Value {
    let out = run_in_dir(dir, name, args);
    assert!(
        out.status.success(),
        "{} failed with stderr: {}",
        name,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("failed to parse {} output as JSON: {}\noutput: {}", name, e, stdout))
}

/// Run with --out json to force compact regardless of TTY
pub fn run_json_forced(name: &str, args: &[&str]) -> Value {
    let mut full_args: Vec<&str> = args.to_vec();
    full_args.extend(&["--out", "json"]);
    run_json(name, &full_args)
}

/// Create a temp directory with some files for testing
pub fn temp_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"),   "fn main() {}").unwrap();
    std::fs::write(root.join("src/lib.rs"),    "pub fn foo() {}").unwrap();
    std::fs::write(root.join("README.md"),     "# Test").unwrap();
    std::fs::write(root.join("Cargo.toml"),    "[package]\nname=\"test\"").unwrap();
    std::fs::write(root.join("data.json"),     r#"{"key": "value"}"#).unwrap();

    dir
}
