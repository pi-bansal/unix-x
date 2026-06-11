// ux-output: shared output + platform fallback logic for aiutilx tools

use serde::Serialize;

// ── Output mode ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum OutMode {
    Json,
    Pretty,
    Table,
    Ndjson,
    Auto,
}

impl OutMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json"   => OutMode::Json,
            "pretty" => OutMode::Pretty,
            "table"  => OutMode::Table,
            "ndjson" => OutMode::Ndjson,
            _        => OutMode::Auto,
        }
    }

    pub fn resolve(&self) -> ResolvedMode {
        match self {
            OutMode::Json   => ResolvedMode::Compact,
            OutMode::Pretty => ResolvedMode::Pretty,
            OutMode::Table  => ResolvedMode::Table,
            OutMode::Ndjson => ResolvedMode::Ndjson,
            OutMode::Auto   => {
                if is_tty() { ResolvedMode::Pretty } else { ResolvedMode::Compact }
            }
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum ResolvedMode {
    Compact,
    Pretty,
    Table,
    Ndjson,
}

pub fn emit<T: Serialize>(value: &T, mode: &OutMode) {
    match mode.resolve() {
        ResolvedMode::Pretty  => println!("{}", serde_json::to_string_pretty(value).unwrap()),
        ResolvedMode::Compact => println!("{}", serde_json::to_string(value).unwrap()),
        ResolvedMode::Ndjson  => {
            let v = serde_json::to_value(value).unwrap();
            emit_ndjson(&v);
        }
        ResolvedMode::Table => {
            // Tools override this; default falls back to pretty
            println!("{}", serde_json::to_string_pretty(value).unwrap());
        }
    }
}

pub fn emit_ndjson(v: &serde_json::Value) {
    match v {
        serde_json::Value::Array(arr) => {
            for item in arr { println!("{}", serde_json::to_string(item).unwrap()); }
        }
        other => println!("{}", serde_json::to_string(other).unwrap()),
    }
}

/// Restore default SIGPIPE behavior so a tool exits quietly when the reader
/// closes the pipe early (`tool | head`, `tool | grep -q`, `… | less` then `q`),
/// exactly like a standard Unix utility.
///
/// Rust sets SIGPIPE to `SIG_IGN` at startup, which turns a closed-pipe write
/// into an `EPIPE` error and makes `println!` panic ("failed printing to stdout:
/// Broken pipe"). Resetting to `SIG_DFL` makes the process terminate on the
/// signal instead, matching `ls`/`grep`/`jq`. Call this as the first line of
/// `main()` in every tool.
pub fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

pub fn is_tty() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::isatty(std::io::stdout().as_raw_fd()) != 0 }
    }
    #[cfg(not(unix))]
    {
        // Windows: check via kernel32
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::io::AsRawHandle;
            extern "system" {
                fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
            }
            let handle = std::io::stdout().as_raw_handle();
            let mut mode: u32 = 0;
            unsafe { GetConsoleMode(handle as *mut _, &mut mode) != 0 }
        }
        #[cfg(not(target_os = "windows"))]
        false
    }
}

// ── Platform availability ─────────────────────────────────────────────────────

/// Describes why a feature is unavailable on the current platform.
#[derive(Serialize, Clone)]
pub struct Unavailable {
    pub feature: String,
    pub reason: String,
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl Unavailable {
    pub fn new(
        feature: impl Into<String>,
        reason: impl Into<String>,
        suggestion: Option<&str>,
    ) -> Self {
        Unavailable {
            feature: feature.into(),
            reason: reason.into(),
            platform: current_platform(),
            suggestion: suggestion.map(|s| s.to_string()),
        }
    }
}

/// Wraps a Vec<T> with an optional unavailability explanation.
/// If unavailable, the vec is always empty and unavailable is Some.
#[derive(Serialize)]
pub struct MaybeAvailable<T: Serialize> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<Unavailable>,
}

impl<T: Serialize> MaybeAvailable<T> {
    pub fn available(items: Vec<T>) -> Self {
        MaybeAvailable { items, unavailable: None }
    }

    pub fn unavailable(feature: &str, reason: &str, suggestion: Option<&str>) -> Self {
        MaybeAvailable {
            items: vec![],
            unavailable: Some(Unavailable::new(feature, reason, suggestion)),
        }
    }

    pub fn unavailable_from(u: Unavailable) -> Self {
        MaybeAvailable { items: vec![], unavailable: Some(u) }
    }

    pub fn from_result(
        result: Result<Vec<T>, Unavailable>,
    ) -> Self {
        match result {
            Ok(items) => Self::available(items),
            Err(u)    => MaybeAvailable { items: vec![], unavailable: Some(u) },
        }
    }
}

// ── Platform detection helpers ────────────────────────────────────────────────

pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

pub fn is_linux() -> bool { std::env::consts::OS == "linux" }
pub fn is_macos() -> bool { std::env::consts::OS == "macos" }
pub fn is_windows() -> bool { std::env::consts::OS == "windows" }

/// Check if a binary is available on PATH.
pub fn has_command(cmd: &str) -> bool {
    which(cmd).is_some()
}

fn which(cmd: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path).find_map(|dir| {
            let full = dir.join(cmd);
            // On Windows try .exe too
            let candidates = if is_windows() {
                vec![full.with_extension("exe"), full.clone()]
            } else {
                vec![full]
            };
            candidates.into_iter().find(|p| p.is_file())
        })
    })
}

// ── Common unavailability reasons ─────────────────────────────────────────────

pub mod unavail {
    use super::Unavailable;

    pub fn proc_net() -> Unavailable {
        Unavailable::new(
            "network_connections",
            "/proc/net is only available on Linux",
            Some("on macOS: install lsof and use `lsof -i`. on Windows: use `netstat -ano`"),
        )
    }

    pub fn systemd() -> Unavailable {
        Unavailable::new(
            "systemd_units",
            "systemd is not available on this platform",
            if super::is_macos() {
                Some("use `procx --source launchd` for scheduled jobs on macOS")
            } else if super::is_windows() {
                Some("use Task Scheduler or `procx --source windows-tasks` (planned)")
            } else {
                Some("systemd may not be the init system — try `ps -p 1`")
            },
        )
    }

    pub fn launchd() -> Unavailable {
        Unavailable::new(
            "launchd_jobs",
            "launchd is only available on macOS",
            Some("on Linux: use `procx --source systemd` or `procx --source cron`"),
        )
    }

    pub fn proc_fs() -> Unavailable {
        Unavailable::new(
            "proc_filesystem",
            "/proc is only available on Linux",
            Some("many px features require Linux — on macOS use `px --name <n>` via sysinfo"),
        )
    }

    pub fn git_unavailable() -> Unavailable {
        Unavailable::new(
            "git_status",
            "git not found on PATH or path is not inside a git repository",
            Some("install git or run lx outside a git repo with --no-git"),
        )
    }
}

#[cfg(test)]
mod tests;

