mod diff;

use clap::Parser;
use diff::{diff_texts, is_binary, FileDiff};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "dx", about = "Structured diff with JSON output for AI agents.", version)]
struct Cli {
    /// Old file or directory
    old: PathBuf,

    /// New file or directory
    new: PathBuf,

    /// Context lines around each change
    #[arg(short = 'c', long, default_value_t = 3)]
    context: usize,

    /// Exclude equal (unchanged) lines from output
    #[arg(long)]
    no_equal: bool,

    /// Pretty-print output
    #[arg(short, long)]
    pretty: bool,

    /// Newline-delimited JSON (one FileDiff per line)
    #[arg(long)]
    ndjson: bool,

    /// Show summary only (no hunks)
    #[arg(short, long)]
    summary: bool,
}

#[derive(Serialize)]
struct Output {
    total_files: usize,
    total_added: u32,
    total_removed: u32,
    files: Vec<FileDiff>,
}

fn main() {
    let cli = Cli::parse();

    let old_path = &cli.old;
    let new_path = &cli.new;

    let mut file_diffs: Vec<FileDiff> = Vec::new();

    if old_path.is_dir() && new_path.is_dir() {
        // Directory diff: walk both, diff matching files
        diff_dirs(old_path, new_path, &cli, &mut file_diffs);
    } else {
        // Single file diff
        if let Some(fd) = diff_files(
            old_path.to_str().unwrap_or("old"),
            new_path.to_str().unwrap_or("new"),
            old_path,
            new_path,
            &cli,
        ) {
            file_diffs.push(fd);
        }
    }

    // Apply no-equal filter
    if cli.no_equal {
        for fd in &mut file_diffs {
            for hunk in &mut fd.hunks {
                hunk.changes.retain(|c| c.kind != diff::ChangeKind::Equal);
            }
        }
    }

    // Apply summary mode (drop hunks)
    if cli.summary {
        for fd in &mut file_diffs {
            fd.hunks.clear();
        }
    }

    let total_added: u32 = file_diffs.iter().map(|f| f.added_lines).sum();
    let total_removed: u32 = file_diffs.iter().map(|f| f.removed_lines).sum();
    let total_files = file_diffs.len();

    if cli.ndjson {
        for fd in &file_diffs {
            println!("{}", serde_json::to_string(fd).unwrap());
        }
        return;
    }

    let output = Output {
        total_files,
        total_added,
        total_removed,
        files: file_diffs,
    };

    let json = if cli.pretty {
        serde_json::to_string_pretty(&output).unwrap()
    } else {
        serde_json::to_string(&output).unwrap()
    };

    println!("{}", json);
}

fn diff_files(
    old_label: &str,
    new_label: &str,
    old_path: &Path,
    new_path: &Path,
    cli: &Cli,
) -> Option<FileDiff> {
    let old_bytes = std::fs::read(old_path).unwrap_or_default();
    let new_bytes = std::fs::read(new_path).unwrap_or_default();

    if is_binary(&old_bytes) || is_binary(&new_bytes) {
        return Some(FileDiff {
            old_path: old_label.to_string(),
            new_path: new_label.to_string(),
            added_lines: 0,
            removed_lines: 0,
            hunks: vec![],
            binary: Some(true),
            rename: None,
        });
    }

    let old_str = String::from_utf8_lossy(&old_bytes);
    let new_str = String::from_utf8_lossy(&new_bytes);

    // Skip identical files
    if old_str == new_str {
        return None;
    }

    Some(diff_texts(old_label, new_label, &old_str, &new_str, cli.context))
}

fn diff_dirs(old_dir: &Path, new_dir: &Path, cli: &Cli, out: &mut Vec<FileDiff>) {
    use ignore::WalkBuilder;
    use std::collections::BTreeSet;

    // Collect relative paths from both sides
    let old_files = collect_relative_paths(old_dir);
    let new_files = collect_relative_paths(new_dir);

    let all_paths: BTreeSet<PathBuf> = old_files.union(&new_files).cloned().collect();

    for rel in all_paths {
        let old_abs = old_dir.join(&rel);
        let new_abs = new_dir.join(&rel);

        let old_label = format!("a/{}", rel.display());
        let new_label = format!("b/{}", rel.display());

        let fd = match (old_abs.exists(), new_abs.exists()) {
            (true, true) => diff_files(&old_label, &new_label, &old_abs, &new_abs, cli),
            (true, false) => {
                // Deleted file
                let bytes = std::fs::read(&old_abs).unwrap_or_default();
                if is_binary(&bytes) {
                    Some(FileDiff { old_path: old_label, new_path: new_label, added_lines: 0, removed_lines: 0, hunks: vec![], binary: Some(true), rename: None })
                } else {
                    let s = String::from_utf8_lossy(&bytes);
                    Some(diff_texts(&old_label, &new_label, &s, "", cli.context))
                }
            }
            (false, true) => {
                // Added file
                let bytes = std::fs::read(&new_abs).unwrap_or_default();
                if is_binary(&bytes) {
                    Some(FileDiff { old_path: old_label, new_path: new_label, added_lines: 0, removed_lines: 0, hunks: vec![], binary: Some(true), rename: None })
                } else {
                    let s = String::from_utf8_lossy(&bytes);
                    Some(diff_texts(&old_label, &new_label, "", &s, cli.context))
                }
            }
            _ => None,
        };

        if let Some(fd) = fd {
            out.push(fd);
        }
    }
}

fn collect_relative_paths(dir: &Path) -> std::collections::BTreeSet<PathBuf> {
    let mut paths = std::collections::BTreeSet::new();
    if let Ok(entries) = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter(|e| e.as_ref().map(|e| !e.file_type().is_dir()).unwrap_or(false))
        .collect::<Result<Vec<_>, _>>()
    {
        for e in entries {
            if let Ok(rel) = e.path().strip_prefix(dir) {
                paths.insert(rel.to_path_buf());
            }
        }
    }
    paths
}
