mod convert;
mod lang;
mod symbols;
#[cfg(test)]
mod tests;

use clap::Parser;
use lang::Lang;
use serde::Serialize;
use std::path::PathBuf;
use streaming_iterator::StreamingIterator;
use ux_output::{emit, OutMode, Unavailable};

#[derive(Parser)]
#[command(
    name = "astx",
    about = "Structured source-code AST inspection for AI agents.\nReplaces: ctags, tree-sitter CLI, ad-hoc grammar scraping.",
    long_about = "Parse a source file into a structured JSON AST using tree-sitter.\n\nExamples:\n  astx src/main.rs                  # full AST as JSON\n  astx src/main.rs --symbols        # top-level declarations (ctags-style)\n  astx app.py --kind function_definition\n  astx src/lib.rs --depth 2",
    version
)]
struct Cli {
    /// Source file to parse
    file: PathBuf,

    /// Override language detection (rust, python, javascript, typescript, tsx, go)
    #[arg(short, long)]
    lang: Option<String>,

    /// Emit a flat list of declarations (functions, classes, types, …)
    #[arg(short, long)]
    symbols: bool,

    /// Emit only nodes of these kinds, as a flat list (repeatable)
    #[arg(short, long)]
    kind: Vec<String>,

    /// Cap AST depth (root = 0)
    #[arg(short, long)]
    depth: Option<usize>,

    /// Run a tree-sitter query (S-expression) and emit the captures
    #[arg(short, long)]
    query: Option<String>,

    /// Output mode: auto (default), json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

#[derive(Serialize)]
struct Capture {
    name: String,
    kind: String,
    start: convert::Pos,
    end: convert::Pos,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

fn die_error(msg: &str, path: &std::path::Path) -> ! {
    eprintln!(
        "{}",
        serde_json::json!({ "error": msg, "path": path.display().to_string() })
    );
    std::process::exit(1);
}

fn main() {
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);

    // Resolve language: explicit --lang wins, else by extension.
    let language = match &cli.lang {
        Some(name) => Lang::from_name(name),
        None => cli
            .file
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Lang::from_ext),
    };

    let language = match language {
        Some(l) => l,
        None => {
            let ext = cli
                .file
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("(none)");
            let u = Unavailable::new(
                "ast_parsing",
                format!("no tree-sitter grammar for extension '.{}'", ext),
                Some(&format!(
                    "supported languages: {}. Override with --lang.",
                    Lang::supported()
                )),
            );
            emit(&serde_json::json!({ "unavailable": u }), &mode);
            std::process::exit(1);
        }
    };

    let src = match std::fs::read(&cli.file) {
        Ok(bytes) => bytes,
        Err(e) => die_error(&e.to_string(), &cli.file),
    };

    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language.language()).is_err() {
        die_error("failed to load grammar for language", &cli.file);
    }
    let tree = match parser.parse(&src, None) {
        Some(t) => t,
        None => die_error("failed to parse source", &cli.file),
    };
    let root = tree.root_node();

    // --query: run a tree-sitter query and emit captures.
    if let Some(query_str) = &cli.query {
        let query = match tree_sitter::Query::new(&language.language(), query_str) {
            Ok(q) => q,
            Err(e) => die_error(&format!("invalid query: {}", e), &cli.file),
        };
        let names = query.capture_names();
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut out: Vec<Capture> = Vec::new();
        let mut matches = cursor.matches(&query, root, src.as_slice());
        while let Some(m) = matches.next() {
            for cap in m.captures {
                out.push(Capture {
                    name: names[cap.index as usize].to_string(),
                    kind: cap.node.kind().to_string(),
                    start: convert::Pos {
                        row: cap.node.start_position().row,
                        col: cap.node.start_position().column,
                        byte: cap.node.start_byte(),
                    },
                    end: convert::Pos {
                        row: cap.node.end_position().row,
                        col: cap.node.end_position().column,
                        byte: cap.node.end_byte(),
                    },
                    text: cap.node.utf8_text(&src).ok().map(|s| s.to_string()),
                });
            }
        }
        emit(&out, &mode);
        return;
    }

    // --symbols: flat list of declarations.
    if cli.symbols {
        let syms = symbols::extract(root, &src, language);
        if cli.out == "table" {
            println!("{:<8} {:<24} {}", "LINE", "KIND", "NAME");
            for s in &syms {
                println!("{:<8} {:<24} {}", s.start.row + 1, s.kind, s.name);
            }
            return;
        }
        emit(&syms, &mode);
        return;
    }

    // --kind: flat list of nodes matching the given kinds.
    if !cli.kind.is_empty() {
        let mut out = Vec::new();
        convert::collect_kinds(root, &src, &cli.kind, cli.depth, &mut out);
        emit(&out, &mode);
        return;
    }

    // Default: the full AST from the root.
    let node = convert::convert(root, &src, cli.depth, None);
    emit(&node, &mode);
}
