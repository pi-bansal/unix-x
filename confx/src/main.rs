mod parse;

use clap::Parser;
use serde::Serialize;
use std::path::PathBuf;
use ux_output::{emit, OutMode};

#[derive(Parser)]
#[command(
    name = "confx",
    about = "Structured config reader for AI agents.\nParses YAML, TOML, INI, .properties (and JSON) into JSON.",
    version
)]
struct Cli {
    /// Config files to parse
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Force input format: yaml, toml, ini, properties, json (default: detect by extension)
    #[arg(short, long)]
    format: Option<String>,

    /// Emit just the parsed data (single file) — clean for piping into jsonx
    #[arg(short, long)]
    raw: bool,

    /// Output mode: auto, json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

#[derive(Serialize)]
struct FileConfig {
    path: String,
    format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct Output {
    count: usize,
    files: Vec<FileConfig>,
}

fn parse_one(path: &PathBuf, forced: &Option<String>) -> FileConfig {
    let path_str = path.to_string_lossy().to_string();

    let format = match forced {
        Some(f) => f.to_lowercase(),
        None => match parse::detect_format(path) {
            Some(f) => f.to_string(),
            None => {
                return FileConfig {
                    path: path_str,
                    format: "unknown".to_string(),
                    data: None,
                    error: Some(
                        "could not detect format from extension; pass --format".to_string(),
                    ),
                };
            }
        },
    };

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return FileConfig {
                path: path_str,
                format,
                data: None,
                error: Some(e.to_string()),
            }
        }
    };

    match parse::parse(&content, &format) {
        Ok(data) => FileConfig {
            path: path_str,
            format,
            data: Some(data),
            error: None,
        },
        Err(e) => FileConfig {
            path: path_str,
            format,
            data: None,
            error: Some(e),
        },
    }
}

fn main() {
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);

    let results: Vec<FileConfig> = cli
        .files
        .iter()
        .map(|p| parse_one(p, &cli.format))
        .collect();
    let any_error = results.iter().any(|r| r.error.is_some());

    if results.len() == 1 {
        let one = &results[0];
        if cli.raw {
            // --raw: emit just the parsed value for clean piping into jsonx.
            match &one.data {
                Some(data) => emit(data, &mode),
                None => {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "error": one.error.clone().unwrap_or_else(|| "parse failed".into()),
                            "path": one.path,
                        })
                    );
                    std::process::exit(1);
                }
            }
        } else {
            emit(one, &mode);
        }
    } else {
        let output = Output {
            count: results.len(),
            files: results,
        };
        emit(&output, &mode);
    }

    // Non-zero exit if any file failed, so scripts and agents can detect it
    // while still receiving the structured output above.
    if any_error {
        std::process::exit(1);
    }
}
