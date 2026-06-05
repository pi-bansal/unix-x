use serde::Serialize;
use similar::{ChangeTag, TextDiff};

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Equal,
}

#[derive(Serialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub changes: Vec<Change>,
}

#[derive(Serialize)]
pub struct Change {
    pub kind: ChangeKind,
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_lineno: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_lineno: Option<u32>,
}

#[derive(Serialize)]
pub struct FileDiff {
    pub old_path: String,
    pub new_path: String,
    pub added_lines: u32,
    pub removed_lines: u32,
    pub hunks: Vec<Hunk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename: Option<bool>,
}

pub fn diff_texts(old_path: &str, new_path: &str, old: &str, new: &str, context: usize) -> FileDiff {
    let diff = TextDiff::from_lines(old, new);

    let mut hunks = Vec::new();
    let mut total_added = 0u32;
    let mut total_removed = 0u32;

    for group in diff.grouped_ops(context) {
        let first = &group[0];
        let last = &group[group.len() - 1];

        let old_start = first.old_range().start as u32 + 1;
        let new_start = first.new_range().start as u32 + 1;
        let old_end = last.old_range().end as u32;
        let new_end = last.new_range().end as u32;

        let mut changes = Vec::new();
        let mut old_lineno = old_start;
        let mut new_lineno = new_start;

        for op in &group {
            for change in diff.iter_changes(op) {
                let (kind, oln, nln) = match change.tag() {
                    ChangeTag::Delete => {
                        let oln = old_lineno;
                        old_lineno += 1;
                        total_removed += 1;
                        (ChangeKind::Removed, Some(oln), None)
                    }
                    ChangeTag::Insert => {
                        let nln = new_lineno;
                        new_lineno += 1;
                        total_added += 1;
                        (ChangeKind::Added, None, Some(nln))
                    }
                    ChangeTag::Equal => {
                        let oln = old_lineno;
                        let nln = new_lineno;
                        old_lineno += 1;
                        new_lineno += 1;
                        (ChangeKind::Equal, Some(oln), Some(nln))
                    }
                };

                changes.push(Change {
                    kind,
                    line: change.value().trim_end_matches('\n').to_string(),
                    old_lineno: oln,
                    new_lineno: nln,
                });
            }
        }

        hunks.push(Hunk {
            old_start,
            old_lines: old_end.saturating_sub(old_start - 1),
            new_start,
            new_lines: new_end.saturating_sub(new_start - 1),
            changes,
        });
    }

    FileDiff {
        old_path: old_path.to_string(),
        new_path: new_path.to_string(),
        added_lines: total_added,
        removed_lines: total_removed,
        hunks,
        binary: None,
        rename: None,
    }
}

/// Check if content looks binary (has null bytes or high non-UTF8 ratio)
pub fn is_binary(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(8000)];
    sample.contains(&0u8) || {
        let non_utf8 = sample.iter().filter(|&&b| b > 127).count();
        non_utf8 as f32 / sample.len() as f32 > 0.3
    }
}
