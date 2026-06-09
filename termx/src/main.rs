use clap::Parser;
use serde::Serialize;
use std::env;
use ux_output::{emit, OutMode};

#[derive(Parser)]
#[command(
    name = "termx",
    about = "Structured terminal and TTY inspection for AI agents.\nReplaces: tput, stty, env inspection, tty heuristics.",
    version
)]
struct Cli {
    /// Output mode: auto, json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

#[derive(Serialize)]
pub struct TermInfo {
    // TTY
    pub is_tty:         bool,
    pub tty_device:     Option<String>,   // e.g. /dev/pts/0

    // Dimensions
    pub cols:           Option<u16>,
    pub rows:           Option<u16>,

    // Color support
    pub color_depth:    ColorDepth,
    pub color_depth_bits: u8,             // 0, 1, 4, 8, 24

    // Terminal emulator
    pub term:           Option<String>,   // $TERM
    pub term_program:   Option<String>,   // $TERM_PROGRAM (iTerm2, vscode, etc.)
    pub colorterm:      Option<String>,   // $COLORTERM (truecolor, 256color)

    // Shell
    pub shell:          Option<String>,   // $SHELL
    pub shell_name:     Option<String>,   // basename of $SHELL

    // Multiplexer
    pub multiplexer:    Option<Multiplexer>,

    // Editor / pager
    pub editor:         Option<String>,   // $EDITOR
    pub pager:          Option<String>,   // $PAGER

    // Locale
    pub lang:           Option<String>,   // $LANG
    pub unicode:        bool,             // lang contains UTF-8

    // CI / non-interactive detection
    pub ci:             bool,             // $CI set
    pub ci_name:        Option<String>,   // GitHub Actions, GitLab CI, etc.
    pub interactive:    bool,             // is_tty && !ci
}

#[derive(Serialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ColorDepth {
    None,         // no color support
    Basic,        // 8 colors (ANSI)
    Extended,     // 256 colors
    TrueColor,    // 24-bit
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Multiplexer {
    Tmux,
    Screen,
    Zellij,
    Wezterm,
}

fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);
    let info = inspect();

    if cli.out == "table" {
        print_table(&info);
    } else {
        emit(&info, &mode);
    }
}

fn inspect() -> TermInfo {
    let is_tty = ux_output::is_tty();
    let tty_device = get_tty_device();
    let (cols, rows) = get_terminal_size();
    let term         = env::var("TERM").ok();
    let term_program = env::var("TERM_PROGRAM").ok();
    let colorterm    = env::var("COLORTERM").ok();
    let shell        = env::var("SHELL").ok();
    let shell_name   = shell.as_ref()
        .and_then(|s| std::path::Path::new(s).file_name())
        .map(|n| n.to_string_lossy().to_string());
    let lang         = env::var("LANG").ok();
    let unicode      = lang.as_deref().map(|l| l.to_uppercase().contains("UTF")).unwrap_or(false);
    let editor       = env::var("EDITOR").or(env::var("VISUAL")).ok();
    let pager        = env::var("PAGER").ok();

    let (color_depth, color_depth_bits) = detect_color(&term, &colorterm, &term_program);

    let multiplexer = detect_multiplexer();
    let (ci, ci_name) = detect_ci();
    let interactive = is_tty && !ci;

    TermInfo {
        is_tty,
        tty_device,
        cols,
        rows,
        color_depth,
        color_depth_bits,
        term,
        term_program,
        colorterm,
        shell,
        shell_name,
        multiplexer,
        editor,
        pager,
        lang,
        unicode,
        ci,
        ci_name,
        interactive,
    }
}

fn get_tty_device() -> Option<String> {
    #[cfg(unix)]
    {
        use std::ffi::CStr;
        let fd = 1i32; // stdout
        let ptr = unsafe { libc::ttyname(fd) };
        if ptr.is_null() { return None; }
        unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .ok()
            .map(|s| s.to_string())
    }
    #[cfg(not(unix))]
    None
}

