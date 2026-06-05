#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

use clap::Parser;
use serde::Serialize;
use ux_output::{emit, is_linux, is_macos, unavail, MaybeAvailable, OutMode};

#[derive(Parser)]
#[command(
    name = "memx",
    about = "Structured memory inspection for AI agents.\nReplaces: /proc/smaps, vmmap, pmap",
    version
)]
struct Cli {
    /// PID to inspect
    pid: u32,

    /// Include full region list (default: summary only)
    #[arg(short, long)]
    regions: bool,

    /// Filter regions by kind: heap, stack, lib, anon, exe, file, text, data
    #[arg(short, long)]
    kind: Option<String>,

    /// Sort regions by: size (default), rss, pss
    #[arg(short, long, default_value = "size")]
    sort: String,

    /// Output mode: auto, json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

fn main() {
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);

    if is_linux() {
        #[cfg(target_os = "linux")]
        run_linux(&cli, &mode);
        #[cfg(not(target_os = "linux"))]
        unreachable!()
    } else if is_macos() {
        #[cfg(target_os = "macos")]
        run_macos(&cli, &mode);
        #[cfg(not(target_os = "macos"))]
        unreachable!()
    } else {
        let u = unavail::proc_fs();
        emit(&serde_json::json!({
            "unavailable": u
        }), &mode);
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run_linux(cli: &Cli, mode: &OutMode) {
    use linux::RegionKind;

    match linux::read_process_memory(cli.pid, cli.regions || cli.kind.is_some()) {
        Ok(mut mem) => {
            // Filter by kind
            if let Some(ref k) = cli.kind {
                let target = parse_linux_kind(k);
                mem.regions.retain(|r| r.kind == target);
            }

            // Sort
            match cli.sort.as_str() {
                "rss" => mem.regions.sort_by(|a, b| b.rss_bytes.cmp(&a.rss_bytes)),
                "pss" => mem.regions.sort_by(|a, b| b.pss_bytes.cmp(&a.pss_bytes)),
                _     => mem.regions.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes)),
            }

            if cli.out == "table" {
                print_linux_table(&mem);
            } else {
                emit(&mem, mode);
            }
        }
        Err(e) => {
            eprintln!("{}", serde_json::json!({"error": e, "pid": cli.pid}));
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "macos")]
fn run_macos(cli: &Cli, mode: &OutMode) {
    use macos::RegionKind;

    match macos::read_process_memory(cli.pid, cli.regions || cli.kind.is_some()) {
        Ok(mut mem) => {
            if let Some(ref k) = cli.kind {
                let target = parse_macos_kind(k);
                mem.regions.retain(|r| r.kind == target);
            }

            mem.regions.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

            if cli.out == "table" {
                print_macos_table(&mem);
            } else {
                emit(&mem, mode);
            }
        }
        Err(e) => {
            eprintln!("{}", serde_json::json!({"error": e, "pid": cli.pid}));
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
fn parse_linux_kind(s: &str) -> linux::RegionKind {
    match s.to_lowercase().as_str() {
        "heap"  => linux::RegionKind::Heap,
        "stack" => linux::RegionKind::Stack,
        "lib"   => linux::RegionKind::Lib,
        "exe"   => linux::RegionKind::Exe,
        "anon"  => linux::RegionKind::Anon,
        "vdso"  => linux::RegionKind::Vdso,
        "file"  => linux::RegionKind::File,
        _       => linux::RegionKind::Other,
    }
}

#[cfg(target_os = "macos")]
fn parse_macos_kind(s: &str) -> macos::RegionKind {
    match s.to_lowercase().as_str() {
        "heap"  => macos::RegionKind::Heap,
        "stack" => macos::RegionKind::Stack,
        "text"  => macos::RegionKind::Text,
        "data"  => macos::RegionKind::Data,
        "file"  => macos::RegionKind::MappedFile,
        "anon"  => macos::RegionKind::Anon,
        _       => macos::RegionKind::Other,
    }
}

#[cfg(target_os = "linux")]
fn print_linux_table(mem: &linux::ProcessMemory) {
    println!("PID:          {}", mem.pid);
    println!("Exe:          {}", mem.exe.as_deref().unwrap_or("?"));
    println!("VSS:          {}", human_bytes(mem.total_vss));
    println!("RSS:          {}", human_bytes(mem.total_rss));
    println!("PSS:          {}", human_bytes(mem.total_pss));
    println!("Private:      {}", human_bytes(mem.total_private));
    println!("Shared:       {}", human_bytes(mem.total_shared));
    println!("Swap:         {}", human_bytes(mem.total_swap));
    println!("Heap:         {}", human_bytes(mem.heap_bytes));
    println!("Stack:        {}", human_bytes(mem.stack_bytes));
    println!("Libs (PSS):   {}", human_bytes(mem.lib_bytes));
    println!("Anon:         {}", human_bytes(mem.anon_bytes));
    println!("Regions:      {}", mem.region_count);

    if !mem.regions.is_empty() {
        println!("\n{:<20} {:<8} {:<12} {:<12} {:<12} {}", "KIND", "PERMS", "SIZE", "RSS", "PSS", "PATH");
        println!("{}", "-".repeat(90));
        for r in &mem.regions {
            println!(
                "{:<20} {:<8} {:<12} {:<12} {:<12} {}",
                format!("{:?}", r.kind).to_lowercase(),
                r.permissions,
                human_bytes(r.size_bytes),
                human_bytes(r.rss_bytes),
                human_bytes(r.pss_bytes),
                r.backing_file.as_deref().unwrap_or("[anon]"),
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn print_macos_table(mem: &macos::ProcessMemory) {
    println!("PID:     {}", mem.pid);
    println!("Exe:     {}", mem.exe.as_deref().unwrap_or("?"));
    println!("VSS:     {}", human_bytes(mem.total_vss));
    println!("Dirty:   {}", human_bytes(mem.total_dirty));
    println!("Swap:    {}", human_bytes(mem.total_swap));
    println!("Heap:    {}", human_bytes(mem.heap_bytes));
    println!("Stack:   {}", human_bytes(mem.stack_bytes));
    println!("Text:    {}", human_bytes(mem.text_bytes));
    println!("Regions: {}", mem.region_count);
}

fn human_bytes(n: u64) -> String {
    match n {
        n if n >= 1024 * 1024 * 1024 => format!("{:.1}GB", n as f64 / (1024.0 * 1024.0 * 1024.0)),
        n if n >= 1024 * 1024        => format!("{:.1}MB", n as f64 / (1024.0 * 1024.0)),
        n if n >= 1024               => format!("{:.1}KB", n as f64 / 1024.0),
        n                            => format!("{}B", n),
    }
}
