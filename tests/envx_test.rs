mod common;
use common::run_json_forced;

/// Create a temp dir containing a `.env` file with a known secret + plain var.
fn make_env() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".env"),
        "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\nGREETING=hello\n",
    )
    .unwrap();
    dir
}

#[test]
fn envx_reads_dotenv_and_flags_secret() {
    let dir = make_env();
    let out = run_json_forced("envx", &[dir.path().to_str().unwrap()]);
    assert!(out["dotenv_var_count"].as_u64().unwrap() >= 2);
    assert!(out["secret_count"].as_u64().unwrap() >= 1);

    let vars = out["vars"].as_array().expect("vars array");
    let aws = vars
        .iter()
        .find(|v| v["key"] == "AWS_ACCESS_KEY_ID")
        .expect("AWS key var present");
    assert!(!aws["secret"].is_null(), "AWS key should be flagged as a secret");

    let greeting = vars.iter().find(|v| v["key"] == "GREETING").expect("greeting var");
    assert_eq!(greeting["value"], "hello");
}

#[test]
fn envx_redacts_secret_values() {
    let dir = make_env();
    let out = run_json_forced("envx", &[dir.path().to_str().unwrap(), "--redact"]);
    let vars = out["vars"].as_array().unwrap();
    let aws = vars.iter().find(|v| v["key"] == "AWS_ACCESS_KEY_ID").unwrap();
    assert_eq!(aws["redacted"], true);
    assert_ne!(
        aws["value"].as_str(),
        Some("AKIAIOSFODNN7EXAMPLE"),
        "redacted secret value must not be exposed"
    );
}

#[test]
fn envx_secrets_only_filters_plain_vars() {
    let dir = make_env();
    let out = run_json_forced("envx", &[dir.path().to_str().unwrap(), "--secrets-only"]);
    let vars = out["vars"].as_array().unwrap();
    assert!(vars.iter().all(|v| !v["secret"].is_null()));
    assert!(vars.iter().all(|v| v["key"] != "GREETING"));
}

/// A .env exercising several distinct secret detectors plus a plain value.
fn make_mixed_secrets() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".env"),
        concat!(
            "JWT_TOKEN=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc123signaturepart\n",
            "DB_PASSWORD=supersecretvalue123\n",
            "GREETING=hello\n",
        ),
    )
    .unwrap();
    dir
}

#[test]
fn envx_detects_jwt_and_key_name_heuristics() {
    let dir = make_mixed_secrets();
    let out = run_json_forced("envx", &[dir.path().to_str().unwrap()]);
    let vars = out["vars"].as_array().unwrap();

    let jwt = vars.iter().find(|v| v["key"] == "JWT_TOKEN").unwrap();
    assert!(!jwt["secret"].is_null(), "JWT should be flagged");

    let pw = vars.iter().find(|v| v["key"] == "DB_PASSWORD").unwrap();
    assert!(!pw["secret"].is_null(), "PASSWORD-named key should be flagged");

    let plain = vars.iter().find(|v| v["key"] == "GREETING").unwrap();
    assert!(plain["secret"].is_null(), "plain value should not be flagged");
}

#[test]
fn envx_filter_by_key_substring() {
    let dir = make_mixed_secrets();
    let out = run_json_forced("envx", &[dir.path().to_str().unwrap(), "--filter", "PASSWORD"]);
    let vars = out["vars"].as_array().unwrap();
    assert!(!vars.is_empty());
    assert!(vars.iter().all(|v| v["key"].as_str().unwrap().contains("PASSWORD")));
}

#[test]
fn envx_shell_includes_process_environment() {
    // Use an empty dir so no .env is picked up; --shell should still surface
    // the inherited process environment (PATH is virtually always set).
    let dir = tempfile::tempdir().unwrap();
    let out = run_json_forced("envx", &[dir.path().to_str().unwrap(), "--shell"]);
    let vars = out["vars"].as_array().unwrap();
    assert!(
        vars.iter().any(|v| v["key"] == "PATH"),
        "--shell should include inherited environment variables like PATH"
    );
}
