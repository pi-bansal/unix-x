use blake3::Hasher as Blake3;
use clap::Parser;
use hex::encode;
use md5::Md5;
use memmap2::Mmap;
use rayon::prelude::*;
use serde::Serialize;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::PathBuf;
use std::time::Instant;
use ux_output::{emit, OutMode};

#[derive(Parser)]
#[command(
    name = "hashx",
    about = "Structured multi-algorithm file hashing for AI agents.\nReplaces: md5sum, sha256sum, b3sum — all in one call.",
    version
)]
struct Cli {
    /// Files to hash
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Algorithms to compute: md5, sha1, sha256, blake3 (default: sha256 blake3)
    #[arg(short, long, value_delimiter = ',')]
    algos: Vec<String>,

    /// Verify against a known hash (format: algo:hexhash)
    #[arg(short, long)]
    verify: Option<String>,

    /// Compare two files for equality (no hash output)
    #[arg(short, long)]
    compare: bool,

    /// Output mode: auto, json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

#[derive(Serialize, Clone)]
pub struct FileHashes {
    pub path:         String,
    pub size_bytes:   u64,
    pub elapsed_ms:   u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5:          Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1:         Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256:       Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blake3:       Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error:        Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified:     Option<bool>,  // set when --verify used
}

#[derive(Serialize)]
struct Output {
    count:      usize,
    total_bytes: u64,
    total_ms:   u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    equal:      Option<bool>,        // set when --compare used
    files:      Vec<FileHashes>,
}

fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();
    let mode = OutMode::from_str(&cli.out);

    // Default algorithms
    let algos: Vec<String> = if cli.algos.is_empty() {
        vec!["sha256".to_string(), "blake3".to_string()]
    } else {
        cli.algos.clone()
    };

    let start = Instant::now();

    // Hash all files in parallel (rayon)
    let results: Vec<FileHashes> = cli.files
        .par_iter()
        .map(|path| hash_file(path, &algos))
        .collect();

    // Apply --verify if given
    let mut results = if let Some(ref verify_str) = cli.verify {
        results.into_iter().map(|mut r| {
            r.verified = Some(check_verify(&r, verify_str));
            r
        }).collect()
    } else {
        results
    };

    // --compare: check if all files have identical blake3 (or sha256)
    let equal = if cli.compare && results.len() >= 2 {
        let reference = results[0].blake3.clone().or(results[0].sha256.clone());
        Some(results.iter().all(|r| {
            let h = r.blake3.clone().or(r.sha256.clone());
            h == reference
        }))
    } else {
        None
    };

    let total_bytes: u64 = results.iter().map(|r| r.size_bytes).sum();
    let total_ms = start.elapsed().as_millis();
    let count = results.len();

    let output = Output {
        count,
        total_bytes,
        total_ms,
        equal,
        files: results,
    };

    if cli.out == "table" {
        print_table(&output);
    } else {
        emit(&output, &mode);
    }

    // Exit 1 if any verify failed
    if output.files.iter().any(|f| f.verified == Some(false)) {
        std::process::exit(1);
    }
}

fn hash_file(path: &PathBuf, algos: &[String]) -> FileHashes {
    let start = Instant::now();

    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => return FileHashes {
            path: path.to_string_lossy().to_string(),
            size_bytes: 0,
            elapsed_ms: 0,
            md5: None, sha1: None, sha256: None, blake3: None,
            error: Some(e.to_string()),
            verified: None,
        },
    };

    let size_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);

    // Use mmap for large files (>1MB) — avoids buffered copy overhead
    let data: Vec<u8> = if size_bytes > 1024 * 1024 {
        match unsafe { Mmap::map(&file) } {
            Ok(mmap) => mmap.to_vec(),
            Err(_)   => read_fallback(&file),
        }
    } else {
        read_fallback(&file)
    };

    let want_md5    = algos.iter().any(|a| a == "md5");
    let want_sha1   = algos.iter().any(|a| a == "sha1");
    let want_sha256 = algos.iter().any(|a| a == "sha256");
    let want_blake3 = algos.iter().any(|a| a == "blake3");

    // Compute all requested hashes in one pass over the data
    let md5_hash = if want_md5 {
        let mut h = Md5::new();
        h.update(&data);
        Some(encode(h.finalize()))
    } else { None };

    let sha1_hash = if want_sha1 {
        let mut h = Sha1::new();
        h.update(&data);
        Some(encode(h.finalize()))
    } else { None };

    let sha256_hash = if want_sha256 {
        let mut h = Sha256::new();
        h.update(&data);
        Some(encode(h.finalize()))
    } else { None };

    let blake3_hash = if want_blake3 {
        Some(blake3::hash(&data).to_hex().to_string())
    } else { None };

    FileHashes {
        path: path.to_string_lossy().to_string(),
        size_bytes,
        elapsed_ms: start.elapsed().as_millis(),
        md5: md5_hash,
        sha1: sha1_hash,
        sha256: sha256_hash,
        blake3: blake3_hash,
        error: None,
        verified: None,
    }
}

fn read_fallback(file: &File) -> Vec<u8> {
    use std::io::Read;
    let mut buf = Vec::new();
    let mut f = file;
    let _ = f.read_to_end(&mut buf);
    buf
}

fn check_verify(file: &FileHashes, verify_str: &str) -> bool {
    // Format: "sha256:abc123..." or just "abc123..." (assume blake3/sha256)
    let (algo, expected) = if let Some((a, h)) = verify_str.split_once(':') {
        (a, h)
    } else {
        ("blake3", verify_str)
    };

    match algo {
        "md5"    => file.md5.as_deref()    == Some(expected),
        "sha1"   => file.sha1.as_deref()   == Some(expected),
        "sha256" => file.sha256.as_deref() == Some(expected),
        "blake3" => file.blake3.as_deref() == Some(expected),
        _        => false,
    }
}

fn print_table(output: &Output) {
    println!("{:<60} {:>12} {:>10} {}", "FILE", "SIZE", "TIME", "HASHES");
    println!("{}", "-".repeat(110));
    for f in &output.files {
        if let Some(ref e) = f.error {
            println!("{:<60} ERROR: {}", f.path, e);
            continue;
        }
        let hashes: Vec<String> = [
            f.sha256.as_ref().map(|h| format!("sha256:{}", &h[..12])),
            f.blake3.as_ref().map(|h| format!("blake3:{}", &h[..12])),
            f.md5.as_ref().map(|h|    format!("md5:{}", &h[..12])),
            f.sha1.as_ref().map(|h|   format!("sha1:{}", &h[..12])),
        ].into_iter().flatten().collect();

        let verified = f.verified.map(|v| if v { " ✓" } else { " ✗" }).unwrap_or("");
        println!(
            "{:<60} {:>12} {:>8}ms {}{}",
            truncate(&f.path, 59),
            human_bytes(f.size_bytes),
            f.elapsed_ms,
            hashes.join(" "),
            verified,
        );
    }
    println!("{}", "-".repeat(110));
    println!("Total: {} files, {}, {}ms",
        output.count,
        human_bytes(output.total_bytes),
        output.total_ms,
    );

    if let Some(eq) = output.equal {
        println!("Files equal: {}", eq);
    }
}

fn human_bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1}GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1}MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.1}KB", n as f64 / (1u64 << 10) as f64),
        n                  => format!("{}B", n),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() }
    else { format!("{}…", &s[..max-1]) }
}
