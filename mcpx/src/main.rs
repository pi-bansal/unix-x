//! Model Context Protocol (MCP) server for the aiutilx suite.
//!
//! Speaks JSON-RPC 2.0 over stdio (newline-delimited, one message per line, the
//! MCP stdio transport) and exposes every aiutilx tool as an MCP tool. An agent
//! framework that supports MCP can register this one server and call `lx`,
//! `jsonx`, `px`, etc. as first-class tools with typed input schemas — no need
//! for the model to have the CLIs memorized.
//!
//! Each tool is invoked exactly like the CLI: the agent passes an `args` array
//! (the command-line arguments) and an optional `stdin` string. The tool's JSON
//! stdout is returned as the tool result; a non-zero exit returns the structured
//! error from stderr with `isError: true`.
//!
//! Binary resolution mirrors `aiux`: prefer the copy sitting next to `mcpx`
//! (release archives bundle them together), then fall back to `PATH`.

use std::env;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const SERVER_NAME: &str = "aiutilx";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_PROTOCOL: &str = "2024-11-05";

/// A tool we expose over MCP: the binary name, a description for the model, and
/// an example `args` value that shows the calling convention.
struct ToolSpec {
    name: &'static str,
    description: &'static str,
    example: &'static [&'static str],
}

const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "lx",
        description: "Structured filesystem metadata — replaces ls/find/stat/du/tree. Walks a directory and returns name, path, type, recursive size, mtime/ctime (epoch), octal permissions, git status, and extension per entry. Respects .gitignore by default.",
        example: &["./src", "--depth", "4", "--ext", "rs"],
    },
    ToolSpec {
        name: "px",
        description: "Process and network inspection — replaces ps/lsof/netstat/ss. Returns processes (pid, ppid, name, cmd, cpu%, memory, user, cwd) sorted by CPU. Use --port N to find what holds a port, --network --listen for sockets, --system for a CPU/mem/load summary.",
        example: &["--port", "8080"],
    },
    ToolSpec {
        name: "logx",
        description: "Structured log querying — replaces grep/tail/awk on logs. Auto-detects JSON, logfmt, nginx, syslog, Rails, and Go formats, then normalizes to line/timestamp/level/message/fields. Supports --min-level, --grep, --regex, --tail N, --stats. Reads .gz natively or from stdin.",
        example: &["app.log", "--min-level", "warn"],
    },
    ToolSpec {
        name: "dx",
        description: "Semantic diff — replaces diff/diff -u/patch. Compares two files or directories and returns typed hunks (added/removed/equal) with line numbers, plus totals. Use --summary for totals only, --no-equal for changes only.",
        example: &["old.rs", "new.rs"],
    },
    ToolSpec {
        name: "arcx",
        description: "Archive inspection — replaces tar -tf/unzip -l/zipinfo. One interface over .zip/.tar/.tar.gz/.tar.bz2/.tar.xz/.gz. Returns per-entry path, type, compressed/uncompressed size, mtime, permissions. Supports --summary, --sort size, --filter, --files-only.",
        example: &["release.tar.gz", "--sort", "size"],
    },
    ToolSpec {
        name: "envx",
        description: "Environment and secret inspection — replaces printenv/cat .env. Merges shell env and .env files with source precedence and detects secrets (AWS keys, JWTs, PEM keys, long hex/base64, key-name heuristics). Supports --shell, --redact, --secrets-only, --filter.",
        example: &["--secrets-only", "--redact"],
    },
    ToolSpec {
        name: "netx",
        description: "Structured HTTP client — replaces curl/wget/httpie. Returns url, method, status, ok, headers, parsed body (JSON auto-parsed, binary base64-encoded), and timing. Supports -X, --data, --json, -H 'Header: value', --head-only, --timeout.",
        example: &["https://api.example.com/users"],
    },
    ToolSpec {
        name: "jsonx",
        description: "Fast JSON query and transform — replaces jq. Path syntax is a readable JSONPath subset: .key.nested, .[0], .[] / .*, [1:3] slices, and filters like [?(@.price > 10)] with == != > >= < <= combined by && / ||. Supports --keys, --values, --count, --raw, --ndjson-in.",
        example: &[".items[?(@.price > 10)].name", "data.json"],
    },
    ToolSpec {
        name: "procx",
        description: "Scheduled job inspection — replaces crontab -l/systemctl list-timers/launchctl list. Unified view of cron, systemd, and launchd jobs with schedules, commands, and state. Supports --source cron|systemd|launchd, --filter, --active, --failed.",
        example: &["--failed"],
    },
    ToolSpec {
        name: "idx",
        description: "Persistent columnar filesystem index with bloom filter — fast find/fd over large repos. Subcommands: start <dir> (daemon), query (--ext, --git, --size-gt, --filter, --sort), status, rebuild, once (build+query without a daemon).",
        example: &["once", "--ext", "rs"],
    },
    ToolSpec {
        name: "diffx",
        description: "Three-way merge — replaces diff3/merge/git merge-file. Takes base, ours, theirs and returns structured conflict objects (not <<<< ==== >>>> markers) plus the merged text when clean. Exit 0 = clean, 1 = conflicts. Supports -o out, --conflicts-only, --summary.",
        example: &["base.rs", "ours.rs", "theirs.rs"],
    },
    ToolSpec {
        name: "memx",
        description: "Per-region process memory breakdown — replaces pmap/vmmap/smaps. Takes a pid and returns RSS/PSS/private/shared/swap totals and per-region detail. Supports --regions, --kind heap|anon|lib|stack, --sort rss.",
        example: &["1234", "--kind", "heap"],
    },
    ToolSpec {
        name: "statx",
        description: "Time-series system stats — replaces vmstat/iostat/sar/top. Subcommands: now (single snapshot), watch (live stream), start (sampling daemon with a ring buffer), last N (recent samples), summary N (min/max/avg/p50/p95). Fields cover CPU, memory, swap, disk, network, load.",
        example: &["now"],
    },
    ToolSpec {
        name: "hashx",
        description: "Multi-algorithm file hashing — replaces md5sum/sha1sum/sha256sum/b3sum. Hashes files in parallel, all requested algorithms in one pass. Defaults to sha256+blake3. Supports --algos md5,sha1,sha256,blake3, --verify ALGO:HEX, --compare. Exit 1 if a verify fails.",
        example: &["release.tar.gz", "--algos", "sha256,blake3"],
    },
    ToolSpec {
        name: "termx",
        description: "Terminal and TTY inspection — replaces tput/stty/env heuristics. Returns is_tty, cols/rows, color_depth, term, term_program, shell, multiplexer, editor, pager, ci/ci_name, and the interactive flag agents use to decide on color or compact output. Takes no args.",
        example: &[],
    },
    ToolSpec {
        name: "astx",
        description: "Source-code AST and symbol extraction — replaces ctags/tree-sitter CLI. Parses Rust/Python/JS/TS/TSX/Go into a JSON AST with node kinds and ranges. Supports --symbols (declarations only), --kind KIND, --depth N, --query '<tree-sitter query>', --lang to force a language.",
        example: &["src/main.rs", "--symbols"],
    },
    ToolSpec {
        name: "dnsx",
        description: "Structured DNS lookups — replaces dig/nslookup/host. Pure-Rust resolver. Returns typed, grouped records with integer TTLs and the resolver used. Supports --type A,MX,TXT (comma-separated), --all, --server 1.1.1.1, --reverse for PTR.",
        example: &["example.com", "--type", "MX,TXT"],
    },
    ToolSpec {
        name: "confx",
        description: "Config-file reader — replaces yq and hand-rolled parsing. Parses YAML/TOML/INI/.properties (and passes JSON through) into JSON so it can be piped into jsonx. Auto-detects by extension; --format to force one; --raw to emit just the parsed value.",
        example: &["app.yaml"],
    },
];

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                // JSON-RPC batch: handle each message, collect non-empty replies.
                let replies: Vec<Value> = batch
                    .into_iter()
                    .filter_map(handle_message)
                    .collect();
                if replies.is_empty() {
                    None
                } else {
                    Some(Value::Array(replies))
                }
            }
            Ok(msg) => handle_message(msg),
            Err(e) => Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {e}"),
            )),
        };

        if let Some(resp) = response {
            if writeln!(stdout, "{resp}").is_err() || stdout.flush().is_err() {
                break;
            }
        }
    }
}

