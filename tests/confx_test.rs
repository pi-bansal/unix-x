mod common;
use common::run;
use std::fs;
use tempfile::tempdir;

fn write_temp(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

fn json(out: &std::process::Output) -> serde_json::Value {
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn confx_yaml_parses_to_json() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "app.yaml", "server:\n  port: 8080\n  host: localhost\n");
    let out = run("confx", &[f.to_str().unwrap(), "--out", "json"]);
    assert!(out.status.success());
    let v = json(&out);
    assert_eq!(v["format"], "yaml");
    assert_eq!(v["data"]["server"]["port"], 8080);
    assert_eq!(v["data"]["server"]["host"], "localhost");
}

#[test]
fn confx_toml_parses_to_json() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "config.toml", "[server]\nport = 8080\nhost = \"localhost\"\n");
    let out = run("confx", &[f.to_str().unwrap(), "--out", "json"]);
    let v = json(&out);
    assert_eq!(v["format"], "toml");
    assert_eq!(v["data"]["server"]["port"], 8080);
}

#[test]
fn confx_ini_sections_become_objects() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "settings.ini", "[database]\nhost = db.local\nport = 5432\n");
    let out = run("confx", &[f.to_str().unwrap(), "--out", "json"]);
    let v = json(&out);
    assert_eq!(v["format"], "ini");
    // INI values are strings (no type info).
    assert_eq!(v["data"]["database"]["host"], "db.local");
    assert_eq!(v["data"]["database"]["port"], "5432");
}

#[test]
fn confx_properties_parses_flat() {
    let dir = tempdir().unwrap();
    let f = write_temp(
        dir.path(),
        "db.properties",
        "# comment\ndb.host=localhost\ndb.port:5432\n",
    );
    let out = run("confx", &[f.to_str().unwrap(), "--out", "json"]);
    let v = json(&out);
    assert_eq!(v["format"], "properties");
    assert_eq!(v["data"]["db.host"], "localhost");
    assert_eq!(v["data"]["db.port"], "5432");
}

#[test]
fn confx_json_passthrough() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "data.json", r#"{"a":1,"b":[2,3]}"#);
    let out = run("confx", &[f.to_str().unwrap(), "--out", "json"]);
    let v = json(&out);
    assert_eq!(v["format"], "json");
    assert_eq!(v["data"]["a"], 1);
    assert_eq!(v["data"]["b"][1], 3);
}

#[test]
fn confx_format_override_on_extensionless_file() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "mystery", "key = \"value\"\n");
    let out = run("confx", &[f.to_str().unwrap(), "--format", "toml", "--out", "json"]);
    let v = json(&out);
    assert_eq!(v["format"], "toml");
    assert_eq!(v["data"]["key"], "value");
}

#[test]
fn confx_raw_emits_bare_data() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "app.yaml", "port: 9090\n");
    let out = run("confx", &[f.to_str().unwrap(), "--raw", "--out", "json"]);
    let v = json(&out);
    // No wrapper fields — the data itself is at the top level.
    assert_eq!(v["port"], 9090);
    assert!(v.get("format").is_none());
}

#[test]
fn confx_multiple_files_wrapped_in_count_and_files() {
    let dir = tempdir().unwrap();
    let a = write_temp(dir.path(), "a.yaml", "x: 1\n");
    let b = write_temp(dir.path(), "b.toml", "y = 2\n");
    let out = run("confx", &[a.to_str().unwrap(), b.to_str().unwrap(), "--out", "json"]);
    let v = json(&out);
    assert_eq!(v["count"], 2);
    assert_eq!(v["files"].as_array().unwrap().len(), 2);
}

#[test]
fn confx_unknown_extension_errors() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "file.xyz", "whatever");
    let out = run("confx", &[f.to_str().unwrap(), "--out", "json"]);
    assert!(!out.status.success(), "unknown extension should exit non-zero");
    let v = json(&out);
    assert!(v["error"].is_string());
}

#[test]
fn confx_invalid_syntax_has_error_field() {
    let dir = tempdir().unwrap();
    let f = write_temp(dir.path(), "bad.toml", "this is = = not valid toml");
    let out = run("confx", &[f.to_str().unwrap(), "--out", "json"]);
    assert!(!out.status.success());
    let v = json(&out);
    assert!(v["error"].is_string(), "parse failure should expose an error field");
}

#[test]
fn confx_missing_file_errors() {
    let out = run("confx", &["/nonexistent/config.yaml", "--out", "json"]);
    assert!(!out.status.success());
    let v = json(&out);
    assert!(v["error"].is_string());
}
