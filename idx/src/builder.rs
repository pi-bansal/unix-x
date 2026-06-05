/// Index builder — walks the filesystem once, populates columnar index + bloom.
///
/// Uses the `ignore` crate (same as ripgrep) for .gitignore-aware traversal,
/// and git2 for batch git status collection.

use crate::bloom::{rebuild_bloom, BloomSet};
use crate::columns::{ColumnarIndex, GitStatus};
use git2::{Repository, Status};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

pub struct BuildResult {
    pub index: ColumnarIndex,
    pub bloom: BloomSet,
    pub duration_ms: u128,
}

pub fn build_index(root: &Path, respect_gitignore: bool) -> BuildResult {
    let start = std::time::Instant::now();

    let mut index = ColumnarIndex::new(root.to_string_lossy().to_string());

    // Batch-load git statuses upfront — one Repository::statuses() call
    // is far cheaper than checking each path individually
    let git_map = load_git_statuses(root);

    // Walk the tree
    let walker = WalkBuilder::new(root)
        .hidden(false)          // include hidden files
        .git_ignore(respect_gitignore)
        .git_global(respect_gitignore)
        .sort_by_file_path(|a, b| a.cmp(b))
        .build();

    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        // Skip the root itself
        if path == root { continue; }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let path_str = path.to_string_lossy().to_string();
        let is_dir = meta.is_dir();

        let size = if is_dir {
            // Recursive size — we compute this as a second pass below
            // for now store 0, patch later
            0u64
        } else {
            meta.len()
        };

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let ext = path.extension().and_then(|e| e.to_str());

        let git = git_map
            .get(&path_str)
            .copied()
            .unwrap_or(if git_map.is_empty() { GitStatus::Unknown } else { GitStatus::Clean });

        index.push(path_str, size, mtime, ext, git, is_dir);
    }

    // Second pass: compute recursive directory sizes
    // We walk children in sorted order, so parents come after children
    // (since we sorted by path). Accumulate child sizes into parent.
    patch_dir_sizes(&mut index);

    let n = index.len;
    let bloom = rebuild_bloom(&index.paths, n);

    BuildResult {
        index,
        bloom,
        duration_ms: start.elapsed().as_millis(),
    }
}

/// Patch directory sizes by summing child file sizes.
/// Relies on paths being sorted (children sort after parents lexicographically).
fn patch_dir_sizes(index: &mut ColumnarIndex) {
    // Build path → index map
    let path_to_idx: HashMap<&str, usize> = index
        .paths
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();

    // For each file, add its size to all parent directories
    let sizes_snapshot = index.sizes.clone();
    let flags_snapshot = index.dir_flags.clone();

    for i in 0..index.len {
        if flags_snapshot[i] == 1 { continue; } // skip dirs themselves

        let file_size = sizes_snapshot[i];
        let path = &index.paths[i];

        // Walk up parent chain
        let mut current = std::path::Path::new(path);
        while let Some(parent) = current.parent() {
            let parent_str = parent.to_str().unwrap_or("");
            if parent_str.is_empty() { break; }
            if let Some(&parent_idx) = path_to_idx.get(parent_str) {
                index.sizes[parent_idx] += file_size;
            }
            current = parent;
        }
    }
}

/// Load all git statuses for a repo in one batch call.
/// Returns a map of absolute path → GitStatus.
fn load_git_statuses(root: &Path) -> HashMap<String, GitStatus> {
    let mut map = HashMap::new();

    let Ok(repo) = Repository::discover(root) else { return map; };
    let Some(workdir) = repo.workdir() else { return map; };

    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);

    let Ok(statuses) = repo.statuses(Some(&mut opts)) else { return map; };

    for entry in statuses.iter() {
        let Some(path_str) = entry.path() else { continue };
        let abs = workdir.join(path_str);
        let status = map_git_status(entry.status());
        map.insert(abs.to_string_lossy().to_string(), status);
    }

    map
}

fn map_git_status(s: Status) -> GitStatus {
    if s.contains(Status::WT_NEW) || s.contains(Status::INDEX_NEW) {
        GitStatus::Untracked
    } else if s.contains(Status::WT_MODIFIED) || s.contains(Status::INDEX_MODIFIED) {
        GitStatus::Modified
    } else if s.contains(Status::INDEX_DELETED) || s.contains(Status::WT_DELETED) {
        GitStatus::Deleted
    } else if s.contains(Status::INDEX_RENAMED) || s.contains(Status::WT_RENAMED) {
        GitStatus::Renamed
    } else if s.contains(Status::IGNORED) {
        GitStatus::Ignored
    } else {
        GitStatus::Clean
    }
}