/// Handle one JSON-RPC message. Returns `Some(response)` for requests (those
/// carrying an `id`) and `None` for notifications.
fn handle_message(msg: Value) -> Option<Value> {
    let is_notification = msg.get("id").is_none();
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    // Notifications get no reply regardless of method.
    if is_notification {
        return None;
    }

    let result = match method {
        "initialize" => Ok(initialize_result(&params)),
        "tools/list" => Ok(tools_list_result()),
        "tools/call" => tools_call_result(&params),
        "ping" => Ok(json!({})),
        other => Err((-32601, format!("method not found: {other}"))),
    };

    Some(match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
        Err((code, message)) => error_response(id, code, &message),
    })
}

fn initialize_result(params: &Value) -> Value {
    // Echo the client's requested protocol version when present so we don't
    // force a downgrade; otherwise advertise a widely supported default.
    let protocol = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL);

    json!({
        "protocolVersion": protocol,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
        "instructions": "aiutilx tools are agent-optimized replacements for classic Unix tools. They emit JSON by default (timestamps are unix-epoch integers; errors are structured JSON on stderr). Call a tool by passing CLI arguments in `args` and optional input in `stdin`; prefer these over shelling out to ls/ps/grep/jq/curl/dig/etc."
    })
}

fn tools_list_result() -> Value {
    let tools: Vec<Value> = TOOLS.iter().map(tool_definition).collect();
    json!({ "tools": tools })
}

