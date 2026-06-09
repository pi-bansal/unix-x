use clap::Parser;
use serde::Serialize;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use ux_output::{emit, OutMode};

#[derive(Serialize)]
pub struct ArchiveEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String, // file, dir, symlink
    pub size_compressed: Option<u64>,
    pub size_uncompressed: u64,
    pub modified: Option<u64>, // unix epoch
    pub permissions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
}

#[derive(Serialize)]
pub struct ArchiveInfo {
    pub path: String,
    pub format: String,
    pub total_entries: usize,
    pub total_size_uncompressed: u64,
    pub total_size_compressed: Option<u64>,
    pub compression_ratio: Option<f32>,
    pub entries: Vec<ArchiveEntry>,
}

#[derive(Parser)]
#[command(name = "arcx", about = "Unified archive inspection for AI agents.", version)]
struct Cli {
    /// Archive file to inspect
    archive: PathBuf,

    /// Filter entries by path substring
    #[arg(long)]
    filter: Option<String>,

    /// List only files (no dirs)
    #[arg(short = 'f', long)]
    files_only: bool,

    /// Sort by: name, size, modified
    #[arg(short, long, default_value = "name")]
    sort: String,

    /// Summary only (no entry list)
    #[arg(long)]
    summary: bool,

    /// Output mode: auto (default), json, pretty, table, ndjson
    #[arg(short, long, default_value = "auto")]
    out: String,
}

fn main() {
    ux_output::reset_sigpipe();
    let cli = Cli::parse();

    let path = &cli.archive;
    if !path.exists() {
        eprintln!("{{\"error\": \"file not found\", \"path\": \"{}\"}}", path.display());
        std::process::exit(1);
    }

    let result = inspect_archive(path);

    let mut info = match result {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{{\"error\": \"{}\", \"path\": \"{}\"}}", e, path.display());
            std::process::exit(1);
        }
    };

    // Filter
    if let Some(ref filter) = cli.filter {
        let f = filter.to_lowercase();
        info.entries.retain(|e| e.path.to_lowercase().contains(&f));
    }

    if cli.files_only {
        info.entries.retain(|e| e.entry_type == "file");
    }

    // Sort
    match cli.sort.as_str() {
        "size" => info.entries.sort_by(|a, b| b.size_uncompressed.cmp(&a.size_uncompressed)),
        "modified" => info.entries.sort_by(|a, b| b.modified.cmp(&a.modified)),
        _ => info.entries.sort_by(|a, b| a.path.cmp(&b.path)),
    }

    if cli.summary {
        info.entries.clear();
    }

    // ndjson streams one entry per line.
    if cli.out == "ndjson" {
        for entry in &info.entries {
            println!("{}", serde_json::to_string(entry).unwrap());
        }
        return;
    }

    if cli.out == "table" {
        print_table(&info);
        return;
    }

    emit(&info, &OutMode::from_str(&cli.out));
}

fn print_table(info: &ArchiveInfo) {
    println!("format={}  entries={}  uncompressed={}  compressed={}  ratio={}",
        info.format,
        info.total_entries,
        info.total_size_uncompressed,
        info.total_size_compressed.map(|c| c.to_string()).unwrap_or_else(|| "-".into()),
        info.compression_ratio.map(|r| format!("{:.3}", r)).unwrap_or_else(|| "-".into()));
    if !info.entries.is_empty() {
        println!("{:>12}  {:<8}  {}", "SIZE", "TYPE", "PATH");
        for e in &info.entries {
            println!("{:>12}  {:<8}  {}", e.size_uncompressed, e.entry_type, e.path);
        }
    }
}

fn inspect_archive(path: &Path) -> Result<ArchiveInfo, String> {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();

    if name.ends_with(".zip") {
        inspect_zip(path)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        inspect_tar(path, "tar.gz")
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        inspect_tar(path, "tar.bz2")
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        inspect_tar(path, "tar.xz")
    } else if name.ends_with(".tar") {
        inspect_tar(path, "tar")
    } else if name.ends_with(".gz") {
        inspect_gz(path)
    } else {
        Err(format!("unrecognized archive format: {}", name))
    }
}

