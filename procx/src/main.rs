mod cron;
mod launchd;
mod systemd;

use clap::Parser;
use serde::Serialize;
use ux_output::{emit, has_command, is_linux, is_macos, unavail, MaybeAvailable, OutMode};

#[derive(Parser)]
#[command(
    name = "procx",
    about = "Structured cron/systemd/launchd job inspection for AI agents.",
    version
)]
struct Cli {
    #[arg(short, long)]
    filter: Option<String>,

    /// Only inspect one source: cron, systemd, launchd
    #[arg(short, long)]
    source: Option<String>,

    #[arg(short, long)]
    active: bool,

    #[arg(long)]
    failed: bool,

    /// Output mode: auto (default), json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

#[derive(Serialize)]
struct Output {
    platform: String,
    cron: MaybeAvailable<cron::CronJob>,
    systemd: MaybeAvailable<systemd::SystemdUnit>,
    launchd: MaybeAvailable<launchd::LaunchdJob>,
}

fn main() {
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);
    let want = cli.source.as_deref();

    // ── cron ──────────────────────────────────────────────────────────────────
    let mut cron = if want.map(|s| s == "cron").unwrap_or(true) {
        if is_linux() || is_macos() {
            let mut jobs = cron::collect_cron();
            if let Some(ref f) = cli.filter {
                let fl = f.to_lowercase();
                jobs.retain(|j| j.command.to_lowercase().contains(&fl));
            }
            MaybeAvailable::available(jobs)
        } else {
            MaybeAvailable::unavailable(
                "cron",
                "cron is not available on Windows",
                Some("use Task Scheduler or WSL for cron-like scheduling"),
            )
        }
    } else {
        MaybeAvailable::available(vec![])
    };

    // ── systemd ───────────────────────────────────────────────────────────────
    let mut systemd = if want.map(|s| s == "systemd").unwrap_or(true) {
        if is_linux() && has_command("systemctl") {
            let mut units = systemd::collect_systemd();
            if let Some(ref f) = cli.filter {
                let fl = f.to_lowercase();
                units.retain(|u| {
                    u.name.to_lowercase().contains(&fl)
                        || u.description.to_lowercase().contains(&fl)
                });
            }
            if cli.active {
                units.retain(|u| matches!(u.active_state, systemd::UnitState::Active));
            }
            if cli.failed {
                units.retain(|u| matches!(u.active_state, systemd::UnitState::Failed));
            }
            MaybeAvailable::available(units)
        } else if is_linux() {
            MaybeAvailable::unavailable(
                "systemd_units",
                "systemctl not found — this Linux system may not use systemd",
                Some("check your init system with: ps -p 1 -o comm="),
            )
        } else {
            MaybeAvailable::unavailable_from(unavail::systemd())
        }
    } else {
        MaybeAvailable::available(vec![])
    };

    // ── launchd ───────────────────────────────────────────────────────────────
    let mut launchd = if want.map(|s| s == "launchd").unwrap_or(true) {
        if is_macos() && has_command("launchctl") {
            let mut jobs = launchd::collect_launchd();
            if let Some(ref f) = cli.filter {
                let fl = f.to_lowercase();
                jobs.retain(|j| j.label.to_lowercase().contains(&fl));
            }
            if cli.active {
                jobs.retain(|j| j.pid.is_some());
            }
            if cli.failed {
                jobs.retain(|j| j.last_exit_status.map(|s| s != 0).unwrap_or(false));
            }
            MaybeAvailable::available(jobs)
        } else if is_macos() {
            MaybeAvailable::unavailable(
                "launchd_jobs",
                "launchctl not found",
                Some("launchctl should be present on all macOS systems — check your PATH"),
            )
        } else {
            MaybeAvailable::unavailable_from(unavail::launchd())
        }
    } else {
        MaybeAvailable::available(vec![])
    };

    let output = Output {
        platform: ux_output::current_platform(),
        cron,
        systemd,
        launchd,
    };

    emit(&output, &mode);
}
