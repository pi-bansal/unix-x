use clap::Parser;
use hickory_resolver::config::{NameServerConfigGroup, ResolverConfig, ResolverOpts};
use hickory_resolver::error::ResolveErrorKind;
use hickory_resolver::proto::rr::{RData, RecordType};
use hickory_resolver::Resolver;
use serde::Serialize;
use std::net::IpAddr;
use std::time::Instant;
use ux_output::{emit, OutMode};

#[derive(Parser)]
#[command(
    name = "dnsx",
    about = "Structured DNS lookups for AI agents.\nReplaces: dig, nslookup, host",
    long_about = "Resolve DNS records into structured JSON. TTLs are integers, records are\ngrouped and typed, and the resolver used is reported.\n\nExamples:\n  dnsx example.com                      # A records\n  dnsx example.com --type MX,TXT        # specific record types\n  dnsx example.com --all                # common record set\n  dnsx example.com --server 1.1.1.1     # use a specific resolver\n  dnsx 93.184.216.34 --reverse          # reverse (PTR) lookup",
    version
)]
struct Cli {
    /// Domain name to resolve (or IP address with --reverse)
    name: String,

    /// Record type(s): A, AAAA, MX, TXT, CNAME, NS, SOA, PTR, SRV, CAA
    /// (comma-separated or repeatable)
    #[arg(short, long)]
    r#type: Vec<String>,

    /// Query a common set of record types (A, AAAA, MX, TXT, NS, CNAME, SOA)
    #[arg(short, long)]
    all: bool,

    /// Reverse lookup: treat the argument as an IP and resolve PTR records
    #[arg(short = 'x', long)]
    reverse: bool,

    /// Resolver to use, e.g. 1.1.1.1 or 8.8.8.8:53 (default: system resolver)
    #[arg(short, long)]
    server: Option<String>,

    /// Output mode: auto (default), json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

#[derive(Serialize)]
struct Record {
    #[serde(rename = "type")]
    rtype: String,
    name: String,
    ttl: u32,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
}

#[derive(Serialize)]
struct Output {
    query: String,
    resolver: String,
    record_types: Vec<String>,
    records: Vec<Record>,
    count: usize,
    elapsed_ms: u128,
}

fn die_error(msg: &str, query: &str) -> ! {
    eprintln!("{}", serde_json::json!({ "error": msg, "query": query }));
    std::process::exit(1);
}

/// Strip a single trailing dot from an FQDN for readability.
fn fqdn(s: impl ToString) -> String {
    s.to_string().trim_end_matches('.').to_string()
}

fn parse_rtype(s: &str) -> Option<RecordType> {
    match s.trim().to_uppercase().as_str() {
        "A" => Some(RecordType::A),
        "AAAA" => Some(RecordType::AAAA),
        "CNAME" => Some(RecordType::CNAME),
        "MX" => Some(RecordType::MX),
        "TXT" => Some(RecordType::TXT),
        "NS" => Some(RecordType::NS),
        "SOA" => Some(RecordType::SOA),
        "PTR" => Some(RecordType::PTR),
        "SRV" => Some(RecordType::SRV),
        "CAA" => Some(RecordType::CAA),
        _ => None,
    }
}

/// Parse a resolver spec into (ip, port). Accepts "1.1.1.1", "1.1.1.1:53",
/// or a bare IPv6 like "::1".
fn parse_server(s: &str) -> Result<(IpAddr, u16), String> {
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Ok((ip, 53));
    }
    if let Some((host, port)) = s.rsplit_once(':') {
        let ip = host
            .parse::<IpAddr>()
            .map_err(|_| format!("invalid resolver IP: {}", host))?;
        let port = port
            .parse::<u16>()
            .map_err(|_| format!("invalid resolver port: {}", port))?;
        return Ok((ip, port));
    }
    Err(format!("invalid resolver address: {}", s))
}

