mod collect;
mod ring;

use clap::{Parser, Subcommand};
use collect::Collector;
#[cfg(unix)]
use ring::RingBuffer;
use ring::Sample;
use serde::Serialize;
#[cfg(unix)]
use std::sync::{Arc, RwLock};
use ux_output::{emit, OutMode};

const DEFAULT_RING_SIZE: usize = 3600; // 1 hour at 1s intervals

#[derive(Parser)]
#[command(
    name = "statx",
    about = "Time-series system stats — vmstat/iostat/sar in one tool.\nStores samples in a circular buffer. Query the last N seconds instantly.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start the stats daemon (samples every second into a ring buffer)
    Start {
        /// Sample interval in seconds
        #[arg(short, long, default_value_t = 1)]
        interval: u64,

        /// Ring buffer capacity (number of samples to retain)
        #[arg(short, long, default_value_t = DEFAULT_RING_SIZE)]
        capacity: usize,
    },

    /// Get the latest snapshot
    Now {
        #[arg(short, long, default_value = "auto")]
        out: String,
    },

    /// Get last N samples from the ring buffer
    Last {
        /// Number of samples (seconds at default interval)
        #[arg(default_value_t = 60)]
        n: usize,

        #[arg(short, long, default_value = "auto")]
        out: String,
    },

    /// Watch live — stream samples to stdout
    Watch {
        /// Sample interval in seconds
        #[arg(short, long, default_value_t = 1)]
        interval: u64,

        /// Number of samples (0 = forever)
        #[arg(short, long, default_value_t = 0)]
        count: u64,

        #[arg(short, long, default_value = "auto")]
        out: String,
    },

    /// Summarize last N samples: min/max/avg per metric
    Summary {
        #[arg(default_value_t = 60)]
        n: usize,

        #[arg(short, long, default_value = "auto")]
        out: String,
    },
}

#[derive(Serialize)]
struct SampleSummary {
    n_samples:       usize,
    duration_secs:   u64,
    cpu: MetricSummary,
    mem_used: MetricSummary,
    disk_read_bps: MetricSummary,
    disk_write_bps: MetricSummary,
    net_rx_bps: MetricSummary,
    net_tx_bps: MetricSummary,
    load_1m: MetricSummary,
}

#[derive(Serialize)]
struct MetricSummary {
    min: f64,
    max: f64,
    avg: f64,
    p50: f64,
    p95: f64,
}

#[derive(Serialize)]
struct LastOutput {
    count: usize,
    samples: Vec<Sample>,
}

#[cfg(unix)]
fn socket_path() -> std::path::PathBuf {
    // Per-user socket in the runtime dir so concurrent users don't collide on a
    // single world-shared /tmp/statx.sock (one daemon per user is the design;
    // the client discovers it by this same well-known path).
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let who = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into());
    dir.join(format!("statx-{who}.sock"))
}

#[tokio::main]
async fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();

    match cli.command {
        Cmd::Start { interval, capacity } => {
            run_daemon(interval, capacity).await;
        }

        Cmd::Now { out } => {
            let mode = OutMode::from_str(&out);
            let mut collector = Collector::new();
            // Two samples: first warms up the delta counters. /proc counters
            // tick at 100Hz (10ms), so 50ms is plenty to get a non-zero delta
            // while keeping `now` fast.
            collector.sample();
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let sample = collector.sample();
            emit(&sample, &mode);
        }

        Cmd::Watch { interval, count, out } => {
            let mode = OutMode::from_str(&out);
            let mut collector = Collector::new();
            collector.sample(); // warm up

            let mut i = 0u64;
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                let sample = collector.sample();
                emit(&sample, &mode);
                i += 1;
                if count > 0 && i >= count { break; }
            }
        }

        Cmd::Last { n, out } => {
            let mode = OutMode::from_str(&out);
            match read_from_daemon(n).await {
                Ok(samples) => {
                    let output = LastOutput { count: samples.len(), samples };
                    emit(&output, &mode);
                }
                Err(e) => {
                    // Daemon not running — collect live samples instead
                    eprintln!("[statx] No daemon running ({}). Collecting {} live samples...", e, n);
                    let mut collector = Collector::new();
                    collector.sample(); // warm up
                    let mut samples = Vec::new();
                    for _ in 0..n.min(30) { // cap at 30 live samples
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        samples.push(collector.sample());
                    }
                    let output = LastOutput { count: samples.len(), samples };
                    emit(&output, &mode);
                }
            }
        }

        Cmd::Summary { n, out } => {
            let mode = OutMode::from_str(&out);
            let samples = match read_from_daemon(n).await {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("[statx] No daemon — collecting 10 live samples for summary...");
                    let mut collector = Collector::new();
                    collector.sample();
                    let mut s = Vec::new();
                    for _ in 0..10 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        s.push(collector.sample());
                    }
                    s
                }
            };

            let summary = summarize(&samples);
            emit(&summary, &mode);
        }
    }
}

