mod network;
mod process;

use clap::Parser;
use network::collect_connections;

#[derive(Serialize)]
#[serde(untagged)]
enum NetworkResult {
    Available(Vec<network::Connection>),
    Unavailable { reason: String, suggestion: String },
    NotRequested,
}
use process::collect_processes;
use serde::Serialize;
use sysinfo::System;
use std::collections::HashMap;
use ux_output::{emit, OutMode};

#[derive(Parser)]
#[command(name = "px", about = "Structured process and network inspection for AI agents.", version)]
struct Cli {
    /// Filter by process name (substring match)
    #[arg(short, long)]
    name: Option<String>,

    /// Filter by PID
    #[arg(long)]
    pid: Option<u32>,

    /// Filter by port (show process using that port)
    #[arg(short, long)]
    port: Option<u16>,

    /// Show network connections
    #[arg(long)]
    network: bool,

    /// Only show listening ports
    #[arg(short, long)]
    listen: bool,

    /// Include environment variables (expensive)
    #[arg(short, long)]
    env: bool,

    /// Limit number of results
    #[arg(long, default_value_t = 50)]
    limit: usize,

    /// Show system summary (cpu, memory, load)
    #[arg(short, long)]
    system: bool,

    /// Output mode: auto (default), json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

#[derive(Serialize)]
struct SystemSummary {
    total_memory_bytes: u64,
    used_memory_bytes: u64,
    total_swap_bytes: u64,
    used_swap_bytes: u64,
    cpu_count: usize,
    global_cpu_percent: f32,
    load_avg_1m: f64,
    load_avg_5m: f64,
    load_avg_15m: f64,
    uptime_secs: u64,
}

#[derive(Serialize)]
struct Output {
    platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<SystemSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    processes: Option<Vec<process::ProcessEntry>>,
    connections: NetworkResult,
    count: usize,
}

fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();

    // `new_all()` already performs a full refresh; an extra `refresh_all()`
    // would redo the entire /proc scan for no benefit.
    let sys = System::new_all();

    // Build pid->name map for network correlation
    let pid_names: HashMap<u32, String> = sys
        .processes()
        .iter()
        .map(|(pid, p)| (pid.as_u32(), p.name().to_string_lossy().to_string()))
        .collect();

    // System summary
    let system_summary = if cli.system {
        let load = System::load_average();
        Some(SystemSummary {
            total_memory_bytes: sys.total_memory(),
            used_memory_bytes: sys.used_memory(),
            total_swap_bytes: sys.total_swap(),
            used_swap_bytes: sys.used_swap(),
            cpu_count: sys.cpus().len(),
            global_cpu_percent: sys.global_cpu_usage(),
            load_avg_1m: load.one,
            load_avg_5m: load.five,
            load_avg_15m: load.fifteen,
            uptime_secs: System::uptime(),
        })
    } else {
        None
    };

    // Processes
    let processes = if !cli.network || cli.name.is_some() || cli.pid.is_some() || cli.port.is_none() {
        let mut procs = collect_processes(&sys, cli.env);

        // Filter by name
        if let Some(ref name) = cli.name {
            let name_lower = name.to_lowercase();
            procs.retain(|p| p.name.to_lowercase().contains(&name_lower));
        }

        // Filter by pid
        if let Some(pid) = cli.pid {
            procs.retain(|p| p.pid == pid);
        }

        // Filter by port: find pid using that port, then filter
        if let Some(port) = cli.port {
            let conns = collect_connections(&pid_names);
            let pids_on_port: std::collections::HashSet<u32> = conns
                .iter()
                .filter(|c| c.local_port == port)
                .filter_map(|c| c.pid)
                .collect();
            procs.retain(|p| pids_on_port.contains(&p.pid));
        }

        procs.truncate(cli.limit);
        Some(procs)
    } else {
        None
    };

    // Network connections — Linux only via /proc/net
    let connections = if cli.network || cli.listen || cli.port.is_some() {
        if cfg!(target_os = "linux") {
            let mut conns = collect_connections(&pid_names);
            if cli.listen {
                conns.retain(|c| c.state.as_deref() == Some("LISTEN"));
            }
            if let Some(port) = cli.port {
                conns.retain(|c| c.local_port == port || c.remote_port == Some(port));
            }
            conns.truncate(cli.limit);
            NetworkResult::Available(conns)
        } else {
            NetworkResult::Unavailable {
                reason: format!(
                    "/proc/net is only available on Linux (current: {})", 
                    std::env::consts::OS
                ),
                suggestion: if cfg!(target_os = "macos") {
                    "use `lsof -i` or `netstat -an` on macOS".to_string()
                } else {
                    "use `netstat -ano` on Windows".to_string()
                },
            }
        }
    } else {
        NetworkResult::NotRequested
    };

    let count = processes.as_ref().map(|p| p.len()).unwrap_or(0)
        + if let NetworkResult::Available(ref c) = connections { c.len() } else { 0 };

    let output = Output {
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        system: system_summary,
        processes,
        connections,
        count,
    };

    let mode = OutMode::from_str(&cli.out);

    // ndjson streams the inner entries (processes, then connections), one per line.
    if cli.out == "ndjson" {
        if let Some(ref procs) = output.processes {
            for p in procs {
                println!("{}", serde_json::to_string(p).unwrap());
            }
        }
        if let NetworkResult::Available(ref conns) = output.connections {
            for c in conns {
                println!("{}", serde_json::to_string(c).unwrap());
            }
        }
        return;
    }

    if cli.out == "table" {
        print_table(&output);
        return;
    }

    emit(&output, &mode);
}

fn print_table(output: &Output) {
    if let Some(ref sys) = output.system {
        println!("cpu={:.1}%  mem={}/{} MB  load={:.2} {:.2} {:.2}  uptime={}s",
            sys.global_cpu_percent,
            sys.used_memory_bytes / 1_048_576,
            sys.total_memory_bytes / 1_048_576,
            sys.load_avg_1m, sys.load_avg_5m, sys.load_avg_15m,
            sys.uptime_secs);
    }
    if let Some(ref procs) = output.processes {
        println!("{:>8}  {:>6}  {:>10}  {}", "PID", "CPU%", "MEM(MB)", "NAME");
        for p in procs {
            println!("{:>8}  {:>6.1}  {:>10}  {}",
                p.pid, p.cpu_percent, p.memory_bytes / 1_048_576, p.name);
        }
    }
    if let NetworkResult::Available(ref conns) = output.connections {
        if !conns.is_empty() {
            println!("{:>6}  {:<22}  {:<22}  {:<10}  {}", "PROTO", "LOCAL", "REMOTE", "STATE", "PID");
            for c in conns {
                println!("{:>6}  {:<22}  {:<22}  {:<10}  {}",
                    c.protocol,
                    format!("{}:{}", c.local_addr, c.local_port),
                    c.remote_addr.as_deref().map(|a| format!("{}:{}", a, c.remote_port.unwrap_or(0))).unwrap_or_else(|| "-".into()),
                    c.state.as_deref().unwrap_or("-"),
                    c.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()));
            }
        }
    }
}
