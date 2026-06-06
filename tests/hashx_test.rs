mod common;
use common::run;
use std::fs;
use tempfile::tempdir;

fn write_temp(dir: &std::path::Path, name: &str, content: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn hashx_outputs_valid_json() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "file.txt", b"hello world");
    let out = run("hashx", &[f.to_str().unwrap(), "--out", "json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    assert!(v["files"].is_array());
    assert!(v["count"].is_number());
}

#[test]
fn hashx_default_computes_sha256_and_blake3() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "file.txt", b"test content");
    let out = run("hashx", &[f.to_str().unwrap(), "--out", "json"]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    let file = &v["files"][0];
    assert!(file["sha256"].is_string(), "sha256 should be present by default");
    assert!(file["blake3"].is_string(), "blake3 should be present by default");
}

#[test]
fn hashx_sha256_is_correct() {
    let dir = tempdir().unwrap();
    // SHA256 of empty string is known
    let f = write_temp(dir.path(), "empty.txt", b"");
    let out = run("hashx", &[
        f.to_str().unwrap(),
        "--algos", "sha256",
        "--out", "json",
    ]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    let sha256 = v["files"][0]["sha256"].as_str().unwrap();
    assert_eq!(
        sha256,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn hashx_blake3_is_correct() {
    let dir = tempdir().unwrap();
    // BLAKE3 of empty string
    let f = write_temp(dir.path(), "empty.txt", b"");
    let out = run("hashx", &[
        f.to_str().unwrap(),
        "--algos", "blake3",
        "--out", "json",
    ]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    let b3 = v["files"][0]["blake3"].as_str().unwrap();
    // Official BLAKE3 test vector for empty input.
    assert_eq!(
        b3,
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
    );
}

#[test]
fn hashx_md5_only_when_requested() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "file.txt", b"data");
    let out = run("hashx", &[
        f.to_str().unwrap(),
        "--algos", "md5",
        "--out", "json",
    ]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    let file = &v["files"][0];
    assert!(file["md5"].is_string());
    assert!(file["sha256"].is_null() || file.get("sha256").is_none());
    assert!(file["blake3"].is_null() || file.get("blake3").is_none());
}

#[test]
fn hashx_compare_identical_files() {
    let dir = tempdir().unwrap();
    let a = write_temp(dir.path(), "a.bin", b"same content");
    let b = write_temp(dir.path(), "b.bin", b"same content");
    let out = run("hashx", &[
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--compare", "--out", "json",
    ]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    assert_eq!(v["equal"].as_bool().unwrap(), true);
}

#[test]
fn hashx_compare_different_files() {
    let dir = tempdir().unwrap();
    let a = write_temp(dir.path(), "a.bin", b"content A");
    let b = write_temp(dir.path(), "b.bin", b"content B");
    let out = run("hashx", &[
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--compare", "--out", "json",
    ]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    assert_eq!(v["equal"].as_bool().unwrap(), false);
}

#[test]
fn hashx_verify_correct_hash_exits_zero() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "file.txt", b"");
    let out = run("hashx", &[
        f.to_str().unwrap(),
        "--algos", "sha256",
        "--verify", "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "--out", "json",
    ]);
    assert!(out.status.success(), "correct verify should exit 0");
}

#[test]
fn hashx_verify_wrong_hash_exits_nonzero() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "file.txt", b"actual content");
    let out = run("hashx", &[
        f.to_str().unwrap(),
        "--algos", "sha256",
        "--verify", "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "--out", "json",
    ]);
    assert!(!out.status.success(), "wrong verify should exit non-zero");
}

#[test]
fn hashx_multiple_files_parallel() {
    let dir = tempdir().unwrap();
    let files: Vec<_> = (0..10)
        .map(|i| write_temp(dir.path(), &format!("f{}.txt", i), format!("content {}", i).as_bytes()))
        .collect();

    let mut args: Vec<&str> = files.iter().map(|f| f.to_str().unwrap()).collect();
    args.extend(&["--out", "json"]);

    let out = run("hashx", &args);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    assert_eq!(v["count"].as_u64().unwrap(), 10);
    assert_eq!(v["files"].as_array().unwrap().len(), 10);
}

#[test]
fn hashx_size_bytes_is_correct() {
    let dir = tempdir().unwrap();
    let content = b"exactly twenty bytes";
    let f = write_temp(dir.path(), "sized.txt", content);
    let out = run("hashx", &[f.to_str().unwrap(), "--out", "json"]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    assert_eq!(v["files"][0]["size_bytes"].as_u64().unwrap(), content.len() as u64);
}

#[test]
fn hashx_missing_file_has_error_field() {
    let out = run("hashx", &["/nonexistent/file.txt", "--out", "json"]);
    let v: serde_json::Value = serde_json::from_str(
        &String::from_utf8_lossy(&out.stdout)
    ).unwrap();
    let file = &v["files"][0];
    assert!(file["error"].is_string(), "missing file should have error field");
}
