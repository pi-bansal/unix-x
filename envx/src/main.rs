use clap::Parser;
use dotenvy::from_path_iter;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    AwsKey,
    AwsSecret,
    JwtToken,
    HexString,
    Base64Blob,
    PrivateKey,
    GenericSecret, // name contains SECRET/KEY/TOKEN/PASSWORD
}

#[derive(Serialize, Clone)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
    pub source: String, // "shell", ".env", ".env.local", etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<SecretKind>,
    pub redacted: bool,
}

#[derive(Serialize)]
pub struct EnvFile {
    pub path: String,
    pub exists: bool,
    pub entry_count: usize,
}

#[derive(Serialize)]
pub struct Output {
    pub shell_var_count: usize,
    pub dotenv_var_count: usize,
    pub secret_count: usize,
    pub env_files: Vec<EnvFile>,
    pub vars: Vec<EnvVar>,
}

#[derive(Parser)]
#[command(name = "envx", about = "Structured environment and secrets inspection for AI agents.", version)]
struct Cli {
    /// Directory to search for .env files (default: current dir)
    #[arg(default_value = ".")]
    dir: PathBuf,

    /// Redact secret values (replace with ***)
    #[arg(short, long)]
    redact: bool,

    /// Show only secrets
    #[arg(short, long)]
    secrets_only: bool,

    /// Include shell environment variables
    #[arg(short = 'e', long)]
    shell: bool,

    /// Filter by key substring
    #[arg(short, long)]
    filter: Option<String>,

    /// Output: json (default), pretty, table, ndjson
    #[arg(short, long, default_value = "json")]
    out: String,
}

fn main() {
    let cli = Cli::parse();

    let mut vars: Vec<EnvVar> = Vec::new();

    // --- Shell environment ---
    if cli.shell {
        for (key, value) in std::env::vars() {
            let secret = detect_secret(&key, &value);
            let redacted = cli.redact && secret.is_some();
            vars.push(EnvVar {
                key,
                value: if redacted { "***".to_string() } else { value },
                source: "shell".to_string(),
                secret,
                redacted,
            });
        }
    }

    // --- .env files ---
    let dotenv_names = [".env", ".env.local", ".env.development", ".env.production", ".env.test"];
    let mut env_files: Vec<EnvFile> = Vec::new();
    let mut dotenv_count = 0usize;

    for name in &dotenv_names {
        let path = cli.dir.join(name);
        let exists = path.exists();
        let mut entry_count = 0;

        if exists {
            if let Ok(iter) = from_path_iter(&path) {
                for item in iter.flatten() {
                    let (key, value) = item;
                    entry_count += 1;
                    dotenv_count += 1;
                    let secret = detect_secret(&key, &value);
                    let redacted = cli.redact && secret.is_some();

                    // Dotenv overrides shell — mark source precisely
                    vars.push(EnvVar {
                        key,
                        value: if redacted { "***".to_string() } else { value },
                        source: name.to_string(),
                        secret,
                        redacted,
                    });
                }
            }
        }

        env_files.push(EnvFile {
            path: path.to_string_lossy().to_string(),
            exists,
            entry_count,
        });
    }

    // --- Filter ---
    if let Some(ref f) = cli.filter {
        let fl = f.to_lowercase();
        vars.retain(|v| v.key.to_lowercase().contains(&fl));
    }

    if cli.secrets_only {
        vars.retain(|v| v.secret.is_some());
    }

    let secret_count = vars.iter().filter(|v| v.secret.is_some()).count();
    let shell_count = vars.iter().filter(|v| v.source == "shell").count();

    let output = Output {
        shell_var_count: shell_count,
        dotenv_var_count: dotenv_count,
        secret_count,
        env_files,
        vars,
    };

    emit(&output, &cli.out);
}

fn emit(output: &Output, mode: &str) {
    match mode {
        "pretty" => println!("{}", serde_json::to_string_pretty(output).unwrap()),
        "ndjson" => {
            for v in &output.vars {
                println!("{}", serde_json::to_string(v).unwrap());
            }
        }
        "table" => {
            println!("{:<40} {:<12} {:<20} {}", "KEY", "SECRET", "SOURCE", "VALUE");
            println!("{}", "-".repeat(100));
            for v in &output.vars {
                let secret = v.secret.as_ref()
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_else(|| "-".to_string());
                let val = if v.value.len() > 40 { format!("{}...", &v.value[..40]) } else { v.value.clone() };
                println!("{:<40} {:<12} {:<20} {}", v.key, secret, v.source, val);
            }
        }
        _ => println!("{}", serde_json::to_string(output).unwrap()),
    }
}

fn detect_secret(key: &str, value: &str) -> Option<SecretKind> {
    let key_upper = key.to_uppercase();

    // AWS access key
    if Regex::new(r"^AKIA[0-9A-Z]{16}$").unwrap().is_match(value) {
        return Some(SecretKind::AwsKey);
    }

    // JWT
    if Regex::new(r"^eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$")
        .unwrap()
        .is_match(value)
    {
        return Some(SecretKind::JwtToken);
    }

    // PEM private key
    if value.contains("-----BEGIN") && value.contains("PRIVATE KEY") {
        return Some(SecretKind::PrivateKey);
    }

    // Long hex string (likely key/hash)
    if value.len() >= 32 && Regex::new(r"^[0-9a-fA-F]+$").unwrap().is_match(value) {
        return Some(SecretKind::HexString);
    }

    // Long base64 blob
    if value.len() >= 32
        && Regex::new(r"^[A-Za-z0-9+/=]+$").unwrap().is_match(value)
        && value.len() % 4 == 0
    {
        return Some(SecretKind::Base64Blob);
    }

    // Key name heuristics
    for keyword in &["SECRET", "PASSWORD", "PASSWD", "TOKEN", "API_KEY", "PRIVATE"] {
        if key_upper.contains(keyword) && !value.is_empty() {
            return Some(SecretKind::GenericSecret);
        }
    }

    None
}
