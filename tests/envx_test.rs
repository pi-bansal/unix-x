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
