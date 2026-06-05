mod merge;
#[cfg(test)]
mod merge_tests;

use clap::Parser;
use merge::three_way_merge;
use serde::Serialize;
use std::path::PathBuf;
use ux_output::{emit, OutMode};

#[derive(Parser)]
#[command(
    name = "diffx",
    about = "Structured three-way merge for AI agents.\nNo text conflict markers — conflicts are structured data.",
    long_about = "
Three-way merge: given BASE, OURS, THEIRS — produces structured JSON output.

  diffx base.rs ours.rs theirs.rs

Auto-resolves:
  - Identical changes on both sides
  - One side unchanged (accept the modified side)

Outputs conflicts as typed objects, not <<<< ==== >>>> markers.
Exit code 0 = clean merge, 1 = conflicts remain.
",
    version
)]
struct Cli {
    /// Base file (common ancestor)
    base: PathBuf,

    /// Our version
    ours: PathBuf,

    /// Their version
    theirs: PathBuf,

    /// Write resolved output to this file (only if clean merge)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Show only conflicts (skip unchanged and single-side hunks)
    #[arg(short, long)]
    conflicts_only: bool,

    /// Show summary only (no hunks)
    #[arg(short, long)]
    summary: bool,

    /// Output mode: auto, json, pretty, table, ndjson
    #[arg(long, default_value = "auto")]
    out: String,
}

#[derive(Serialize)]
struct Output {
    clean: bool,
    conflict_count: usize,
    auto_resolved_count: usize,
    ours_only_count: usize,
    theirs_only_count: usize,
    base_path: String,
    ours_path: String,
    theirs_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    hunks: Vec<merge::MergeHunk>,
}

fn main() {
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);

    let base   = read_file(&cli.base);
    let ours   = read_file(&cli.ours);
    let theirs = read_file(&cli.theirs);

    let mut result = three_way_merge(&base, &ours, &theirs);

    // Write resolved file if requested and clean
    if let (Some(ref out_path), Some(ref resolved)) = (&cli.output, &result.resolved) {
        std::fs::write(out_path, resolved).unwrap_or_else(|e| {
            eprintln!("{}", serde_json::json!({"error": e.to_string()}));
        });
    }

    // Filter hunks
    if cli.conflicts_only {
        result.hunks.retain(|h| h.kind == merge::HunkKind::Conflict);
    }

    let hunks = if cli.summary { vec![] } else { result.hunks };

    let output = Output {
        clean: result.clean,
        conflict_count: result.conflict_count,
        auto_resolved_count: result.auto_resolved_count,
        ours_only_count: result.ours_only_count,
        theirs_only_count: result.theirs_only_count,
        base_path: cli.base.to_string_lossy().to_string(),
        ours_path: cli.ours.to_string_lossy().to_string(),
        theirs_path: cli.theirs.to_string_lossy().to_string(),
        resolved: if cli.summary { None } else { result.resolved },
        hunks,
    };

    if cli.out == "table" {
        println!("Base  : {}", output.base_path);
        println!("Ours  : {}", output.ours_path);
        println!("Theirs: {}", output.theirs_path);
        println!("Clean : {}", output.clean);
        println!("Conflicts      : {}", output.conflict_count);
        println!("Auto-resolved  : {}", output.auto_resolved_count);
        println!("Ours-only hunks: {}", output.ours_only_count);
        println!("Their-only hunks: {}", output.theirs_only_count);
    } else {
        emit(&output, &mode);
    }

    std::process::exit(if output.clean { 0 } else { 1 });
}

fn read_file(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{}", serde_json::json!({"error": e.to_string(), "path": path.to_string_lossy()}));
        std::process::exit(1);
    })
}
