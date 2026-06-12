/// Dispatcher — runs `aiux <tool> [args...]` as if you'd called `<tool> [args...]`
/// directly. Looks for the tool binary next to `aiux` first, then falls back to
/// `PATH`. Lets agents discover the whole suite through one entry point without
/// rebundling every tool into a single binary.

use std::env;
use std::process::{exit, Command};

const TOOLS: &[&str] = &[
    "lx", "px", "logx", "dx", "arcx", "envx", "netx", "jsonx", "procx", "idx",
    "diffx", "memx", "statx", "hashx", "termx", "astx", "dnsx", "confx",
];

fn main() {
    let mut args = env::args();
    let _prog = args.next();

    let tool = match args.next() {
        Some(t) => t,
        None => {
            eprintln!("{}", serde_json::json!({
                "error": "usage: aiux <tool> [args...]",
                "tools": TOOLS,
            }));
            exit(1);
        }
    };

    if !TOOLS.contains(&tool.as_str()) {
        eprintln!("{}", serde_json::json!({
            "error": format!("unknown tool '{tool}'"),
            "tools": TOOLS,
        }));
        exit(1);
    }

    let bin_name = if cfg!(windows) { format!("{tool}.exe") } else { tool.clone() };

    // Prefer the copy next to aiux (release archives bundle them together);
    // fall back to PATH so `cargo run -p aiux -- lx ...` works in dev too.
    let program = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&bin_name)))
        .filter(|p| p.exists())
        .map(|p| p.into_os_string())
        .unwrap_or_else(|| bin_name.into());

    let status = Command::new(&program).args(args).status();

    match status {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("{}", serde_json::json!({
                "error": format!("failed to launch '{tool}': {e}"),
            }));
            exit(1);
        }
    }
}
