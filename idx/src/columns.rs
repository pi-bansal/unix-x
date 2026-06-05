/// Columnar index — each field stored as a contiguous typed array.
/// Row i across all columns = one file entry.
///
/// Layout on disk (bincode):
///   [ColumnarIndex]
///     paths:      Vec<String>   — absolute paths, sorted
///     sizes:      Vec<u64>      — byte sizes
///     mtimes:     Vec<u64>      — unix epoch seconds
///     ext_ids:    Vec<u16>      — interned extension ids (0 = no ext)
///     ext_pool:   Vec<String>   — extension strings indexed by ext_id
///     git_status: Vec<u8>       — GitStatus as u8 (255 = unknown)
///     dir_flags:  Vec<u8>       — 1 = directory, 0 = file
///
/// Why columnar?
///   A query like `--ext rs` only needs to scan ext_ids[] — a Vec<u16>.
///   It never touches paths[], sizes[], or git_status[].
///   On a 100k-file repo that's ~200KB scanned instead of ~50MB.

use ahash::AHashMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum GitStatus {
    Clean     = 0,
    Modified  = 1,
    Added     = 2,
    Deleted   = 3,
    Renamed   = 4,
    Untracked = 5,
    Ignored   = 6,
    Unknown   = 255,
}

impl GitStatus {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Clean,
            1 => Self::Modified,
            2 => Self::Added,
            3 => Self::Deleted,
            4 => Self::Renamed,
            5 => Self::Untracked,
            6 => Self::Ignored,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Clean     => "clean",
            Self::Modified  => "modified",
            Self::Added     => "added",
            Self::Deleted   => "deleted",
            Self::Renamed   => "renamed",
            Self::Untracked => "untracked",
            Self::Ignored   => "ignored",
            Self::Unknown   => "unknown",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ColumnarIndex {
    /// Indexed root directory
    pub root: String,

    /// Unix epoch of last full rebuild
    pub built_at: u64,

    /// Total entries (len of every column — they're all the same length)
    pub len: usize,

    // ── Columns ───────────────────────────────────────────────────────────────
    pub paths:      Vec<String>,  // absolute paths
    pub sizes:      Vec<u64>,     // bytes (recursive for dirs)
    pub mtimes:     Vec<u64>,     // unix epoch seconds
    pub ext_ids:    Vec<u16>,     // index into ext_pool; 0 = no extension
    pub git_status: Vec<u8>,      // GitStatus as u8
    pub dir_flags:  Vec<u8>,      // 1 = dir, 0 = file/symlink

    // ── String pools ──────────────────────────────────────────────────────────
    /// ext_pool[0] = "" (no extension sentinel)
    pub ext_pool: Vec<String>,
}

impl ColumnarIndex {
    pub fn new(root: String) -> Self {
        ColumnarIndex {
            root,
            built_at: now_secs(),
            len: 0,
            paths:      Vec::new(),
            sizes:      Vec::new(),
            mtimes:     Vec::new(),
            ext_ids:    Vec::new(),
            git_status: Vec::new(),
            dir_flags:  Vec::new(),
            ext_pool:   vec!["".to_string()], // 0 = no extension
        }
    }

    pub fn push(
        &mut self,
        path: String,
        size: u64,
        mtime: u64,
        ext: Option<&str>,
        git: GitStatus,
        is_dir: bool,
    ) {
        let ext_id = self.intern_ext(ext);
        self.paths.push(path);
        self.sizes.push(size);
        self.mtimes.push(mtime);
        self.ext_ids.push(ext_id);
        self.git_status.push(git as u8);
        self.dir_flags.push(if is_dir { 1 } else { 0 });
        self.len += 1;
    }

    fn intern_ext(&mut self, ext: Option<&str>) -> u16 {
        let ext = ext.unwrap_or("");
        if ext.is_empty() {
            return 0;
        }
        // Linear scan is fine — ext_pool is tiny (< 100 unique extensions)
        if let Some(pos) = self.ext_pool.iter().position(|e| e == ext) {
            return pos as u16;
        }
        let id = self.ext_pool.len() as u16;
        self.ext_pool.push(ext.to_string());
        id
    }

