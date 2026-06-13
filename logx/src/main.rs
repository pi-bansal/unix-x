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
    truncated: bool,
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

struct Filters {
    level_filter: Option<String>,
    min_level_rank: Option<u8>,
    grep_lower: Option<String>,
    regex_filter: Option<regex::Regex>,
    raw: bool,
}

#[derive(Default)]
struct Counts {
    total_lines: u64,
    error_count: u64,
    warn_count: u64,
    info_count: u64,
    debug_count: u64,
}

/// Parse one line, update level counts, and apply filters. Returns `None`
/// for empty/filtered-out lines (and for lines that don't count toward
/// `total_lines`).
fn process_line(line: &str, line_num: u64, format: &detect::LogFormat, filters: &Filters, counts: &mut Counts) -> Option<LogEntry> {
    if line.trim().is_empty() {
        return None;
    }
    counts.total_lines += 1;

    let mut entry = parse_line(line, line_num, format);

    match entry.level.as_deref() {
        Some("error") => counts.error_count += 1,
        Some("warn") => counts.warn_count += 1,
        Some("info") => counts.info_count += 1,
        Some("debug") => counts.debug_count += 1,
        _ => {}
    }

    if let Some(ref lf) = filters.level_filter {
        if entry.level.as_deref() != Some(lf.as_str()) {
            return None;
        }
    }

    if let Some(min_rank) = filters.min_level_rank {
        let entry_rank = entry.level.as_deref().map(level_rank).unwrap_or(0);
        if entry_rank < min_rank {
            return None;
        }
    }

    if let Some(ref g) = filters.grep_lower {
        if !entry.message.to_lowercase().contains(g.as_str())
            && !entry.raw.to_lowercase().contains(g.as_str())
        {
            return None;
        }
    }

    if let Some(ref re) = filters.regex_filter {
        if !re.is_match(&entry.raw) {
            return None;
        }
    }

    if !filters.raw {
        entry.raw = String::new();
    }

    Some(entry)
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

    // Build filters
    let filters = Filters {
        level_filter: cli.level.as_deref().map(|l| l.to_lowercase()),
        min_level_rank: cli.min_level.as_deref().map(level_rank),
        grep_lower: cli.grep.as_deref().map(|g| g.to_lowercase()),
        regex_filter: cli.regex.as_deref().and_then(|r| regex::Regex::new(r).ok()),
        raw: cli.raw,
    };

    let format_override = match cli.format.as_deref() {
        Some("json") => Some(detect::LogFormat::Json),
        Some("logfmt") => Some(detect::LogFormat::Logfmt),
        Some("nginx") => Some(detect::LogFormat::Nginx),
        Some("syslog") => Some(detect::LogFormat::Syslog),
        Some("rails") => Some(detect::LogFormat::Rails),
        Some("go") => Some(detect::LogFormat::Go),
        _ => None,
    };

    let mut entries: Vec<LogEntry> = Vec::new();
    let mut counts = Counts::default();
    let mut truncated = false;
    let format_name;

    // `--tail` and `--stats` both need the whole file: tail to find where
    // the last N lines start, stats to count every line. Everything else
    // can stream and stop as soon as `--limit` matches are found, so a huge
    // log file doesn't get fully loaded into memory just to return 20 lines.
    if cli.tail.is_some() || cli.stats {
        let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

        let sample = all_lines.iter().find(|l| !l.trim().is_empty()).map(|s| s.as_str()).unwrap_or("");
        let format = format_override.unwrap_or_else(|| detect_format(sample));
        format_name = format!("{:?}", format).to_lowercase();

        // Track where the slice begins in the original input so emitted line
        // numbers stay file-absolute under --tail/--head instead of restarting at 1.
        let (line_offset, lines_slice): (usize, &[String]) = match (cli.tail, cli.head) {
            (Some(n), _) => {
                let start = all_lines.len().saturating_sub(n);
                (start, &all_lines[start..])
            }
            (_, Some(n)) => (0, &all_lines[..n.min(all_lines.len())]),
            _ => (0, &all_lines),
        };

        for (i, line) in lines_slice.iter().enumerate() {
            if let Some(entry) = process_line(line, (line_offset + i) as u64 + 1, &format, &filters, &mut counts) {
                if entries.len() < cli.limit {
                    entries.push(entry);
                } else {
                    truncated = true;
                }
            }
        }
    } else {
        let mut lines_iter = reader.lines().map_while(Result::ok);

        // Buffer a small lookahead for format detection without reading the
        // whole file.
        let buffer: Vec<String> = lines_iter.by_ref().take(64).collect();
        let sample = buffer.iter().find(|l| !l.trim().is_empty()).map(|s| s.as_str()).unwrap_or("");
        let format = format_override.unwrap_or_else(|| detect_format(sample));
        format_name = format!("{:?}", format).to_lowercase();

        let mut idx: u64 = 0;
        'lines: for line in buffer.into_iter().chain(lines_iter) {
            idx += 1;
            if let Some(max) = cli.head {
                if idx > max as u64 {
                    break 'lines;
                }
            }
            if let Some(entry) = process_line(&line, idx, &format, &filters, &mut counts) {
                entries.push(entry);
                if entries.len() >= cli.limit {
                    truncated = true;
                    break 'lines;
                }
            }
        }
    }

    let stats = if cli.stats {
        Some(Stats {
            total_lines: counts.total_lines,
            matched_lines: entries.len() as u64,
            error_count: counts.error_count,
            warn_count: counts.warn_count,
            info_count: counts.info_count,
            debug_count: counts.debug_count,
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
    let output = Output { stats, count, truncated, entries };

    if cli.out == "table" {
        println!("{:>6}  {:<6}  {}", "LINE", "LEVEL", "MESSAGE");
        for e in &output.entries {
            println!("{:>6}  {:<6}  {}", e.line, e.level.as_deref().unwrap_or("-"), e.message);
        }
        return;
    }

    emit(&output, &mode);
}
