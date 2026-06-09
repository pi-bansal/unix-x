mod bloom;
#[cfg(test)]
mod bloom_tests;
mod builder;
mod client;
mod columns;
#[cfg(test)]
mod columns_tests;
mod daemon;
mod query;

use clap::{Parser, Subcommand};
use client::{daemon_running, send_query, send_rebuild, send_status};
use daemon::Response;
use query::Query;
use serde::Serialize;
use std::path::PathBuf;
use ux_output::{emit, OutMode};

#[derive(Parser)]
#[command(
    name = "idx",
    about = "Persistent columnar filesystem index with bloom filter.\nMakes lx instant on large repos.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start the index daemon for a directory
    Start {
        /// Directory to index and watch (default: current dir)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Don't respect .gitignore
        #[arg(long)]
        no_gitignore: bool,
    },

    /// Query the index (daemon must be running)
    Query {
        /// Directory whose index to query (default: current dir)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Filter by extension (e.g. rs, ts, json)
        #[arg(short, long)]
        ext: Option<String>,

        /// Filter by git status (clean, modified, added, untracked, deleted)
        #[arg(short, long)]
        git: Option<String>,

        /// Minimum file size in bytes
        #[arg(long)]
        size_gt: Option<u64>,

        /// Maximum file size in bytes
        #[arg(long)]
        size_lt: Option<u64>,

        /// Modified after unix timestamp
        #[arg(long)]
        mtime_gt: Option<u64>,

        /// Path must contain this substring
        #[arg(short, long)]
        filter: Option<String>,

        /// Only directories
        #[arg(short, long)]
        dirs: bool,

        /// Only files
        #[arg(short = 'f', long)]
        files: bool,

        /// Max results
        #[arg(short, long)]
        limit: Option<usize>,

        /// Sort: path (default), size, mtime
        #[arg(short, long)]
        sort: Option<String>,

        /// Output mode: auto, json, pretty, table, ndjson
        #[arg(short, long, default_value = "auto")]
        out: String,
    },

    /// Show daemon status
    Status {
        #[arg(default_value = ".")]
        path: PathBuf,

        #[arg(short, long, default_value = "auto")]
        out: String,
    },

    /// Trigger a full index rebuild
    Rebuild {
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Build index once and query without a daemon (useful for scripts)
    Once {
        #[arg(default_value = ".")]
        path: PathBuf,

        #[arg(short, long)]
        ext: Option<String>,

        #[arg(short, long)]
        git: Option<String>,

        #[arg(long)]
        size_gt: Option<u64>,

        #[arg(long)]
        size_lt: Option<u64>,

        #[arg(short, long)]
        filter: Option<String>,

        #[arg(short, long)]
        dirs: bool,

        #[arg(short = 'f', long)]
        files: bool,

        #[arg(short, long)]
        limit: Option<usize>,

        #[arg(short, long)]
        sort: Option<String>,

        #[arg(short, long, default_value = "auto")]
        out: String,
    },
}

#[tokio::main]
async fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();

    match cli.command {
        Cmd::Start { path, no_gitignore } => {
            let root = path.canonicalize().unwrap_or(path);
            if daemon_running(&root) {
                eprintln!("[idx] Daemon already running for {}", root.display());
                std::process::exit(1);
            }
            daemon::run_daemon(root, !no_gitignore).await;
        }

        Cmd::Query {
            path, ext, git, size_gt, size_lt, mtime_gt,
            filter, dirs, files, limit, sort, out,
        } => {
            let root = path.canonicalize().unwrap_or(path);
            let mode = OutMode::from_str(&out);

            let q = Query {
                ext,
                git_status: git,
                size_gt,
                size_lt,
                mtime_gt,
                path_contains: filter,
                dirs_only: dirs,
                files_only: files,
                limit,
                sort,
            };

            match send_query(&root, q) {
                Ok(Response::QueryResult(r)) => {
                    if out == "ndjson" {
                        for e in &r.entries {
                            println!("{}", serde_json::to_string(e).unwrap());
                        }
                    } else {
                        emit(&r, &mode);
                    }
                }
                Ok(_) => eprintln!("unexpected response"),
                Err(e) => {
                    eprintln!("{}", serde_json::json!({"error": e.to_string()}));
                    std::process::exit(1);
                }
            }
        }

        Cmd::Status { path, out } => {
            let root = path.canonicalize().unwrap_or(path);
            let mode = OutMode::from_str(&out);
            match send_status(&root) {
                Ok(Response::Status(s)) => emit(&s, &mode),
                Ok(_) => {}
                Err(e) => {
                    eprintln!("{}", serde_json::json!({"error": e.to_string()}));
                    std::process::exit(1);
                }
            }
        }

        Cmd::Rebuild { path } => {
            let root = path.canonicalize().unwrap_or(path);
            match send_rebuild(&root) {
                Ok(_) => eprintln!("[idx] Rebuild triggered"),
                Err(e) => {
                    eprintln!("{}", serde_json::json!({"error": e.to_string()}));
                    std::process::exit(1);
                }
            }
        }

        Cmd::Once {
            path, ext, git, size_gt, size_lt,
            filter, dirs, files, limit, sort, out,
        } => {
            let root = path.canonicalize().unwrap_or(path);
            let mode = OutMode::from_str(&out);

            let start = std::time::Instant::now();
            let build = builder::build_index(&root, true);
            let build_ms = start.elapsed().as_millis();

            let q = Query {
                ext,
                git_status: git,
                size_gt,
                size_lt,
                mtime_gt: None,
                path_contains: filter,
                dirs_only: dirs,
                files_only: files,
                limit,
                sort,
            };

            let result = query::run_query(&build.index, &build.bloom, &q);

            #[derive(Serialize)]
            struct OnceOutput {
                build_ms: u128,
                scan_ms: u64,
                count: usize,
                entries: Vec<columns::IndexEntry>,
            }

            let output = OnceOutput {
                build_ms,
                scan_ms: result.scan_ms,
                count: result.count,
                entries: result.entries,
            };

            if out == "ndjson" {
                for e in &output.entries {
                    println!("{}", serde_json::to_string(e).unwrap());
                }
            } else {
                emit(&output, &mode);
            }
        }
    }
}