    pub fn ext_for(&self, id: u16) -> &str {
        self.ext_pool.get(id as usize).map(|s| s.as_str()).unwrap_or("")
    }

    /// Serialize to bytes (bincode)
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("serialization failed")
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }

    // ── Columnar queries ──────────────────────────────────────────────────────

    /// Returns matching row indices. Scans only the ext_ids column — O(n/8) vs O(n*avg_path_len).
    pub fn query_by_ext(&self, ext: &str) -> Vec<usize> {
        let Some(ext_id) = self.ext_pool.iter().position(|e| e == ext) else {
            return vec![];
        };
        let ext_id = ext_id as u16;
        self.ext_ids
            .iter()
            .enumerate()
            .filter_map(|(i, &e)| if e == ext_id { Some(i) } else { None })
            .collect()
    }

    /// Scans only sizes column. Never touches paths.
    pub fn query_by_size_gt(&self, threshold: u64) -> Vec<usize> {
        self.sizes
            .iter()
            .enumerate()
            .filter_map(|(i, &s)| if s > threshold { Some(i) } else { None })
            .collect()
    }

    /// Scans only mtimes column.
    pub fn query_by_mtime_gt(&self, since: u64) -> Vec<usize> {
        self.mtimes
            .iter()
            .enumerate()
            .filter_map(|(i, &t)| if t > since { Some(i) } else { None })
            .collect()
    }

    /// Scans only git_status column.
    pub fn query_by_git_status(&self, status: GitStatus) -> Vec<usize> {
        let target = status as u8;
        self.git_status
            .iter()
            .enumerate()
            .filter_map(|(i, &s)| if s == target { Some(i) } else { None })
            .collect()
    }

    /// Scans only dir_flags column.
    pub fn query_dirs_only(&self) -> Vec<usize> {
        self.dir_flags
            .iter()
            .enumerate()
            .filter_map(|(i, &f)| if f == 1 { Some(i) } else { None })
            .collect()
    }

    /// Intersect multiple index sets (AND logic). All sets must be sorted.
    pub fn intersect(sets: &[Vec<usize>]) -> Vec<usize> {
        if sets.is_empty() { return vec![]; }
        if sets.len() == 1 { return sets[0].clone(); }

        // Start with smallest set for efficiency
        let mut sorted = sets.to_vec();
        sorted.sort_by_key(|s| s.len());

        let mut result = sorted[0].clone();
        for set in &sorted[1..] {
            result = intersect_sorted(&result, set);
            if result.is_empty() { break; }
        }
        result
    }

    /// Project row indices into full entry structs
    pub fn project(&self, indices: &[usize]) -> Vec<IndexEntry> {
        indices.iter().filter_map(|&i| {
            if i >= self.len { return None; }
            Some(IndexEntry {
                path:       self.paths[i].clone(),
                size:       self.sizes[i],
                mtime:      self.mtimes[i],
                extension:  {
                    let e = self.ext_for(self.ext_ids[i]);
                    if e.is_empty() { None } else { Some(e.to_string()) }
                },
                git_status: GitStatus::from_u8(self.git_status[i]).as_str().to_string(),
                is_dir:     self.dir_flags[i] == 1,
            })
        }).collect()
    }

    /// All entries (full scan)
    pub fn all_indices(&self) -> Vec<usize> {
        (0..self.len).collect()
    }
}

fn intersect_sorted(a: &[usize], b: &[usize]) -> Vec<usize> {
    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal   => { result.push(a[i]); i += 1; j += 1; }
            std::cmp::Ordering::Less    => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    result
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// A single projected entry — what the query layer returns
#[derive(Serialize, Deserialize, Clone)]
pub struct IndexEntry {
    pub path:       String,
    pub size:       u64,
    pub mtime:      u64,
    pub extension:  Option<String>,
    pub git_status: String,
    pub is_dir:     bool,
}
