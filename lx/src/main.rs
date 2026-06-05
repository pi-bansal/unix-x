mod entry;
mod git;
mod walk;

use clap::Parser;
use serde::Serialize;
use std::path::PathBuf;
use ux_output::{emit, OutMode};

use entry::Entry;
use git::GitIndex;
use walk::{walk, WalkOptions};

#[derive(Parser)]
#[command(
    name = "lx",
    about = "Structured filesystem metadata. Designed for AI agents.",
    version
)]
struct Cli {
    /// Path to inspect (default: current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Max traversal depth
    #[arg(short, long, default_value_t = 2)]
    depth: usize,

    /// Show hidden files and directories
    #[arg(short = 'a', long)]
    all: bool,

    /// Don't respect .gitignore
    #[arg(long)]
    no_gitignore: bool,

    /// Skip git status (faster)
    #[arg(long)]
    no_git: bool,

    /// Only show files (no directories)
    #[arg(short, long)]
    files_only: bool,

    /// Filter by extension (e.g. rs, ts, json)
    #[arg(short, long)]
    ext: Option<String>,

    /// Output mode: auto (default), json, pretty, table, ndjson
    /// auto → pretty when stdout is a terminal, compact when piped
    #[arg(short, long, default_value = "auto")]
    out: String,
}

#[derive(Serialize)]
struct Output {
    root: String,
    depth: usize,
    count: usize,
    entries: Vec<Entry>,
}

#[derive(Serialize)]
struct ErrorOutput {
    error: String,
    path: String,
}

fn main() {
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);

    // Resolve path
    let root = match cli.path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            let err = ErrorOutput {
                error: e.to_string(),
                path: cli.path.to_string_lossy().to_string(),
            };
            eprintln!("{}", serde_json::to_string(&err).unwrap());
            std::process::exit(1);
        }
    };

    // Load git index unless disabled
    let git_index = if cli.no_git {
        None
    } else {
        GitIndex::load(&root)
    };

    let walk_opts = WalkOptions {
        depth: cli.depth,
        show_hidden: cli.all,
        respect_gitignore: !cli.no_gitignore,
        include_dirs: !cli.files_only,
    };

    let mut entries = walk(&root, &walk_opts, git_index.as_ref());

    // Filter by extension if requested
    if let Some(ref ext) = cli.ext {
        let ext = ext.trim_start_matches('.');
        entries.retain(|e| {
            e.extension
                .as_deref()
                .map(|e2| e2 == ext)
                .unwrap_or(false)
        });
    }

    let count = entries.len();

    // ndjson mode: stream entries directly
    if cli.out == "ndjson" {
        for entry in &entries {
            println!("{}", serde_json::to_string(entry).unwrap());
        }
        return;
    }

    let output = Output {
        root: root.to_string_lossy().to_string(),
        depth: cli.depth,
        count,
        entries,
    };

    emit(&output, &mode);
}
