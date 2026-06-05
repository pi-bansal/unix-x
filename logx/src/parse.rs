use crate::detect::LogFormat;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize, Clone)]
pub struct LogEntry {
    pub line: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,    // ISO 8601 if parseable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,        // normalized: error/warn/info/debug/trace
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<HashMap<String, Value>>, // extra structured fields
    pub raw: String,
}

pub fn parse_line(raw: &str, line_num: u64, format: &LogFormat) -> LogEntry {
    match format {
        LogFormat::Json => parse_json(raw, line_num),
        LogFormat::Logfmt => parse_logfmt(raw, line_num),
        LogFormat::Nginx => parse_nginx(raw, line_num),
        LogFormat::Syslog => parse_syslog(raw, line_num),
        LogFormat::Rails => parse_rails(raw, line_num),
        LogFormat::Go => parse_go(raw, line_num),
        _ => plain(raw, line_num),
    }
}

fn parse_json(raw: &str, line: u64) -> LogEntry {
    let Ok(val) = serde_json::from_str::<Value>(raw) else {
        return plain(raw, line);
    };

    let obj = match val.as_object() {
        Some(o) => o,
        None => return plain(raw, line),
    };

    // Common field names across logging libs
    let message = obj
        .get("msg").or_else(|| obj.get("message")).or_else(|| obj.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let level = obj
        .get("level").or_else(|| obj.get("severity")).or_else(|| obj.get("lvl"))
        .and_then(|v| v.as_str())
        .map(normalize_level);

    let timestamp = obj
        .get("ts").or_else(|| obj.get("time")).or_else(|| obj.get("timestamp"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Remaining fields
    let skip = ["msg", "message", "text", "level", "severity", "lvl", "ts", "time", "timestamp"];
    let mut fields: HashMap<String, Value> = obj
        .iter()
        .filter(|(k, _)| !skip.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    LogEntry {
        line,
        timestamp,
        level,
        message,
        fields: if fields.is_empty() { None } else { Some(fields) },
        raw: raw.to_string(),
    }
}

fn parse_logfmt(raw: &str, line: u64) -> LogEntry {
    let mut fields: HashMap<String, Value> = HashMap::new();
    let mut message = String::new();
    let mut level = None;
    let mut timestamp = None;

    // Simple logfmt tokenizer
    let re = Regex::new(r#"(\w+)=("(?:[^"\\]|\\.)*"|\S+)"#).unwrap();
    for cap in re.captures_iter(raw) {
        let key = cap[1].to_string();
        let val = cap[2].trim_matches('"').to_string();

        match key.as_str() {
            "msg" | "message" => message = val,
            "level" | "lvl" | "severity" => level = Some(normalize_level(&val)),
            "ts" | "time" | "timestamp" => timestamp = Some(val),
            _ => { fields.insert(key, Value::String(val)); }
        }
    }

    LogEntry {
        line,
        timestamp,
        level,
        message,
        fields: if fields.is_empty() { None } else { Some(fields) },
        raw: raw.to_string(),
    }
}

fn parse_nginx(raw: &str, line: u64) -> LogEntry {
    // Combined log format: IP - user [date] "method path proto" status bytes "ref" "ua"
    let re = Regex::new(
        r#"^(\S+) \S+ \S+ \[([^\]]+)\] "(\S+) (\S+) \S+" (\d+) (\d+)"#,
    ).unwrap();

    if let Some(cap) = re.captures(raw) {
        let status: u16 = cap[5].parse().unwrap_or(0);
        let level = Some(if status >= 500 { "error" } else if status >= 400 { "warn" } else { "info" }.to_string());
        let mut fields = HashMap::new();
        fields.insert("ip".to_string(), Value::String(cap[1].to_string()));
        fields.insert("method".to_string(), Value::String(cap[3].to_string()));
        fields.insert("path".to_string(), Value::String(cap[4].to_string()));
        fields.insert("status".to_string(), Value::Number(status.into()));
        fields.insert("bytes".to_string(), Value::Number(cap[6].parse::<u64>().unwrap_or(0).into()));

        return LogEntry {
            line,
            timestamp: Some(cap[2].to_string()),
            level,
            message: format!("{} {} {}", &cap[3], &cap[4], &cap[5]),
            fields: Some(fields),
            raw: raw.to_string(),
        };
    }

    plain(raw, line)
}

fn parse_syslog(raw: &str, line: u64) -> LogEntry {
    let re = Regex::new(
        r#"^(\w+\s+\d+\s+\d+:\d+:\d+)\s+(\S+)\s+(\S+?)(?:\[(\d+)\])?:\s*(.*)"#,
    ).unwrap();

    if let Some(cap) = re.captures(raw) {
        let mut fields = HashMap::new();
        fields.insert("host".to_string(), Value::String(cap[2].to_string()));
        fields.insert("unit".to_string(), Value::String(cap[3].to_string()));
        if let Some(pid) = cap.get(4) {
            fields.insert("pid".to_string(), Value::String(pid.as_str().to_string()));
        }

        return LogEntry {
            line,
            timestamp: Some(cap[1].to_string()),
            level: None,
            message: cap[5].to_string(),
            fields: Some(fields),
            raw: raw.to_string(),
        };
    }

    plain(raw, line)
}

fn parse_rails(raw: &str, line: u64) -> LogEntry {
    let re = Regex::new(r#"^([A-Z]), \[([^\]]+)\]\s+(\w+) -- \S*: (.*)"#).unwrap();

    if let Some(cap) = re.captures(raw) {
        return LogEntry {
            line,
            timestamp: Some(cap[2].to_string()),
            level: Some(normalize_level(&cap[3])),
            message: cap[4].to_string(),
            fields: None,
            raw: raw.to_string(),
        };
    }

    plain(raw, line)
}

fn parse_go(raw: &str, line: u64) -> LogEntry {
    let re = Regex::new(r#"^(\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2})\s+(.*)"#).unwrap();

    if let Some(cap) = re.captures(raw) {
        return LogEntry {
            line,
            timestamp: Some(cap[1].to_string()),
            level: None,
            message: cap[2].to_string(),
            fields: None,
            raw: raw.to_string(),
        };
    }

    plain(raw, line)
}

fn plain(raw: &str, line: u64) -> LogEntry {
    LogEntry {
        line,
        timestamp: None,
        level: None,
        message: raw.to_string(),
        fields: None,
        raw: raw.to_string(),
    }
}

pub fn normalize_level(s: &str) -> String {
    match s.to_lowercase().as_str() {
        "error" | "err" | "fatal" | "crit" | "critical" | "alert" | "emerg" | "e" => "error",
        "warn" | "warning" | "w" => "warn",
        "info" | "information" | "i" | "notice" => "info",
        "debug" | "d" | "trace" | "t" | "verbose" => "debug",
        other => return other.to_string(),
    }
    .to_string()
}