#[cfg(not(unix))]
async fn run_daemon(_interval_secs: u64, _capacity: usize) {
    let u = ux_output::Unavailable::new(
        "statx start",
        "background daemon mode (Unix socket IPC) is only implemented on Unix platforms",
        Some("use `statx now`, `statx watch`, or `statx last`/`summary` for live sampling"),
    );
    eprintln!("{}", serde_json::json!({"unavailable": u}));
    std::process::exit(1);
}

#[cfg(unix)]
async fn run_daemon(interval_secs: u64, capacity: usize) {
    eprintln!("[statx] Starting — interval={}s, buffer={}s", interval_secs, capacity);

    let ring: Arc<RwLock<RingBuffer<Sample>>> =
        Arc::new(RwLock::new(RingBuffer::new(capacity)));

    // Collector task
    let ring_writer = ring.clone();
    tokio::spawn(async move {
        let mut collector = Collector::new();
        collector.sample(); // warm up deltas

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(interval_secs)).await;
            let sample = collector.sample();
            ring_writer.write().unwrap().push(sample);
        }
    });

    // Unix socket server
    let sock_path = socket_path();
    let _ = std::fs::remove_file(&sock_path);
    let listener = match tokio::net::UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "{}",
                serde_json::json!({
                    "error": format!("failed to bind socket: {e}"),
                    "path": sock_path.display().to_string(),
                })
            );
            std::process::exit(1);
        }
    };
    eprintln!("[statx] Socket: {}", sock_path.display());

    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let ring = ring.clone();

        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                // Request format: {"n": 60}
                let n: usize = serde_json::from_str::<serde_json::Value>(&line)
                    .ok()
                    .and_then(|v| v["n"].as_u64())
                    .unwrap_or(60) as usize;

                let samples: Vec<Sample> = {
                    let r = ring.read().unwrap();
                    r.last(n).into_iter().cloned().collect()
                };

                let mut resp = serde_json::to_string(&samples).unwrap();
                resp.push('\n');
                if writer.write_all(resp.as_bytes()).await.is_err() { break; }
            }
        });
    }
}

#[cfg(not(unix))]
async fn read_from_daemon(_n: usize) -> Result<Vec<Sample>, String> {
    Err("daemon mode not supported on this platform".into())
}

#[cfg(unix)]
async fn read_from_daemon(n: usize) -> Result<Vec<Sample>, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let sock = socket_path();
    let mut stream = UnixStream::connect(&sock)
        .await
        .map_err(|e| e.to_string())?;

    let req = format!("{{\"n\":{}}}\n", n);
    stream.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;

    let (reader, _) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let line = lines.next_line().await
        .map_err(|e| e.to_string())?
        .ok_or("no response")?;

    serde_json::from_str(&line).map_err(|e| e.to_string())
}

fn summarize(samples: &[Sample]) -> SampleSummary {
    let n = samples.len();
    if n == 0 {
        return SampleSummary {
            n_samples: 0,
            duration_secs: 0,
            cpu: zero_summary(),
            mem_used: zero_summary(),
            disk_read_bps: zero_summary(),
            disk_write_bps: zero_summary(),
            net_rx_bps: zero_summary(),
            net_tx_bps: zero_summary(),
            load_1m: zero_summary(),
        };
    }

    let duration_secs = samples.last().map(|s| s.ts).unwrap_or(0)
        .saturating_sub(samples.first().map(|s| s.ts).unwrap_or(0));

    SampleSummary {
        n_samples: n,
        duration_secs,
        cpu:            metric_summary(&samples.iter().map(|s| s.cpu_total as f64).collect::<Vec<_>>()),
        mem_used:       metric_summary(&samples.iter().map(|s| s.mem_used as f64).collect::<Vec<_>>()),
        disk_read_bps:  metric_summary(&samples.iter().map(|s| s.disk_read_bps as f64).collect::<Vec<_>>()),
        disk_write_bps: metric_summary(&samples.iter().map(|s| s.disk_write_bps as f64).collect::<Vec<_>>()),
        net_rx_bps:     metric_summary(&samples.iter().map(|s| s.net_rx_bps as f64).collect::<Vec<_>>()),
        net_tx_bps:     metric_summary(&samples.iter().map(|s| s.net_tx_bps as f64).collect::<Vec<_>>()),
        load_1m:        metric_summary(&samples.iter().map(|s| s.load_1m).collect::<Vec<_>>()),
    }
}

fn metric_summary(values: &[f64]) -> MetricSummary {
    if values.is_empty() { return zero_summary(); }

    let mut sorted = values.to_vec();
    // total_cmp orders NaN deterministically instead of panicking like
    // partial_cmp().unwrap() does when a kernel counter is reported as NaN.
    sorted.sort_by(|a, b| a.total_cmp(b));

    let min = sorted.first().copied().unwrap_or(0.0);
    let max = sorted.last().copied().unwrap_or(0.0);
    let avg = values.iter().sum::<f64>() / values.len() as f64;
    let p50 = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() as f64 * 0.95) as usize];

    MetricSummary { min, max, avg, p50, p95 }
}

fn zero_summary() -> MetricSummary {
    MetricSummary { min: 0.0, max: 0.0, avg: 0.0, p50: 0.0, p95: 0.0 }
}
