mod common;
use common::{bin, run};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

fn parse(out: &std::process::Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "logx failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap()
}

/// Run logx with the given args, feeding `input` on stdin, and parse the JSON.
fn parse_stdin(input: &str, args: &[&str]) -> serde_json::Value {
    let mut child = Command::new(bin("logx"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn logx");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
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

#[test]
fn logx_reads_from_stdin() {
    let v = parse_stdin(
        "level=info msg=one\nlevel=info msg=two\n",
        &["--out", "json"],
    );
    assert_eq!(v["count"].as_u64(), Some(2));
}

#[test]
fn logx_detects_json_format_and_normalizes_message() {
    let v = parse_stdin(
        "{\"level\":\"error\",\"msg\":\"boom\"}\n{\"level\":\"info\",\"msg\":\"ok\"}\n",
        &["--out", "json"],
    );
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["level"], "error");
    assert_eq!(entries[0]["message"], "boom");
}

#[test]
fn logx_detects_logfmt_in_stats() {
    let v = parse_stdin("level=info msg=hi\n", &["--stats"]);
    assert_eq!(v["format_detected"], "logfmt");
}

#[test]
fn logx_normalizes_levels() {
    // fatal -> error, warning -> warn, trace -> debug.
    let v = parse_stdin(
        "level=fatal msg=a\nlevel=warning msg=b\nlevel=trace msg=c\n",
        &["--stats"],
    );
    assert_eq!(v["error_count"].as_u64(), Some(1));
    assert_eq!(v["warn_count"].as_u64(), Some(1));
    assert_eq!(v["debug_count"].as_u64(), Some(1));
}

#[test]
fn logx_min_level_drops_lower_levels() {
    let v = parse_stdin(
        "level=info msg=ok\nlevel=warn msg=careful\nlevel=error msg=bad\n",
        &["--min-level", "warn", "--out", "json"],
    );
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "info should be filtered out");
    assert!(entries.iter().all(|e| e["level"] != "info"));
}

#[test]
fn logx_grep_filters_by_substring() {
    let v = parse_stdin(
        "level=info msg=alpha\nlevel=info msg=beta\n",
        &["--grep", "beta", "--out", "json"],
    );
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["message"], "beta");
}

#[test]
fn logx_regex_filters_lines() {
    let v = parse_stdin(
        "level=info msg=\"user_id=42\"\nlevel=info msg=none\n",
        &["--regex", r"user_id=\d+", "--out", "json"],
    );
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
}

#[test]
fn logx_reads_gzip_natively() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("app.log.gz");
    // Minimal gzip writer via flate2 isn't a dep here; use the system `gzip`.
    let plain = dir.path().join("app.log");
    fs::write(&plain, "level=error msg=boom\n").unwrap();
    let gz = std::process::Command::new("gzip")
        .arg("-k")
        .arg(&plain)
        .status();
    if gz.map(|s| s.success()).unwrap_or(false) && path.exists() {
        let v = parse(&run("logx", &[path.to_str().unwrap(), "--out", "json"]));
        assert_eq!(v["entries"][0]["level"], "error");
    } else {
        let _ = std::io::stderr().write_all(b"skipping: system `gzip` unavailable\n");
    }
}