fn build_resolver(server: &Option<String>) -> Result<(Resolver, String), String> {
    match server {
        Some(spec) => {
            let (ip, port) = parse_server(spec)?;
            let group = NameServerConfigGroup::from_ips_clear(&[ip], port, true);
            let config = ResolverConfig::from_parts(None, vec![], group);
            let resolver =
                Resolver::new(config, ResolverOpts::default()).map_err(|e| e.to_string())?;
            Ok((resolver, format!("{}:{}", ip, port)))
        }
        None => {
            let resolver = Resolver::from_system_conf()
                .or_else(|_| Resolver::new(ResolverConfig::default(), ResolverOpts::default()));
            match resolver {
                Ok(r) => Ok((r, "system".to_string())),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

/// Build a `Record` from a DNS resolver record.
fn to_record(name: String, ttl: u32, rtype: RecordType, data: &RData) -> Record {
    let mut rec = Record {
        rtype: rtype.to_string(),
        name,
        ttl,
        value: fqdn(data),
        priority: None,
        weight: None,
        port: None,
        target: None,
    };
    match data {
        RData::MX(mx) => {
            rec.priority = Some(mx.preference());
            rec.target = Some(fqdn(mx.exchange()));
        }
        RData::SRV(srv) => {
            rec.priority = Some(srv.priority());
            rec.weight = Some(srv.weight());
            rec.port = Some(srv.port());
            rec.target = Some(fqdn(srv.target()));
        }
        RData::CNAME(c) => rec.target = Some(fqdn(&c.0)),
        RData::NS(ns) => rec.target = Some(fqdn(&ns.0)),
        RData::PTR(ptr) => rec.target = Some(fqdn(&ptr.0)),
        _ => {}
    }
    rec
}

fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);

    let (resolver, resolver_name) = match build_resolver(&cli.server) {
        Ok(pair) => pair,
        Err(e) => die_error(&e, &cli.name),
    };

    let start = Instant::now();
    let mut records: Vec<Record> = Vec::new();
    let mut hard_error: Option<String> = None;

    // Determine which record types to query.
    let record_types: Vec<String>;

    if cli.reverse {
        record_types = vec!["PTR".to_string()];
        let ip: IpAddr = match cli.name.parse() {
            Ok(ip) => ip,
            Err(_) => die_error("--reverse requires a valid IP address", &cli.name),
        };
        match resolver.reverse_lookup(ip) {
            Ok(lookup) => {
                for r in lookup.as_lookup().record_iter() {
                    if let Some(data) = r.data() {
                        records.push(to_record(fqdn(r.name()), r.ttl(), r.record_type(), data));
                    }
                }
            }
            Err(e) => {
                if !matches!(e.kind(), ResolveErrorKind::NoRecordsFound { .. }) {
                    hard_error = Some(e.to_string());
                }
            }
        }
    } else {
        // Resolve requested types: --all, explicit --type, or default A.
        let types: Vec<RecordType> = if cli.all {
            vec![
                RecordType::A,
                RecordType::AAAA,
                RecordType::MX,
                RecordType::TXT,
                RecordType::NS,
                RecordType::CNAME,
                RecordType::SOA,
            ]
        } else if cli.r#type.is_empty() {
            vec![RecordType::A]
        } else {
            let mut out = Vec::new();
            for spec in &cli.r#type {
                for part in spec.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    match parse_rtype(part) {
                        Some(rt) => out.push(rt),
                        None => die_error(
                            &format!(
                                "unknown record type '{}' (supported: A, AAAA, MX, TXT, CNAME, NS, SOA, PTR, SRV, CAA)",
                                part
                            ),
                            &cli.name,
                        ),
                    }
                }
            }
            out
        };

        record_types = types.iter().map(|t| t.to_string()).collect();

        for rt in types {
            match resolver.lookup(cli.name.as_str(), rt) {
                Ok(lookup) => {
                    for r in lookup.record_iter() {
                        if let Some(data) = r.data() {
                            records.push(to_record(fqdn(r.name()), r.ttl(), r.record_type(), data));
                        }
                    }
                }
                Err(e) => {
                    // "No records" is a valid empty answer, not a failure.
                    if !matches!(e.kind(), ResolveErrorKind::NoRecordsFound { .. }) {
                        hard_error = Some(e.to_string());
                    }
                }
            }
        }
    }

    // Only fail if we got nothing AND hit a real resolution error (e.g. no
    // network / SERVFAIL). An empty-but-clean answer is a successful result.
    if records.is_empty() {
        if let Some(e) = hard_error {
            die_error(&e, &cli.name);
        }
    }

    let output = Output {
        query: cli.name.clone(),
        resolver: resolver_name,
        record_types,
        count: records.len(),
        elapsed_ms: start.elapsed().as_millis(),
        records,
    };

    if mode == OutMode::Table {
        print_table(&output);
    } else {
        emit(&output, &mode);
    }
}

fn print_table(out: &Output) {
    println!("Query:    {}", out.query);
    println!("Resolver: {}", out.resolver);
    println!("Records:  {} ({} ms)", out.count, out.elapsed_ms);
    if out.records.is_empty() {
        return;
    }
    println!();
    println!("{:<7} {:<8} {:<40} {}", "TYPE", "TTL", "NAME", "VALUE");
    println!("{}", "-".repeat(80));
    for r in &out.records {
        println!("{:<7} {:<8} {:<40} {}", r.rtype, r.ttl, r.name, r.value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_record_types() {
        assert_eq!(parse_rtype("a"), Some(RecordType::A));
        assert_eq!(parse_rtype("MX"), Some(RecordType::MX));
        assert_eq!(parse_rtype(" aaaa "), Some(RecordType::AAAA));
        assert_eq!(parse_rtype("txt"), Some(RecordType::TXT));
    }

    #[test]
    fn rejects_unknown_record_type() {
        assert_eq!(parse_rtype("nonsense"), None);
        assert_eq!(parse_rtype(""), None);
    }

    #[test]
    fn fqdn_trims_trailing_dot() {
        assert_eq!(fqdn("example.com."), "example.com");
        assert_eq!(fqdn("example.com"), "example.com");
        assert_eq!(fqdn("1.2.3.4"), "1.2.3.4");
    }

    #[test]
    fn parse_server_plain_ip_defaults_port_53() {
        assert_eq!(
            parse_server("8.8.8.8").unwrap(),
            ("8.8.8.8".parse().unwrap(), 53)
        );
    }

    #[test]
    fn parse_server_ip_with_port() {
        assert_eq!(
            parse_server("1.1.1.1:5353").unwrap(),
            ("1.1.1.1".parse().unwrap(), 5353)
        );
    }

    #[test]
    fn parse_server_ipv6() {
        assert_eq!(parse_server("::1").unwrap(), ("::1".parse().unwrap(), 53));
    }

    #[test]
    fn parse_server_rejects_garbage() {
        assert!(parse_server("not-an-ip").is_err());
        assert!(parse_server("1.1.1.1:notaport").is_err());
    }
}
