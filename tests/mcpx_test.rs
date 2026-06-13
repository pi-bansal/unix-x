//! Integration tests for the `mcpx` MCP server.
//!
//! Drives the server over its stdio JSON-RPC transport the same way an MCP
//! client would: write newline-delimited requests, read newline-delimited
//! responses.

mod common;

use serde_json::Value;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// Send newline-delimited JSON-RPC request lines to `mcpx` and return each
/// response line parsed as JSON.
fn rpc(requests: &[&str]) -> Vec<Value> {
    let mut child = Command::new(common::bin("mcpx"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn mcpx — did you run `cargo build --workspace`?");

    {
        let mut stdin = child.stdin.take().unwrap();
        for req in requests {
            writeln!(stdin, "{req}").unwrap();
        }
        // Dropping stdin closes the pipe so the server's read loop ends.
    }

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    child.wait().unwrap();

    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("response line was not valid JSON"))
        .collect()
}

#[test]
fn initialize_reports_server_info_and_echoes_protocol() {
    let resp = rpc(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}"#,
    ]);
    assert_eq!(resp.len(), 1);
    let result = &resp[0]["result"];
    assert_eq!(result["serverInfo"]["name"], "aiutilx");
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert!(result["capabilities"].get("tools").is_some());
}

#[test]
fn notifications_get_no_response() {
    // A notification (no id) followed by a request: only the request replies.
    let resp = rpc(&[
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
    ]);
    assert_eq!(resp.len(), 1);
    assert_eq!(resp[0]["id"], 2);
}

#[test]
fn tools_list_exposes_all_tools() {
    let resp = rpc(&[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#]);
    let tools = resp[0]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 18);

    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in ["lx", "px", "jsonx", "hashx", "confx"] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    // Every tool advertises the args/stdin input schema.
    let jsonx = tools.iter().find(|t| t["name"] == "jsonx").unwrap();
    let props = &jsonx["inputSchema"]["properties"];
    assert!(props.get("args").is_some());
    assert!(props.get("stdin").is_some());
}

#[test]
fn tools_call_runs_tool_with_stdin() {
    let resp = rpc(&[
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"jsonx","arguments":{"args":[".items[?(@.price > 10)].name"],"stdin":"{\"items\":[{\"name\":\"a\",\"price\":5},{\"name\":\"b\",\"price\":20}]}"}}}"#,
    ]);
    let result = &resp[0]["result"];
    assert_eq!(result["isError"], false);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"b\""), "unexpected output: {text}");
}

#[test]
fn tools_call_reports_failure_as_is_error() {
    let resp = rpc(&[
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"jsonx","arguments":{"args":[".x","/no/such/file.json"]}}}"#,
    ]);
    let result = &resp[0]["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("error"), "expected structured error, got: {text}");
}

#[test]
fn unknown_tool_is_invalid_params() {
    let resp = rpc(&[
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
    ]);
    assert_eq!(resp[0]["error"]["code"], -32602);
}

#[test]
fn unknown_method_is_method_not_found() {
    let resp = rpc(&[r#"{"jsonrpc":"2.0","id":6,"method":"bogus/method"}"#]);
    assert_eq!(resp[0]["error"]["code"], -32601);
}
