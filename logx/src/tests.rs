/// Tests for log format detection and parsing

#[cfg(test)]
mod tests {
    use crate::detect::{detect_format, LogFormat};
    use crate::parse::{normalize_level, parse_line};

    // ── Format detection ──────────────────────────────────────────────────────

    #[test]
    fn detect_json_format() {
        let line = r#"{"level":"info","msg":"server started","ts":"2024-01-01T00:00:00Z"}"#;
        assert_eq!(detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_logfmt_format() {
        let line = r#"level=info msg="user logged in" user_id=42"#;
        assert_eq!(detect_format(line), LogFormat::Logfmt);
    }

    #[test]
    fn detect_nginx_format() {
        let line = r#"192.168.1.1 - alice [01/Jan/2024:00:00:00 +0000] "GET /api HTTP/1.1" 200 1234"#;
        assert_eq!(detect_format(line), LogFormat::Nginx);
    }

    #[test]
    fn detect_syslog_format() {
        let line = "Jan  1 00:00:00 myhost myservice[1234]: something happened";
        assert_eq!(detect_format(line), LogFormat::Syslog);
    }

    #[test]
    fn detect_rails_format() {
        let line = "I, [2024-01-01T00:00:00.000000 #1234]  INFO -- : Started GET /";
        assert_eq!(detect_format(line), LogFormat::Rails);
    }

    #[test]
    fn detect_go_format() {
        let line = "2024/01/01 00:00:00 server listening on :8080";
        assert_eq!(detect_format(line), LogFormat::Go);
    }

    #[test]
    fn detect_plain_fallback() {
        let line = "just a plain log line with no recognizable format";
        assert_eq!(detect_format(line), LogFormat::Plain);
    }

    #[test]
    fn detect_empty_is_plain() {
        assert_eq!(detect_format(""), LogFormat::Plain);
    }

    // ── Level normalization ───────────────────────────────────────────────────

    #[test]
    fn normalize_error_variants() {
        for s in &["error", "err", "fatal", "crit", "critical", "alert", "emerg", "e", "ERROR", "FATAL"] {
            assert_eq!(normalize_level(s), "error", "failed for: {}", s);
        }
    }

    #[test]
    fn normalize_warn_variants() {
        for s in &["warn", "warning", "w", "WARN", "WARNING"] {
            assert_eq!(normalize_level(s), "warn", "failed for: {}", s);
        }
    }

    #[test]
    fn normalize_info_variants() {
        for s in &["info", "information", "i", "notice", "INFO"] {
            assert_eq!(normalize_level(s), "info", "failed for: {}", s);
        }
    }

    #[test]
    fn normalize_debug_variants() {
        for s in &["debug", "d", "trace", "t", "verbose", "DEBUG", "TRACE"] {
            assert_eq!(normalize_level(s), "debug", "failed for: {}", s);
        }
    }

    #[test]
    fn normalize_unknown_passthrough() {
        assert_eq!(normalize_level("custom_level"), "custom_level");
        assert_eq!(normalize_level("UNKNOWN"), "unknown");
    }

    // ── JSON parsing ──────────────────────────────────────────────────────────

    #[test]
    fn parse_json_extracts_message() {
        let line = r#"{"level":"info","msg":"hello world","ts":"2024-01-01"}"#;
        let entry = parse_line(line, 1, &LogFormat::Json);
        assert_eq!(entry.message, "hello world");
        assert_eq!(entry.level.as_deref(), Some("info"));
    }

    #[test]
    fn parse_json_normalizes_level() {
        let line = r#"{"level":"fatal","message":"boom"}"#;
        let entry = parse_line(line, 1, &LogFormat::Json);
        assert_eq!(entry.level.as_deref(), Some("error"));
    }

    #[test]
    fn parse_json_handles_message_field_aliases() {
        // "message" alias
        let line = r#"{"severity":"warn","message":"something","time":"now"}"#;
        let entry = parse_line(line, 1, &LogFormat::Json);
        assert_eq!(entry.message, "something");
        assert_eq!(entry.level.as_deref(), Some("warn"));
    }

    #[test]
    fn parse_json_extra_fields_in_fields_map() {
        let line = r#"{"level":"info","msg":"ok","request_id":"abc","user_id":42}"#;
        let entry = parse_line(line, 1, &LogFormat::Json);
        let fields = entry.fields.unwrap();
        assert!(fields.contains_key("request_id"));
        assert!(fields.contains_key("user_id"));
        assert!(!fields.contains_key("level"));
        assert!(!fields.contains_key("msg"));
    }

    #[test]
    fn parse_invalid_json_falls_back_to_plain() {
        let line = "this is not json {";
        let entry = parse_line(line, 1, &LogFormat::Json);
        assert_eq!(entry.message, line);
        assert!(entry.level.is_none());
    }

    // ── Logfmt parsing ────────────────────────────────────────────────────────

    #[test]
    fn parse_logfmt_basic() {
        let line = r#"level=info msg="user logged in" user_id=42"#;
        let entry = parse_line(line, 1, &LogFormat::Logfmt);
        assert_eq!(entry.level.as_deref(), Some("info"));
        assert_eq!(entry.message, "user logged in");
    }

    #[test]
    fn parse_logfmt_extra_fields() {
        let line = r#"level=warn msg="slow query" duration_ms=1500 table=users"#;
        let entry = parse_line(line, 1, &LogFormat::Logfmt);
        let fields = entry.fields.unwrap();
        assert!(fields.contains_key("duration_ms"));
        assert!(fields.contains_key("table"));
    }

    // ── Nginx parsing ─────────────────────────────────────────────────────────

    #[test]
    fn parse_nginx_200_is_info() {
        let line = r#"10.0.0.1 - - [01/Jan/2024:00:00:00 +0000] "GET / HTTP/1.1" 200 512 "-" "curl""#;
        let entry = parse_line(line, 1, &LogFormat::Nginx);
        assert_eq!(entry.level.as_deref(), Some("info"));
        let fields = entry.fields.unwrap();
        assert_eq!(fields["status"], 200);
    }

    #[test]
    fn parse_nginx_500_is_error() {
        let line = r#"10.0.0.1 - - [01/Jan/2024:00:00:00 +0000] "POST /api HTTP/1.1" 500 128 "-" "curl""#;
        let entry = parse_line(line, 1, &LogFormat::Nginx);
        assert_eq!(entry.level.as_deref(), Some("error"));
    }

    #[test]
    fn parse_nginx_404_is_warn() {
        let line = r#"10.0.0.1 - - [01/Jan/2024:00:00:00 +0000] "GET /missing HTTP/1.1" 404 0 "-" "curl""#;
        let entry = parse_line(line, 1, &LogFormat::Nginx);
        assert_eq!(entry.level.as_deref(), Some("warn"));
    }

    // ── Line numbers ──────────────────────────────────────────────────────────

    #[test]
    fn parse_preserves_line_number() {
        let entry = parse_line("some log line", 42, &LogFormat::Plain);
        assert_eq!(entry.line, 42);
    }

    #[test]
    fn parse_plain_uses_full_line_as_message() {
        let line = "a plain unstructured log entry";
        let entry = parse_line(line, 1, &LogFormat::Plain);
        assert_eq!(entry.message, line);
        assert!(entry.level.is_none());
        assert!(entry.timestamp.is_none());
    }
}
