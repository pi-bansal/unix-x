use crate::entry::GitStatus;
use git2::{Repository, Status};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct GitIndex {
    statuses: HashMap<PathBuf, GitStatus>,
    repo_root: PathBuf,
}

impl GitIndex {
    /// Try to open a git repo at or above `path` and index all statuses.
    /// Returns None if path is not inside a git repo.
    pub fn load(path: &Path) -> Option<Self> {
        let repo = Repository::discover(path).ok()?;
        let repo_root = repo.workdir()?.to_path_buf();

        let mut statuses = HashMap::new();

        let mut opts = {
            let mut o = git2::StatusOptions::new();
            o.include_untracked(true)
                .recurse_untracked_dirs(true)
                .include_ignored(false);
            o
        };

        if let Ok(status_list) = repo.statuses(Some(&mut opts)) {
            for entry in status_list.iter() {
                if let Some(path_str) = entry.path() {
                    let full_path = repo_root.join(path_str);
                    let status = map_status(entry.status());
                    statuses.insert(full_path.clone(), status);

                    // Also mark parent dirs as modified if any child is dirty
                    let mut parent = full_path.parent();
                    while let Some(p) = parent {
                        if p == repo_root {
                            break;
                        }
                        statuses
                            .entry(p.to_path_buf())
                            .or_insert(GitStatus::Modified);
                        parent = p.parent();
                    }
                }
            }
        }

        Some(GitIndex {
            statuses,
            repo_root,
        })
    }

    pub fn status_for(&self, path: &Path) -> Option<GitStatus> {
        // Canonicalize relative to repo root
        let canonical = path.canonicalize().ok()?;
        self.statuses
            .get(&canonical)
            .cloned()
            .or_else(|| {
                // If not in index and is inside the repo, it's clean
                if canonical.starts_with(&self.repo_root) {
                    Some(GitStatus::Clean)
                } else {
                    None
                }
            })
    }
}

fn map_status(s: Status) -> GitStatus {
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