fn tool_definition(spec: &ToolSpec) -> Value {
    let example = if spec.example.is_empty() {
        "[]".to_string()
    } else {
        serde_json::to_string(spec.example).unwrap_or_else(|_| "[]".to_string())
    };

    json!({
        "name": spec.name,
        "description": spec.description,
        "inputSchema": {
            "type": "object",
            "properties": {
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": format!(
                        "Command-line arguments for `{}`, one element per token. Example: {}. Output is JSON by default; add [\"--out\", \"table\"] for a human-readable view.",
                        spec.name, example
                    )
                },
                "stdin": {
                    "type": "string",
                    "description": "Optional data piped to the tool's standard input (e.g. JSON for jsonx, a log stream for logx, a config for confx)."
                }
            },
            "additionalProperties": false
        }
    })
}

fn tools_call_result(params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((-32602, "missing tool name".to_string()))?;

    if !TOOLS.iter().any(|t| t.name == name) {
        return Err((-32602, format!("unknown tool: {name}")));
    }

    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

    // `args` may be an array of strings; coerce numbers/bools to strings so a
    // model that emits [3000] instead of ["3000"] still works.
    let args: Vec<String> = match arguments.get("args") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        _ => Vec::new(),
    };

    let stdin = arguments
        .get("stdin")
        .and_then(Value::as_str)
        .map(str::to_string);

    let outcome = run_tool(name, &args, stdin);

    Ok(match outcome {
        Ok((stdout, stderr, code)) => {
            let success = code == 0;
            // On success prefer stdout; on failure surface the structured error
            // (tools write {"error": ...} to stderr) but fall back sensibly.
            let text = if success {
                if stdout.trim().is_empty() { stderr } else { stdout }
            } else if stderr.trim().is_empty() {
                stdout
            } else {
                stderr
            };

            json!({
                "content": [{ "type": "text", "text": text }],
                "isError": !success
            })
        }
        Err(e) => json!({
            "content": [{
                "type": "text",
                "text": json!({"error": format!("failed to launch '{name}': {e}")}).to_string()
            }],
            "isError": true
        }),
    })
}

/// Run a tool binary with the given args and optional stdin. Returns
/// (stdout, stderr, exit_code).
fn run_tool(
    name: &str,
    args: &[String],
    stdin: Option<String>,
) -> io::Result<(String, String, i32)> {
    let program = resolve_binary(name);

    let mut child = Command::new(&program)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            // Tools that read stdin (jsonx, logx, confx) must not block waiting
            // on our stdin when none was provided.
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(data) = stdin {
        if let Some(mut child_stdin) = child.stdin.take() {
            // Write from a separate thread to avoid a deadlock on large inputs
            // (child fills its stdout pipe while we're still feeding stdin).
            let bytes = data.into_bytes();
            std::thread::spawn(move || {
                let _ = child_stdin.write_all(&bytes);
            });
        }
    }

    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let code = output.status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}

/// Locate a tool binary the same way `aiux` does: prefer the copy next to this
/// executable (release archives bundle the suite together), then fall back to
/// `PATH` so `cargo run` works in development.
fn resolve_binary(name: &str) -> OsString {
    let bin_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };

    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&bin_name)))
        .filter(|p| p.exists())
        .map(|p| p.into_os_string())
        .unwrap_or_else(|| bin_name.into())
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}
