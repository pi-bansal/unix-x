use crate::entry::Entry;
use crate::git::GitIndex;
use ignore::WalkBuilder;
use std::path::Path;

pub struct WalkOptions {
    pub depth: usize,
    pub show_hidden: bool,
    pub respect_gitignore: bool,
    pub include_dirs: bool,
}

impl Default for WalkOptions {
    fn default() -> Self {
        WalkOptions {
            depth: 2,
            show_hidden: false,
            respect_gitignore: true,
            include_dirs: true,
        }
    }
}

pub fn walk(root: &Path, opts: &WalkOptions, git: Option<&GitIndex>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();

    let walker = WalkBuilder::new(root)
        .max_depth(Some(opts.depth))
        .hidden(!opts.show_hidden)
        .git_ignore(opts.respect_gitignore)
        .git_global(opts.respect_gitignore)
        .sort_by_file_name(|a, b| {
            // Dirs first, then files, both alphabetical
            a.cmp(b)
        })
        .build();

    for result in walker {
        let dir_entry = match result {
            Ok(e) => e,
            Err(err) => {
                // Emit structured error and continue
                eprintln!(
                    "{{\"error\": {}}}",
                    serde_json::to_string(&err.to_string())
                        .unwrap_or_else(|_| "\"walk error\"".to_string())
                );
                continue;
            }
        };

        let path = dir_entry.path();

        // Skip the root itself
        if path == root {
            continue;
        }

        let meta = match dir_entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.is_dir() && !opts.include_dirs {
            continue;
        }

        let git_status = git.and_then(|g| g.status_for(path));

        // For dirs: count direct children and compute recursive size
        let (recursive_size, child_count, size_truncated) = if meta.is_dir() {
            dir_stats(path)
        } else {
            (None, None, false)
        };

        let entry = Entry::from_metadata(path, &meta, git_status, recursive_size, child_count, size_truncated);
        entries.push(entry);
    }

    entries
}

/// Cap how many filesystem entries a single directory's recursive size/child
/// count will walk. Without this, listing a directory containing e.g.
/// `node_modules` (100k+ files) does a full unbounded walk just to show its
/// size — and that cost is paid again for every ancestor directory shown.
const MAX_STAT_ENTRIES: usize = 50_000;

/// Compute recursive byte size and direct child count for a directory.
/// Stops early (marking the result as a lower-bound/truncated) past
/// `MAX_STAT_ENTRIES` entries.
fn dir_stats(dir: &Path) -> (Option<u64>, Option<u64>, bool) {
    let mut total_size: u64 = 0;
    let mut child_count: u64 = 0;
    let mut visited: usize = 0;
    let mut truncated = false;

    for result in walkdir::WalkDir::new(dir).min_depth(1).into_iter() {
        if visited >= MAX_STAT_ENTRIES {
            truncated = true;
            break;
        }
        if let Ok(e) = result {
            visited += 1;
            // Count direct children
            if e.depth() == 1 {
                child_count += 1;
            }
            if let Ok(m) = e.metadata() {
                if m.is_file() {
                    total_size += m.len();
                }
            }
        }
    }

    (Some(total_size), Some(child_count), truncated)
}
