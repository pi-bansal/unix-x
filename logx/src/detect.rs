use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub enum LogFormat {
    Json,          // {"level":"info","msg":"...","ts":...}
    Logfmt,        // level=info msg="..." ts=...
    Nginx,         // combined access log
    Apache,        // common log format
    Systemd,       // journald output
    Syslog,        // RFC 3164
    Rails,         // [timestamp] LEVEL -- ...
    Go,            // 2009/11/10 23:00:00 ...
    Plain,         // fallback: treat whole line as message
}

pub fn detect_format(sample: &str) -> LogFormat {
    let line = sample.trim();

    if line.starts_with('{') && line.ends_with('}') {
        return LogFormat::Json;
    }

    // logfmt: key=value pairs
    if Regex::new(r#"^\w+=\S+"#).unwrap().is_match(line) {
        return LogFormat::Logfmt;
    }

    // nginx/apache: starts with IP address
    if Regex::new(r#"^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}"#).unwrap().is_match(line) {
        if line.contains("HTTP/") {
            return LogFormat::Nginx;
        }
    }

    // systemd: starts with month day time host unit[pid]:
    if Regex::new(r#"^[A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}"#)
        .unwrap()
        .is_match(line)
    {
        return LogFormat::Syslog;
    }

    // Rails: I, [timestamp#pid] LEVEL -- ...
    if Regex::new(r#"^[A-Z], \["#).unwrap().is_match(line) {
        return LogFormat::Rails;
    }

    // Go standard log: 2009/11/10 23:00:00
    if Regex::new(r#"^\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2}"#)
        .unwrap()
        .is_match(line)
    {
        return LogFormat::Go;
    }

    LogFormat::Plain
}
