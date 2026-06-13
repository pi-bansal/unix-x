use serde::Serialize;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: EntryType,
    pub size: u64,            // bytes; recursive for dirs
    pub modified: u64,        // unix epoch seconds
    pub created: Option<u64>, // not available on all platforms
    pub permissions: String,  // e.g. "644"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_status: Option<GitStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<u64>, // dir only: direct child count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>, // symlink only: resolved target
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>, // file only
    /// dir only: `size`/`children` stopped early at a cap and are a lower bound
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub size_truncated: bool,
}

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GitStatus {
    Untracked,
    Modified,
    Added,
    Deleted,
    Renamed,
    Ignored,
    Clean,
}

impl Entry {
    pub fn from_metadata(
        path: &Path,
        meta: &std::fs::Metadata,
        git_status: Option<GitStatus>,
        recursive_size: Option<u64>,
        child_count: Option<u64>,
        size_truncated: bool,
    ) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let entry_type = if meta.is_dir() {
            EntryType::Dir
        } else if meta.is_symlink() {
            EntryType::Symlink
        } else if meta.is_file() {
            EntryType::File
        } else {
            EntryType::Other
        };

        let size = recursive_size.unwrap_or(meta.len());

        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let created = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());

        let permissions = format_permissions(meta);

        let extension = if meta.is_file() {
            path.extension()
                .map(|e| e.to_string_lossy().to_string())
        } else {
            None
        };

        let target = if meta.is_symlink() {
            std::fs::read_link(path)
                .ok()
                .map(|t| t.to_string_lossy().to_string())
        } else {
            None
        };

        Entry {
            name,
            path: path.to_string_lossy().to_string(),
            entry_type,
            size,
            modified,
            created,
            permissions,
            git_status,
            children: child_count,
            target,
            extension,
            size_truncated,
        }
    }
}

#[cfg(unix)]
fn format_permissions(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    format!("{:03o}", mode)
}

#[cfg(not(unix))]
fn format_permissions(meta: &std::fs::Metadata) -> String {
    if meta.permissions().readonly() {
        "444".to_string()
    } else {
        "644".to_string()
    }
}
