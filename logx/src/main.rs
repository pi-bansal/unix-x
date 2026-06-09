mod detect;
#[cfg(test)]
mod tests;
mod parse;

use clap::Parser;
use detect::detect_format;
use flate2::read::GzDecoder;
use parse::{parse_line, LogEntry};
use serde::Serialize;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::PathBuf;
use ux_output::{emit, OutMode};

#[derive(Parser)]
#[command(name = "logx", about = "Structured log querying for AI agents.", version)]
struct Cli {
    /// Log file path (default: stdin)
    #[arg()]
    file: Option<PathBuf>,

    /// Filter by level: error, warn, info, debug
    #[arg(short, long)]
    level: Option<String>,

    /// Minimum level (error > warn > info > debug)
    #[arg(long)]
    min_level: Option<String>,

    /// Filter lines containing this string (case-insensitive)
    #[arg(short, long)]
    grep: Option<String>,

    /// Filter by regex pattern
    #[arg(short, long)]
    regex: Option<String>,

    /// Show last N lines (like tail)
    #[arg(short, long)]
    tail: Option<usize>,

    /// Show first N lines (like head)
    #[arg(long)]
    head: Option<usize>,

    /// Max results to emit
    #[arg(long, default_value_t = 1000)]
    limit: usize,

    /// Force format detection (json, logfmt, nginx, syslog, rails, go, plain)
    #[arg(short, long)]
    format: Option<String>,

    /// Output mode: auto (default), json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,

    /// Emit summary stats only
    #[arg(short, long)]
    stats: bool,

    /// Include raw line in output
    #[arg(long)]
    raw: bool,
}

#[derive(Serialize)]
struct Stats {
    total_lines: u64,
    matched_lines: u64,
    error_count: u64,
    warn_count: u64,
    info_count: u64,
    debug_count: u64,
    format_detected: String,
}

#[derive(Serialize)]
struct Output {
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<Stats>,
    count: usize,
    entries: Vec<LogEntry>,
}

fn level_rank(level: &str) -> u8 {
    match level {
        "error" => 4,
        "warn" => 3,
        "info" => 2,
        "debug" => 1,
        _ => 0,
    }
}

fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();

    // Open input
    let reader: Box<dyn BufRead> = match &cli.file {
        Some(path) => {
            let f = File::open(path).unwrap_or_else(|e| {
                eprintln!("{{\"error\": \"{}\"}}", e);
                std::process::exit(1);
            });
            if path.extension().map(|e| e == "gz").unwrap_or(false) {
                Box::new(BufReader::new(GzDecoder::new(f)))
            } else {
                Box::new(BufReader::new(f))
            }
        }
        None => Box::new(BufReader::new(io::stdin())),
    };

    // Read all lines for tail support; otherwise stream
    let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

    // Detect format from first non-empty line
    let sample = all_lines.iter().find(|l| !l.trim().is_empty()).map(|s| s.as_str()).unwrap_or("");
    let format = match cli.format.as_deref() {
        Some("json") => detect::LogFormat::Json,
        Some("logfmt") => detect::LogFormat::Logfmt,
        Some("nginx") => detect::LogFormat::Nginx,
        Some("syslog") => detect::LogFormat::Syslog,
        Some("rails") => detect::LogFormat::Rails,
        Some("go") => detect::LogFormat::Go,
        _ => detect_format(sample),
    };
    let format_name = format!("{:?}", format).to_lowercase();

    // Apply tail/head slicing
    // Track where the slice begins in the original input so emitted line numbers
    // stay file-absolute under --tail/--head instead of restarting at 1.
    let (line_offset, lines_slice): (usize, &[String]) = match (cli.tail, cli.head) {
        (Some(n), _) => {
            let start = all_lines.len().saturating_sub(n);
            (start, &all_lines[start..])
        }
        (_, Some(n)) => (0, &all_lines[..n.min(all_lines.len())]),
        _ => (0, &all_lines),
    };

    // Build filters
    let level_filter = cli.level.as_deref().map(|l| l.to_lowercase());
    let min_level_rank = cli.min_level.as_deref().map(level_rank);
    let grep_lower = cli.grep.as_deref().map(|g| g.to_lowercase());
    let regex_filter = cli.regex.as_deref().and_then(|r| regex::Regex::new(r).ok());

    // Parse and filter
    let mut entries: Vec<LogEntry> = Vec::new();
    let mut total_lines: u64 = 0;
    let mut error_count = 0u64;
    let mut warn_count = 0u64;
    let mut info_count = 0u64;
    let mut debug_count = 0u64;

    for (i, line) in lines_slice.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        total_lines += 1;

        let mut entry = parse_line(line, (line_offset + i) as u64 + 1, &format);

        // Count by level
        match entry.level.as_deref() {
            Some("error") => error_count += 1,
            Some("warn") => warn_count += 1,
            Some("info") => info_count += 1,
            Some("debug") => debug_count += 1,
            _ => {}
        }

        // Level filter
        if let Some(ref lf) = level_filter {
            if entry.level.as_deref() != Some(lf.as_str()) {
                continue;
            }
        }

        // Min level filter
        if let Some(min_rank) = min_level_rank {
            let entry_rank = entry.level.as_deref().map(level_rank).unwrap_or(0);
            if entry_rank < min_rank {
                continue;
            }
        }

        // Grep filter
        if let Some(ref g) = grep_lower {
            if !entry.message.to_lowercase().contains(g.as_str())
                && !entry.raw.to_lowercase().contains(g.as_str())
            {
                continue;
            }
        }

        // Regex filter
        if let Some(ref re) = regex_filter {
            if !re.is_match(&entry.raw) {
                continue;
            }
        }

        // Strip raw unless requested
        if !cli.raw {
            entry.raw = String::new();
        }

        entries.push(entry);

        if entries.len() >= cli.limit {
            break;
        }
    }

    let stats = if cli.stats {
        Some(Stats {
            total_lines,
            matched_lines: entries.len() as u64,
            error_count,
            warn_count,
            info_count,
            debug_count,
            format_detected: format_name,
        })
    } else {
        None
    };

    let mode = OutMode::from_str(&cli.out);

    if cli.stats && cli.out != "ndjson" {
        // Stats-only mode: emit just the stats object.
        emit(stats.as_ref().unwrap(), &mode);
        return;
    }

    // ndjson streams one entry per line.
    if cli.out == "ndjson" {
        for entry in &entries {
            println!("{}", serde_json::to_string(entry).unwrap());
        }
        return;
    }

    let count = entries.len();
    let output = Output { stats, count, entries };

    if cli.out == "table" {
        println!("{:>6}  {:<6}  {}", "LINE", "LEVEL", "MESSAGE");
        for e in &output.entries {
            println!("{:>6}  {:<6}  {}", e.line, e.level.as_deref().unwrap_or("-"), e.message);
        }
        return;
    }

    emit(&output, &mode);
}
