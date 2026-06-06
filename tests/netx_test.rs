mod common;
use common::run_json_forced;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

/// Spin up a one-shot local HTTP server returning a canned JSON response.
/// Returns the bound port. No external network is touched.
fn serve_once(body: &'static str, content_type: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Drain the request headers so the client's write completes.
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                content_type,
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

#[test]
fn netx_parses_json_response() {
    let port = serve_once(r#"{"hello":"world","n":42}"#, "application/json");
    let url = format!("http://127.0.0.1:{}/", port);
    let out = run_json_forced("netx", &[&url]);

    assert_eq!(out["status"].as_u64(), Some(200));
    assert_eq!(out["ok"], true);
    // application/json bodies are parsed into a structured object.
    assert_eq!(out["body"]["hello"], "world");
    assert_eq!(out["body"]["n"].as_u64(), Some(42));
    assert!(out["timing"]["total_ms"].is_number());
}

#[test]
fn netx_text_body_is_string() {
    let port = serve_once("plain text response", "text/plain");
    let url = format!("http://127.0.0.1:{}/", port);
    let out = run_json_forced("netx", &[&url]);
    assert_eq!(out["status"].as_u64(), Some(200));
    assert_eq!(out["body"], "plain text response");
}