fn get_terminal_size() -> (Option<u16>, Option<u16>) {
    #[cfg(unix)]
    {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) };
        if ret == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            return (Some(ws.ws_col), Some(ws.ws_row));
        }
    }

    // Fallback: $COLUMNS / $LINES
    let cols = env::var("COLUMNS").ok().and_then(|v| v.parse().ok());
    let rows = env::var("LINES").ok().and_then(|v| v.parse().ok());
    (cols, rows)
}

fn detect_color(
    term: &Option<String>,
    colorterm: &Option<String>,
    term_program: &Option<String>,
) -> (ColorDepth, u8) {
    // Explicit truecolor signals
    if let Some(ref ct) = colorterm {
        let ct = ct.to_lowercase();
        if ct == "truecolor" || ct == "24bit" {
            return (ColorDepth::TrueColor, 24);
        }
    }

    // Known truecolor terminals
    if let Some(ref tp) = term_program {
        match tp.to_lowercase().as_str() {
            "iterm.app" | "hyper" | "wezterm" | "vscode" | "alacritty" | "kitty" => {
                return (ColorDepth::TrueColor, 24);
            }
            _ => {}
        }
    }

    // $TERM signals
    if let Some(ref t) = term {
        let t = t.to_lowercase();
        if t.contains("256color") {
            return (ColorDepth::Extended, 8);
        }
        if t == "dumb" || t == "vt100" {
            return (ColorDepth::None, 0);
        }
        if t.starts_with("xterm") || t.starts_with("screen") || t.starts_with("tmux") {
            return (ColorDepth::Extended, 8);
        }
        if t.contains("color") {
            return (ColorDepth::Basic, 4);
        }
    }

    // $COLORTERM fallback
    if colorterm.is_some() {
        return (ColorDepth::Basic, 4);
    }

    (ColorDepth::None, 0)
}

fn detect_multiplexer() -> Option<Multiplexer> {
    if env::var("TMUX").is_ok() { return Some(Multiplexer::Tmux); }
    if env::var("STY").is_ok()  { return Some(Multiplexer::Screen); }
    if env::var("ZELLIJ").is_ok() { return Some(Multiplexer::Zellij); }
    if env::var("TERM_PROGRAM").ok().as_deref() == Some("WezTerm") {
        return Some(Multiplexer::Wezterm);
    }
    None
}

fn detect_ci() -> (bool, Option<String>) {
    // Standard $CI variable
    if env::var("CI").is_ok() {
        let name = if env::var("GITHUB_ACTIONS").is_ok() {
            Some("GitHub Actions")
        } else if env::var("GITLAB_CI").is_ok() {
            Some("GitLab CI")
        } else if env::var("CIRCLECI").is_ok() {
            Some("CircleCI")
        } else if env::var("TRAVIS").is_ok() {
            Some("Travis CI")
        } else if env::var("JENKINS_URL").is_ok() {
            Some("Jenkins")
        } else if env::var("BUILDKITE").is_ok() {
            Some("Buildkite")
        } else {
            None
        };
        return (true, name.map(|s| s.to_string()));
    }
    (false, None)
}

fn print_table(info: &TermInfo) {
    println!("TTY          : {} ({})", info.is_tty, info.tty_device.as_deref().unwrap_or("?"));
    println!("Size         : {}x{}", 
        info.cols.map(|c| c.to_string()).unwrap_or("?".to_string()),
        info.rows.map(|r| r.to_string()).unwrap_or("?".to_string()));
    println!("Colors       : {:?} ({}-bit)", info.color_depth, info.color_depth_bits);
    println!("TERM         : {}", info.term.as_deref().unwrap_or("?"));
    println!("TERM_PROGRAM : {}", info.term_program.as_deref().unwrap_or("?"));
    println!("Shell        : {}", info.shell.as_deref().unwrap_or("?"));
    println!("Multiplexer  : {}", info.multiplexer.as_ref().map(|m| format!("{:?}", m)).unwrap_or("none".to_string()));
    println!("Editor       : {}", info.editor.as_deref().unwrap_or("?"));
    println!("Pager        : {}", info.pager.as_deref().unwrap_or("?"));
    println!("Unicode      : {}", info.unicode);
    println!("CI           : {} ({})", info.ci, info.ci_name.as_deref().unwrap_or("?"));
    println!("Interactive  : {}", info.interactive);
}
