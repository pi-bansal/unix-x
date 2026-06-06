mod common;
use common::run_json_forced;

#[test]
fn procx_outputs_structured_sources() {
    let out = run_json_forced("procx", &[]);
    assert!(out["platform"].is_string());
    // Each source is always present as either available items or a structured
    // unavailable block — never missing, never a crash.
    for source in ["cron", "systemd", "launchd"] {
        assert!(out.get(source).is_some(), "missing source key: {}", source);
    }
}

#[test]
fn procx_source_filter_isolates_one_source() {
    // Asking for only cron must not populate systemd/launchd with items.
    let out = run_json_forced("procx", &["--source", "cron"]);
    assert!(out["systemd"]["items"].as_array().map(|a| a.is_empty()).unwrap_or(true));
    assert!(out["launchd"]["items"].as_array().map(|a| a.is_empty()).unwrap_or(true));
}

#[test]
fn procx_unavailable_block_is_well_formed() {
    // Whichever sources are unavailable on this platform must carry a reason.
    let out = run_json_forced("procx", &[]);
    for source in ["cron", "systemd", "launchd"] {
        if let Some(u) = out[source].get("unavailable") {
            assert!(u["reason"].is_string(), "{} unavailable block needs a reason", source);
            assert!(u["feature"].is_string());
        }
    }
}
