//! Format detection and parsing. Every parser converts its input into a
//! `serde_json::Value` so the rest of the suite (notably `jsonx`) can consume it
//! uniformly. INI and `.properties` are untyped, so their values are strings.

use serde_json::{Map, Value};
use std::path::Path;

/// Detect the config format from a file extension. Returns `None` when the
/// extension is unknown — the caller should then require an explicit `--format`.
pub fn detect_format(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "ini" | "cfg" | "conf" => "ini",
        "properties" => "properties",
        "json" => "json",
        _ => return None,
    })
}

/// Parse `content` in the named `format` into a `serde_json::Value`.
pub fn parse(content: &str, format: &str) -> Result<Value, String> {
    match format {
        "yaml" => parse_yaml(content),
        "toml" => toml::from_str::<Value>(content).map_err(|e| e.to_string()),
        "ini" => parse_ini(content),
        "properties" => Ok(parse_properties(content)),
        "json" => serde_json::from_str::<Value>(content).map_err(|e| e.to_string()),
        other => Err(format!("unknown format: {other}")),
    }
}

/// YAML supports multiple `---`-separated documents. A single document parses to
/// its value; multiple documents parse to a JSON array of documents.
fn parse_yaml(content: &str) -> Result<Value, String> {
    use serde::Deserialize;
    let mut docs = Vec::new();
    for de in serde_yaml::Deserializer::from_str(content) {
        docs.push(Value::deserialize(de).map_err(|e| e.to_string())?);
    }
    match docs.len() {
        0 => Ok(Value::Null),
        1 => Ok(docs.into_iter().next().unwrap()),
        _ => Ok(Value::Array(docs)),
    }
}

/// INI parses to an object: one nested object per named `[section]`, with keys
/// from the default (section-less) block placed at the top level. Values are
/// strings, as INI carries no type information.
fn parse_ini(content: &str) -> Result<Value, String> {
    let ini = ini::Ini::load_from_str(content).map_err(|e| e.to_string())?;
    let mut root = Map::new();
    for (section, props) in ini.iter() {
        let mut obj = Map::new();
        for (k, v) in props.iter() {
            obj.insert(k.to_string(), Value::String(v.to_string()));
        }
        match section {
            Some(name) => {
                root.insert(name.to_string(), Value::Object(obj));
            }
            None => {
                for (k, v) in obj {
                    root.insert(k, v);
                }
            }
        }
    }
    Ok(Value::Object(root))
}

/// `.properties` parses to a flat object of string values. Supports `=`/`:`
/// separators, `#`/`!` comments, and trailing-backslash line continuation.
/// Dotted keys are kept flat (no auto-nesting) for predictable output.
fn parse_properties(content: &str) -> Value {
    let mut map = Map::new();
    let mut acc = String::new();

    for raw in content.lines() {
        if acc.is_empty() {
            let trimmed = raw.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
        }
        if let Some(stripped) = raw.strip_suffix('\\') {
            acc.push_str(stripped);
            continue;
        }
        acc.push_str(raw);
        if let Some((k, v)) = split_property(acc.trim()) {
            map.insert(k, Value::String(v));
        }
        acc.clear();
    }
    if !acc.is_empty() {
        if let Some((k, v)) = split_property(acc.trim()) {
            map.insert(k, Value::String(v));
        }
    }
    Value::Object(map)
}

/// Split a property line on the first `=` or `:`. A line with no separator is a
/// key with an empty value.
fn split_property(line: &str) -> Option<(String, String)> {
    let sep = line.char_indices().find(|&(_, c)| c == '=' || c == ':');
    match sep {
        Some((i, c)) => {
            let key = line[..i].trim().to_string();
            let val = line[i + c.len_utf8()..].trim().to_string();
            if key.is_empty() {
                None
            } else {
                Some((key, val))
            }
        }
        None => {
            let key = line.trim();
            if key.is_empty() {
                None
            } else {
                Some((key.to_string(), String::new()))
            }
        }
    }
}
