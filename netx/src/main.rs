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

    /// Output: json (default), pretty, table, ndjson
    #[arg(short, long, default_value = "json")]
    out: String,
}

fn main() {
    let cli = Cli::parse();

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(cli.timeout))
        .redirects(cli.follow as usize)
        .build();

    let method = cli.method.to_uppercase();
    let mut req = agent.request(&method, &cli.url);

    // Headers
    for h in &cli.header {
        if let Some((k, v)) = h.split_once(':') {
            req = req.set(k.trim(), v.trim());
        }
    }

    if cli.json {
        req = req.set("Content-Type", "application/json");
    }

    let start = Instant::now();

    let result = if let Some(ref body) = cli.data {
        req.send_string(body)
    } else {
        req.call()
    };

    let elapsed = start.elapsed().as_millis();

    let resp = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            // Still parse error responses
            process_response(r, &cli, elapsed, &method);
            return;
        }
        Err(e) => {
            eprintln!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "error": e.to_string(),
                    "url": cli.url
                }))
                .unwrap()
            );
            std::process::exit(1);
        }
    };

    process_response(resp, &cli, elapsed, &method);
}

fn process_response(resp: ureq::Response, cli: &Cli, elapsed_ms: u128, method: &str) {
    let status = resp.status();
    let status_text = resp.status_text().to_string();
    let content_type = resp.content_type().to_string();
    let url = resp.get_url().to_string();

    let mut headers: HashMap<String, String> = HashMap::new();
    // ureq doesn't expose header names directly; capture known useful ones
    for name in &[
        "content-type", "content-length", "content-encoding",
        "server", "date", "cache-control", "etag",
        "x-request-id", "x-ratelimit-remaining", "location",
        "set-cookie", "transfer-encoding",
    ] {
        if let Some(val) = resp.header(name) {
            headers.insert(name.to_string(), val.to_string());
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
        redirects: vec![], // ureq follows silently; expose final URL
    };

    emit(&output, &cli.out);
}

fn parse_body(resp: ureq::Response, content_type: &str) -> ResponseBody {
    let ct = content_type.to_lowercase();

    if ct.contains("application/json") || ct.contains("+json") {
        match resp.into_json::<Value>() {
            Ok(v) => return ResponseBody::Json(v),
            Err(_) => {}
        }
    }

    if ct.contains("text/") || ct.contains("application/xml") || ct.contains("application/javascript") {
        match resp.into_string() {
            Ok(s) => return ResponseBody::Text(s),
            Err(_) => {}
        }
    }

    // Binary fallback
    let mut buf = Vec::new();
    if let Ok(mut reader) = resp.into_reader().read_to_end(&mut buf).map(|_| ()) {
        // unreachable pattern, just use buf
    } else {
        let _ = resp.into_reader();
    }

    use base64::{Engine as _, engine::general_purpose};
    ResponseBody::Binary {
        encoding: "base64".to_string(),
        data: general_purpose::STANDARD.encode(&buf),
    }
}

fn emit(output: &Response, mode: &str) {
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

trait ReadToEnd {
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize>;
}
