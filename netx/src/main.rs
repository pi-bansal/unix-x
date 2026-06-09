use clap::Parser;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Serialize)]
pub struct Timing {
    pub total_ms: u128,
}

#[derive(Serialize)]
pub struct RedirectEntry {
    pub url: String,
    pub status: u16,
}

#[derive(Serialize)]
pub struct Response {
    pub url: String,
    pub method: String,
    pub status: u16,
    pub status_text: String,
    pub ok: bool,                              // status 200-299
    pub headers: HashMap<String, String>,
    pub content_type: Option<String>,
    pub body_bytes: u64,
    pub body: ResponseBody,
    pub timing: Timing,
    pub redirects: Vec<RedirectEntry>,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ResponseBody {
    Json(Value),
    Text(String),
    Binary { encoding: String, data: String }, // base64
}

#[derive(Parser)]
#[command(name = "netx", about = "Structured HTTP client for AI agents.", version)]
struct Cli {
    /// URL to request
    url: String,

    /// HTTP method
    #[arg(short = 'X', long, default_value = "GET")]
    method: String,

    /// Request headers (key:value)
    #[arg(short = 'H', long)]
    header: Vec<String>,

    /// Request body
    #[arg(short, long)]
    data: Option<String>,

    /// Send data as JSON (sets Content-Type: application/json)
    #[arg(short, long)]
    json: bool,

    /// Follow redirects (max N)
    #[arg(short = 'L', long, default_value_t = 10)]
    follow: u32,

    /// Timeout in seconds
    #[arg(short, long, default_value_t = 30)]
    timeout: u64,

    /// Only emit response headers (no body)
    #[arg(short = 'I', long)]
    head_only: bool,

    /// Output mode: auto (default), json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();

    // redirects(0): follow manually so the redirect chain can be recorded
    // instead of being swallowed silently by ureq.
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(cli.timeout))
        .redirects(0)
        .build();

    let method = cli.method.to_uppercase();
    let mut current_url = cli.url.clone();
    let mut redirects: Vec<RedirectEntry> = Vec::new();
    let start = Instant::now();

    let resp = loop {
        let mut req = agent.request(&method, &current_url);
        for h in &cli.header {
            if let Some((k, v)) = h.split_once(':') {
                req = req.set(k.trim(), v.trim());
            }
        }
        if cli.json {
            req = req.set("Content-Type", "application/json");
        }

        let result = if let Some(ref body) = cli.data {
            req.send_string(body)
        } else {
            req.call()
        };

        // A 3xx with a Location we should follow; otherwise the final response.
        // ureq may surface 3xx as either Ok or Err(Status) depending on version,
        // so handle redirects in both arms.
        match result {
            Ok(r) => {
                if follow(&r, &mut redirects, &mut current_url, cli.follow) {
                    continue;
                }
                break r;
            }
            Err(ureq::Error::Status(_, r)) => {
                if follow(&r, &mut redirects, &mut current_url, cli.follow) {
                    continue;
                }
                break r;
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "error": e.to_string(),
                        "url": current_url,
                    }))
                    .unwrap()
                );
                std::process::exit(1);
            }
        }
    };

    let elapsed = start.elapsed().as_millis();
    process_response(resp, &cli, elapsed, &method, redirects);
}

/// If `resp` is a redirect we're still allowed to follow, record the hop and
/// advance `current_url`, returning true. Otherwise return false (final response).
fn follow(
    resp: &ureq::Response,
    redirects: &mut Vec<RedirectEntry>,
    current_url: &mut String,
    max: u32,
) -> bool {
    let code = resp.status();
    if (300..400).contains(&code) && (redirects.len() as u32) < max {
        if let Some(loc) = resp.header("location") {
            redirects.push(RedirectEntry { url: current_url.clone(), status: code });
            *current_url = resolve_url(current_url, loc);
            return true;
        }
    }
    false
}

/// Resolve a possibly-relative Location header against the current URL.
fn resolve_url(base: &str, location: &str) -> String {
    if let Ok(abs) = url::Url::parse(location) {
        return abs.to_string();
    }
    if let Ok(b) = url::Url::parse(base) {
        if let Ok(joined) = b.join(location) {
            return joined.to_string();
        }
    }
    location.to_string()
}

fn process_response(
    resp: ureq::Response,
    cli: &Cli,
    elapsed_ms: u128,
    method: &str,
    redirects: Vec<RedirectEntry>,
) {
    let status = resp.status();
    let status_text = resp.status_text().to_string();
    let content_type = resp.content_type().to_string();
    let url = resp.get_url().to_string();

    // Capture all response headers (ureq 2.x exposes header names). Multiple
    // values for one name (e.g. set-cookie) are joined with ", ".
    let mut headers: HashMap<String, String> = HashMap::new();
    for name in resp.headers_names() {
        let values = resp.all(&name);
        if !values.is_empty() {
            headers.insert(name.clone(), values.join(", "));
        }
    }

    let body = if cli.head_only {
        ResponseBody::Text(String::new())
    } else {
        parse_body(resp, &content_type)
    };

    let body_bytes = match &body {
        ResponseBody::Json(v) => serde_json::to_string(v).map(|s| s.len() as u64).unwrap_or(0),
        ResponseBody::Text(s) => s.len() as u64,
        ResponseBody::Binary { data, .. } => data.len() as u64,
    };

    let output = Response {
        url,
        method: method.to_string(),
        status,
        status_text,
        ok: (200..300).contains(&status),
        headers,
        content_type: Some(content_type),
        body_bytes,
        body,
        timing: Timing { total_ms: elapsed_ms },
        redirects,
    };

    emit(&output, &cli.out);
}

fn parse_body(resp: ureq::Response, content_type: &str) -> ResponseBody {
    let ct = content_type.to_lowercase();
    let mut buf = Vec::new();
    let mut reader = resp.into_reader();
    reader.read_to_end(&mut buf).ok();
    let body_str = String::from_utf8_lossy(&buf);

    if ct.contains("application/json") || ct.contains("+json") {
        if let Ok(v) = serde_json::from_str::<Value>(&body_str) {
            return ResponseBody::Json(v);
        }
    }

    if ct.contains("text/") || ct.contains("application/xml") || ct.contains("application/javascript") {
        return ResponseBody::Text(body_str.to_string());
    }

    // Binary fallback
    use base64::{Engine as _, engine::general_purpose};
    ResponseBody::Binary {
        encoding: "base64".to_string(),
        data: general_purpose::STANDARD.encode(&buf),
    }
}

fn emit(output: &Response, mode: &str) {
    // Resolve `auto`: pretty on a terminal, compact when piped.
    let mode = if mode == "auto" {
        if ux_output::is_tty() { "pretty" } else { "json" }
    } else {
        mode
    };
    match mode {
        "pretty" => println!("{}", serde_json::to_string_pretty(output).unwrap()),
        "table" => {
            println!("URL:     {}", output.url);
            println!("Method:  {}", output.method);
            println!("Status:  {} {}", output.status, output.status_text);
            println!("OK:      {}", output.ok);
            println!("Time:    {}ms", output.timing.total_ms);
            println!("Size:    {} bytes", output.body_bytes);
            println!("\nHeaders:");
            for (k, v) in &output.headers {
                println!("  {}: {}", k, v);
            }
        }
        _ => println!("{}", serde_json::to_string(output).unwrap()),
    }
}