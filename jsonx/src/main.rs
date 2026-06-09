mod query;
#[cfg(test)]
mod tests;

use clap::Parser;
use query::{parse_path, query as jq};
use serde_json::Value;
use std::io::{self, BufRead, Read};
use std::path::PathBuf;

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
        Some(path) => std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("{{\"error\": \"{}\"}}", e);
            std::process::exit(1);
        }),
        None => {
            // read_to_string errors on non-UTF-8 stdin; surface it as a
            // structured error instead of panicking.
            let mut s = String::new();
            if let Err(e) = io::stdin().read_to_string(&mut s) {
                eprintln!("{}", serde_json::json!({ "error": e.to_string() }));
                std::process::exit(1);
            }
            s
        }
    };

    let steps = parse_path(&cli.path);

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
