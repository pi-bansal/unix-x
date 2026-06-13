mod query;
#[cfg(test)]
mod tests;

use clap::Parser;
use query::{parse_path, query as jq};
use serde_json::Value;
use std::io::{self, BufRead, Read};
use std::path::PathBuf;

/// Cap input size — without this, a huge file is fully read into memory
/// before we even attempt to parse it.
const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;

/// `serde_json::from_str::<Value>` recurses once per nesting level with no
/// built-in limit; deeply nested input (e.g. 10k+ levels of `[[[...]]]`) can
/// overflow the stack. Reject input nested deeper than this before parsing.
const MAX_JSON_DEPTH: usize = 512;

/// Scan for the maximum `{`/`[` nesting depth, ignoring brackets inside
/// JSON strings.
fn max_nesting_depth(s: &str) -> usize {
    let mut depth = 0usize;
    let mut max_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for b in s.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                max_depth = max_depth.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    max_depth
}

fn read_capped(mut reader: impl Read) -> io::Result<String> {
    let mut buf = Vec::new();
    reader.by_ref().take(MAX_INPUT_BYTES + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_INPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("input exceeds {} byte limit", MAX_INPUT_BYTES),
        ));
    }
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[derive(Parser)]
#[command(
    name = "jsonx",
    about = "Fast JSON query and transform for AI agents.\n\nExamples:\n  jsonx '.users[].name' data.json\n  jsonx '.items[?(@.price > 10)]' data.json\n  cat data.json | jsonx '.results[0:5]'\n  jsonx --keys data.json\n  jsonx --count '.users[]' data.json",
    version
)]
struct Cli {
    /// Path expression (e.g. .users[0].name, .items[], .items[?(@.active == true)])
    #[arg(default_value = ".")]
    path: String,

    /// JSON file (default: stdin)
    #[arg()]
    file: Option<PathBuf>,

    /// Count matched results instead of printing them
    #[arg(short, long)]
    count: bool,

    /// Print only keys of matched object(s)
    #[arg(short, long)]
    keys: bool,

    /// Print only values of matched object(s)
    #[arg(short, long)]
    values: bool,

    /// Flatten array results into lines
    #[arg(short, long)]
    flatten: bool,

    /// Output raw strings (no JSON quoting)
    #[arg(short, long)]
    raw: bool,

    /// Output mode: auto (default), json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,

    /// Read input as newline-delimited JSON (one object per line)
    #[arg(long)]
    ndjson_in: bool,
}

fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();

    let input = match &cli.file {
        Some(path) => {
            let f = std::fs::File::open(path).unwrap_or_else(|e| {
                eprintln!("{{\"error\": \"{}\"}}", e);
                std::process::exit(1);
            });
            read_capped(f).unwrap_or_else(|e| {
                eprintln!("{{\"error\": \"{}\"}}", e);
                std::process::exit(1);
            })
        }
        None => read_capped(io::stdin()).unwrap_or_else(|e| {
            eprintln!("{}", serde_json::json!({ "error": e.to_string() }));
            std::process::exit(1);
        }),
    };

    let steps = parse_path(&cli.path);

    // Reject pathologically nested input before handing it to serde_json's
    // recursive-descent parser, which has no built-in depth limit.
    if max_nesting_depth(&input) > MAX_JSON_DEPTH {
        eprintln!("{}", serde_json::json!({
            "error": format!("input nesting exceeds max depth of {}", MAX_JSON_DEPTH)
        }));
        std::process::exit(1);
    }

    // Parse input as single JSON or NDJSON
    let roots: Vec<Value> = if cli.ndjson_in {
        input
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    } else {
        match serde_json::from_str::<Value>(&input) {
            Ok(v) => vec![v],
            Err(e) => {
                eprintln!("{{\"error\": \"parse error: {}\"}}", e);
                std::process::exit(1);
            }
        }
    };

    // Query all roots
    let results: Vec<&Value> = roots.iter().flat_map(|r| jq(r, &steps)).collect();

    if cli.count {
        println!("{}", results.len());
        return;
    }

    if cli.keys {
        for v in &results {
            if let Value::Object(obj) = v {
                let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                emit_value(&Value::Array(keys.iter().map(|k| Value::String(k.to_string())).collect()), &cli.out, cli.raw);
            }
        }
        return;
    }

    if cli.values {
        for v in &results {
            if let Value::Object(obj) = v {
                let vals: Vec<Value> = obj.values().cloned().collect();
                emit_value(&Value::Array(vals), &cli.out, cli.raw);
            }
        }
        return;
    }

    if cli.flatten {
        for v in &results {
            match v {
                Value::Array(arr) => {
                    for item in arr {
                        emit_value(item, &cli.out, cli.raw);
                    }
                }
                other => emit_value(other, &cli.out, cli.raw),
            }
        }
        return;
    }

    // Default: emit each result
    match results.len() {
        0 => {} // no match, no output, exit 0
        1 => emit_value(results[0], &cli.out, cli.raw),
        _ => {
            // Wrap multiple results in array
            let arr: Vec<Value> = results.iter().map(|v| (*v).clone()).collect();
            emit_value(&Value::Array(arr), &cli.out, cli.raw);
        }
    }
}

fn emit_value(v: &Value, mode: &str, raw: bool) {
    if raw {
        match v {
            Value::String(s) => { println!("{}", s); return; }
            Value::Number(n) => { println!("{}", n); return; }
            Value::Bool(b) => { println!("{}", b); return; }
            _ => {}
        }
    }

    // Resolve `auto`: pretty on a terminal, compact when piped.
    let mode = if mode == "auto" {
        if ux_output::is_tty() { "pretty" } else { "json" }
    } else {
        mode
    };

    match mode {
        "pretty" => println!("{}", serde_json::to_string_pretty(v).unwrap()),
        "ndjson" => {
            if let Value::Array(arr) = v {
                for item in arr {
                    println!("{}", serde_json::to_string(item).unwrap());
                }
            } else {
                println!("{}", serde_json::to_string(v).unwrap());
            }
        }
        "table" => {
            match v {
                Value::Array(arr) => {
                    // Detect columns from first object
                    if let Some(Value::Object(first)) = arr.first() {
                        let cols: Vec<&str> = first.keys().map(|k| k.as_str()).collect();
                        // Header
                        println!("{}", cols.join("\t"));
                        println!("{}", "-".repeat(cols.len() * 16));
                        for item in arr {
                            if let Value::Object(obj) = item {
                                let row: Vec<String> = cols.iter()
                                    .map(|c| obj.get(*c).map(|v| value_str(v)).unwrap_or_default())
                                    .collect();
                                println!("{}", row.join("\t"));
                            }
                        }
                    }
                }
                Value::Object(obj) => {
                    for (k, v) in obj {
                        println!("{}\t{}", k, value_str(v));
                    }
                }
                other => println!("{}", value_str(other)),
            }
        }
        _ => println!("{}", serde_json::to_string(v).unwrap()),
    }
}

fn value_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}
