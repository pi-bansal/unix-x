/// Query engine — composes columnar filters with bloom pre-checks.
///
/// Query pipeline:
///   1. Bloom check  — O(k) hash ops, prunes obviously impossible queries
///   2. Column scan  — scans only the relevant column(s)
///   3. Intersect    — AND logic across multiple filters
///   4. Project      — pulls full entry data for matching row indices only
///
/// Example: `idx query --ext rs --git modified --size-gt 1000`
///   → bloom.might_have_ext("rs")          — if false, return [] immediately
///   → col_scan(ext_ids, "rs")             — Vec<usize> of matching rows
///   → col_scan(git_status, Modified)      — Vec<usize>
///   → col_scan(sizes, > 1000)             — Vec<usize>
///   → intersect([ext_rows, git_rows, size_rows])
///   → project(intersected_indices)        — only now touch paths[]

use crate::bloom::BloomSet;
use crate::columns::{ColumnarIndex, GitStatus, IndexEntry};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Query {
    /// Filter by extension (e.g. "rs")
    pub ext: Option<String>,

    /// Filter by git status
    pub git_status: Option<String>,

    /// Minimum file size in bytes
    pub size_gt: Option<u64>,

    /// Maximum file size in bytes
    pub size_lt: Option<u64>,

    /// Modified after this unix timestamp
    pub mtime_gt: Option<u64>,

    /// Path substring filter (applied post-scan, bloom-checked first)
    pub path_contains: Option<String>,

    /// Only return directories
    pub dirs_only: bool,

    /// Only return files
    pub files_only: bool,

    /// Max results to return
    pub limit: Option<usize>,

    /// Sort order: path, size, mtime (default: path)
    pub sort: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct QueryResult {
    pub count: usize,
    pub bloom_skipped: bool,   // true if bloom filter short-circuited the query
    pub scan_ms: u64,
    pub entries: Vec<IndexEntry>,
}

pub fn run_query(
    index: &ColumnarIndex,
    bloom: &BloomSet,
    query: &Query,
) -> QueryResult {
    let start = std::time::Instant::now();

    // ── Bloom pre-checks ──────────────────────────────────────────────────────

    if let Some(ref ext) = query.ext {
        if !bloom.might_have_ext(ext) {
            return QueryResult {
                count: 0,
                bloom_skipped: true,
                scan_ms: start.elapsed().as_millis() as u64,
                entries: vec![],
            };
        }
    }

    if let Some(ref path_sub) = query.path_contains {
        // Use first path component as bloom prefix check
        let first_component = path_sub.split('/').next().unwrap_or(path_sub);
        if !bloom.might_have_prefix(first_component) {
            return QueryResult {
                count: 0,
                bloom_skipped: true,
                scan_ms: start.elapsed().as_millis() as u64,
                entries: vec![],
            };
        }
    }

    // ── Column scans ──────────────────────────────────────────────────────────
    let mut filter_sets: Vec<Vec<usize>> = Vec::new();

    if let Some(ref ext) = query.ext {
        filter_sets.push(index.query_by_ext(ext));
    }

    if let Some(ref gs) = query.git_status {
        let status = parse_git_status(gs);
        filter_sets.push(index.query_by_git_status(status));
    }

    if let Some(threshold) = query.size_gt {
        filter_sets.push(index.query_by_size_gt(threshold));
    }

    if let Some(threshold) = query.size_lt {
        // size_lt: scan sizes, keep where size < threshold
        let indices: Vec<usize> = index.sizes
            .iter()
            .enumerate()
            .filter_map(|(i, &s)| if s < threshold { Some(i) } else { None })
            .collect();
        filter_sets.push(indices);
    }

    if let Some(since) = query.mtime_gt {
        filter_sets.push(index.query_by_mtime_gt(since));
    }

    if query.dirs_only {
        filter_sets.push(index.query_dirs_only());
    }

    if query.files_only {
        let indices: Vec<usize> = index.dir_flags
            .iter()
            .enumerate()
            .filter_map(|(i, &f)| if f == 0 { Some(i) } else { None })
            .collect();
        filter_sets.push(indices);
    }

    // ── Intersect ─────────────────────────────────────────────────────────────
    let mut indices = if filter_sets.is_empty() {
        index.all_indices()
    } else {
        ColumnarIndex::intersect(&filter_sets)
    };

    // ── Post-scan path filter (substring — can't be columnar) ─────────────────
    if let Some(ref sub) = query.path_contains {
        let sub_lower = sub.to_lowercase();
        indices.retain(|&i| {
            index.paths[i].to_lowercase().contains(&sub_lower)
        });
    }

    // ── Sort ──────────────────────────────────────────────────────────────────
    match query.sort.as_deref().unwrap_or("path") {
        "size"  => indices.sort_by(|&a, &b| index.sizes[b].cmp(&index.sizes[a])),
        "mtime" => indices.sort_by(|&a, &b| index.mtimes[b].cmp(&index.mtimes[a])),
        _       => indices.sort_by(|&a, &b| index.paths[a].cmp(&index.paths[b])),
    }

    // ── Limit ─────────────────────────────────────────────────────────────────
    if let Some(limit) = query.limit {
        indices.truncate(limit);
    }

    // ── Project ───────────────────────────────────────────────────────────────
    let entries = index.project(&indices);
    let count = entries.len();

    QueryResult {
        count,
        bloom_skipped: false,
        scan_ms: start.elapsed().as_millis() as u64,
        entries,
    }
}

fn parse_git_status(s: &str) -> GitStatus {
    match s.to_lowercase().as_str() {
        "clean"     => GitStatus::Clean,
        "modified"  => GitStatus::Modified,
        "added"     => GitStatus::Added,
        "deleted"   => GitStatus::Deleted,
        "renamed"   => GitStatus::Renamed,
        "untracked" => GitStatus::Untracked,
        "ignored"   => GitStatus::Ignored,
        _           => GitStatus::Unknown,
    }
}
