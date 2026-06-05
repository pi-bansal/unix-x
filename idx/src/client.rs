/// Client — connects to the daemon socket and sends queries.
/// Used by `idx query` and by `lx` when a daemon is running for the current dir.

use crate::daemon::{socket_path, Request, Response};
use crate::query::Query;
use std::path::Path;
use std::time::Duration;

#[derive(Debug)]
pub enum ClientError {
    NoDaemon,
    Io(std::io::Error),
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NoDaemon => write!(f, "no idx daemon running for this directory — run `idx start`"),
            ClientError::Io(e)    => write!(f, "socket error: {}", e),
            ClientError::Protocol(s) => write!(f, "protocol error: {}", s),
        }
    }
}

/// Check whether a daemon is running for `root`.
pub fn daemon_running(root: &Path) -> bool {
    let sock = socket_path(root);
    sock.exists() && std::os::unix::net::UnixStream::connect(&sock).is_ok()
}

/// Send a query to the daemon and return the response.
pub fn send_query(root: &Path, query: Query) -> Result<Response, ClientError> {
    send_request(root, Request::Query(query))
}

pub fn send_status(root: &Path) -> Result<Response, ClientError> {
    send_request(root, Request::Status)
}

pub fn send_rebuild(root: &Path) -> Result<Response, ClientError> {
    send_request(root, Request::Rebuild)
}

fn send_request(root: &Path, request: Request) -> Result<Response, ClientError> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let sock = socket_path(root);
    if !sock.exists() {
        return Err(ClientError::NoDaemon);
    }

    let mut stream = UnixStream::connect(&sock)
        .map_err(|_| ClientError::NoDaemon)?;

    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(ClientError::Io)?;

    let mut msg = serde_json::to_string(&request)
        .map_err(|e| ClientError::Protocol(e.to_string()))?;
    msg.push('\n');

    stream.write_all(msg.as_bytes()).map_err(ClientError::Io)?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(ClientError::Io)?;

    serde_json::from_str(&line)
        .map_err(|e| ClientError::Protocol(format!("{}: {}", e, line.trim())))
}