fn inspect_zip(path: &Path) -> Result<ArchiveInfo, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    let mut entries = Vec::new();
    let mut total_uncompressed = 0u64;
    let mut total_compressed = 0u64;

    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;

        let entry_type = if entry.is_dir() {
            "dir"
        } else {
            "file"
        }
        .to_string();

        let dt = entry.last_modified();
        let modified = {
            // Approximate: seconds since 1970 from zip datetime
            let year = dt.year() as u64;
            let days = (year - 1970) * 365 + dt.month() as u64 * 30 + dt.day() as u64;
            Some(days * 86400 + dt.hour() as u64 * 3600 + dt.minute() as u64 * 60 + dt.second() as u64)
        };

        let permissions = entry.unix_mode().map(|m| format!("{:03o}", m & 0o777));

        total_uncompressed += entry.size();
        total_compressed += entry.compressed_size();

        entries.push(ArchiveEntry {
            path: entry.name().to_string(),
            entry_type,
            size_compressed: Some(entry.compressed_size()),
            size_uncompressed: entry.size(),
            modified,
            permissions,
            link_target: None,
        });
    }

    let ratio = if total_uncompressed > 0 {
        Some(total_compressed as f32 / total_uncompressed as f32)
    } else {
        None
    };

    Ok(ArchiveInfo {
        path: path.to_string_lossy().to_string(),
        format: "zip".to_string(),
        total_entries: entries.len(),
        total_size_uncompressed: total_uncompressed,
        total_size_compressed: Some(total_compressed),
        compression_ratio: ratio,
        entries,
    })
}

fn inspect_tar(path: &Path, format: &str) -> Result<ArchiveInfo, String> {
    use std::time::UNIX_EPOCH;

    let file = File::open(path).map_err(|e| e.to_string())?;
    let file_size = file.metadata().map(|m| m.len()).ok();

    let reader: Box<dyn Read> = match format {
        "tar.gz" => Box::new(flate2::read::GzDecoder::new(file)),
        "tar.bz2" => Box::new(bzip2::read::BzDecoder::new(file)),
        "tar.xz" => Box::new(xz2::read::XzDecoder::new(file)),
        _ => Box::new(file),
    };

    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    let mut total_uncompressed = 0u64;

    for entry_result in archive.entries().map_err(|e| e.to_string())? {
        let entry = entry_result.map_err(|e| e.to_string())?;
        let header = entry.header();

        let entry_type = match header.entry_type() {
            tar::EntryType::Directory => "dir",
            tar::EntryType::Symlink => "symlink",
            _ => "file",
        }
        .to_string();

        let path_str = entry
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let size = header.size().unwrap_or(0);
        total_uncompressed += size;

        let modified = header.mtime().ok();
        let permissions = header.mode().ok().map(|m| format!("{:03o}", m & 0o777));
        let link_target = entry
            .link_name()
            .ok()
            .flatten()
            .map(|l| l.to_string_lossy().to_string());

        entries.push(ArchiveEntry {
            path: path_str,
            entry_type,
            size_compressed: None, // tar doesn't have per-entry compressed size
            size_uncompressed: size,
            modified,
            permissions,
            link_target,
        });
    }

    Ok(ArchiveInfo {
        path: path.to_string_lossy().to_string(),
        format: format.to_string(),
        total_entries: entries.len(),
        total_size_uncompressed: total_uncompressed,
        total_size_compressed: file_size,
        compression_ratio: file_size.map(|fs| fs as f32 / total_uncompressed.max(1) as f32),
        entries,
    })
}

fn inspect_gz(path: &Path) -> Result<ArchiveInfo, String> {
    // Single gzipped file — just report metadata
    let file = File::open(path).map_err(|e| e.to_string())?;
    let compressed_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let uncompressed_size = buf.len() as u64;

    let inner_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "contents".to_string());

    Ok(ArchiveInfo {
        path: path.to_string_lossy().to_string(),
        format: "gz".to_string(),
        total_entries: 1,
        total_size_uncompressed: uncompressed_size,
        total_size_compressed: Some(compressed_size),
        compression_ratio: Some(compressed_size as f32 / uncompressed_size.max(1) as f32),
        entries: vec![ArchiveEntry {
            path: inner_name,
            entry_type: "file".to_string(),
            size_compressed: Some(compressed_size),
            size_uncompressed: uncompressed_size,
            modified: None,
            permissions: None,
            link_target: None,
        }],
    })
}
