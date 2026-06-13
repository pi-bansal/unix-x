mod common;
use common::run_json_forced;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
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

/// Serve a 302 -> 200 chain over two sequential connections. No external network.
fn serve_redirect_then_ok(final_body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        // First hit: redirect to a relative path.
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let resp = "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
        // Second hit: the final 200.
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                final_body.len(),
                final_body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

#[test]
fn netx_tracks_redirect_chain() {
    let port = serve_redirect_then_ok("done");
    let url = format!("http://127.0.0.1:{}/", port);
    let out = run_json_forced("netx", &[&url]);

    assert_eq!(out["status"].as_u64(), Some(200));
    assert_eq!(out["body"], "done");

    let redirects = out["redirects"].as_array().expect("redirects array");
    assert_eq!(redirects.len(), 1, "one hop should be recorded");
    assert_eq!(redirects[0]["status"].as_u64(), Some(302));
    // Relative Location resolved against the base; final URL is the target.
    assert!(out["url"].as_str().unwrap().ends_with("/final"));
}

/// Serve a single response with an arbitrary status line, returning the port.
fn serve_status(status_line: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let resp = format!("HTTP/1.1 {status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

#[test]
fn netx_non_2xx_sets_ok_false() {
    let port = serve_status("404 Not Found");
    let url = format!("http://127.0.0.1:{port}/");
    let out = run_json_forced("netx", &[&url]);
    assert_eq!(out["status"].as_u64(), Some(404));
    assert_eq!(out["ok"], false);
}

#[test]
fn netx_head_only_omits_body() {
    let port = serve_once(r#"{"k":"v"}"#, "application/json");
    let url = format!("http://127.0.0.1:{port}/");
    let out = run_json_forced("netx", &[&url, "--head-only"]);
    assert_eq!(out["status"].as_u64(), Some(200));
    // --head-only skips reading the body: it comes back empty with zero bytes.
    assert_eq!(out["body"].as_str(), Some(""), "head-only body should be empty");
    assert_eq!(out["body_bytes"].as_u64(), Some(0));
    assert!(out["headers"].is_object(), "headers should still be present");
}

/// Capture the raw request bytes the client sends, then return 200. The captured
/// request is delivered over a channel so the test can assert on method/body.
///
/// Reads in a loop until the full request (headers + any `Content-Length` body)
/// has arrived — a single `read()` can return just the headers if the body lands
/// in a separate TCP segment, which is timing-dependent and flaky under debug
/// builds / loaded CI runners.
fn serve_capturing(tx: mpsc::Sender<String>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..n]);

                let text = String::from_utf8_lossy(&data);
                if let Some(header_end) = text.find("\r\n\r\n") {
                    let headers = &text[..header_end];
                    let content_length = headers
                        .lines()
                        .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string()))
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    let body_len = data.len() - (header_end + 4);
                    if body_len >= content_length {
                        break;
                    }
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&data).into_owned());
            let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

#[test]
fn netx_post_sends_method_headers_and_body() {
    let (tx, rx) = mpsc::channel();
    let port = serve_capturing(tx);
    let url = format!("http://127.0.0.1:{port}/");
    let out = run_json_forced(
        "netx",
        &[&url, "-X", "POST", "--data", r#"{"name":"alice"}"#, "--json", "-H", "X-Custom: hello"],
    );
    assert_eq!(out["status"].as_u64(), Some(200));

    let request = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("request captured");
    assert!(request.starts_with("POST "), "method should be POST: {request}");
    assert!(request.contains("application/json"), "--json should set Content-Type");
    assert!(request.contains("X-Custom: hello"), "custom header should be sent");
    assert!(request.contains(r#"{"name":"alice"}"#), "body should be sent");
}
