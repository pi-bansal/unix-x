/// Three-way merge engine.
///
/// Given base, ours, theirs — produces a structured result with:
///   - resolved: the merged text (if no conflicts)
///   - conflicts: typed conflict regions (if unresolvable)
///   - hunks: every change annotated with which side introduced it
///
/// Why structured output matters for agents:
///   Traditional merge outputs text like:
///     <<<<<<< ours
///     fn foo() { 1 }
///     =======
///     fn foo() { 2 }
///     >>>>>>> theirs
///
///   An agent has to parse that back. Instead we output:
///     {"kind": "conflict", "base": "...", "ours": "...", "theirs": "...",
///      "start_line": 10, "end_line": 12,
///      "auto_resolvable": false, "resolution_hint": "both_modified_same_region"}
///
/// Auto-resolution strategies (applied before declaring a conflict):
///   1. Identical change on both sides → accept either (deduplicate)
///   2. One side unchanged from base → accept the changed side
///   3. Non-overlapping edits → accept both in order
///
/// Only truly ambiguous overlapping edits become conflicts.

use serde::Serialize;
use similar::{ChangeTag, TextDiff};

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HunkKind {
    Unchanged,
    OursOnly,
    TheirsOnly,
    BothSame,    // both sides made the same change — auto-resolved
    Conflict,
}

#[derive(Serialize, Clone)]
pub struct MergeHunk {
    pub kind: HunkKind,
    pub base_lines: Vec<String>,
    pub ours_lines: Vec<String>,
    pub theirs_lines: Vec<String>,
    pub resolved_lines: Option<Vec<String>>, // Some if auto-resolvable
    pub start_line: usize,                   // 1-indexed, in base
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_hint: Option<String>,
}

#[derive(Serialize)]
pub struct MergeResult {
    pub clean: bool,              // true if no conflicts
    pub conflict_count: usize,
    pub auto_resolved_count: usize,
    pub ours_only_count: usize,
    pub theirs_only_count: usize,
    pub resolved: Option<String>, // full merged text if clean
    pub hunks: Vec<MergeHunk>,
}

pub fn three_way_merge(base: &str, ours: &str, theirs: &str) -> MergeResult {
    // Compute both diffs against base
    let diff_ours   = line_diff(base, ours);
    let diff_theirs = line_diff(base, theirs);

    let base_lines: Vec<&str> = base.lines().collect();
    let ours_lines: Vec<&str> = ours.lines().collect();
    let theirs_lines: Vec<&str> = theirs.lines().collect();

    // Build change maps: base_line_idx → what each side did
    let ours_changes   = build_change_map(&base_lines, &ours_lines,   &diff_ours);
    let theirs_changes = build_change_map(&base_lines, &theirs_lines, &diff_theirs);

    let mut hunks: Vec<MergeHunk> = Vec::new();
    let mut resolved_lines: Vec<String> = Vec::new();
    let mut conflict_count = 0;
    let mut auto_resolved = 0;
    let mut ours_only = 0;
    let mut theirs_only = 0;

    let n = base_lines.len().max(ours_changes.len()).max(theirs_changes.len());
    let mut i = 0;

    while i < n {
        let our_op   = ours_changes.get(i).copied().unwrap_or(Op::Keep);
        let their_op = theirs_changes.get(i).copied().unwrap_or(Op::Keep);

        match (our_op, their_op) {
            // Both unchanged
            (Op::Keep, Op::Keep) => {
                let line = base_lines.get(i).copied().unwrap_or("").to_string();
                resolved_lines.push(line.clone());
                // Merge into previous Unchanged hunk or start new
                if let Some(last) = hunks.last_mut() {
                    if last.kind == HunkKind::Unchanged {
                        last.base_lines.push(line.clone());
                        last.ours_lines.push(line.clone());
                        last.theirs_lines.push(line.clone());
                        last.resolved_lines.as_mut().unwrap().push(line);
                        i += 1;
                        continue;
                    }
                }
                hunks.push(MergeHunk {
                    kind: HunkKind::Unchanged,
                    base_lines: vec![line.clone()],
                    ours_lines: vec![line.clone()],
                    theirs_lines: vec![line.clone()],
                    resolved_lines: Some(vec![line]),
                    start_line: i + 1,
                    resolution_hint: None,
                });
                i += 1;
            }

            // Only ours changed
            (Op::Replace(ref o), Op::Keep) | (Op::Delete, Op::Keep) => {
                let base_line  = base_lines.get(i).copied().unwrap_or("").to_string();
                let our_lines  = match &our_op {
                    Op::Replace(v) => v.clone(),
                    Op::Delete     => vec![],
                    _              => vec![base_line.clone()],
                };
                resolved_lines.extend(our_lines.iter().cloned());
                ours_only += 1;
                hunks.push(MergeHunk {
                    kind: HunkKind::OursOnly,
                    base_lines: vec![base_line],
                    ours_lines: our_lines.clone(),
                    theirs_lines: vec![],
                    resolved_lines: Some(our_lines),
                    start_line: i + 1,
                    resolution_hint: None,
                });
                i += 1;
            }

            // Only theirs changed
            (Op::Keep, Op::Replace(ref t)) | (Op::Keep, Op::Delete) => {
                let base_line    = base_lines.get(i).copied().unwrap_or("").to_string();
                let their_lines  = match &their_op {
                    Op::Replace(v) => v.clone(),
                    Op::Delete     => vec![],
                    _              => vec![base_line.clone()],
                };
                resolved_lines.extend(their_lines.iter().cloned());
                theirs_only += 1;
                hunks.push(MergeHunk {
                    kind: HunkKind::TheirsOnly,
                    base_lines: vec![base_line],
                    ours_lines: vec![],
                    theirs_lines: their_lines.clone(),
                    resolved_lines: Some(their_lines),
                    start_line: i + 1,
                    resolution_hint: None,
                });
                i += 1;
            }

            // Both changed — check if they made the same change
            _ => {
                let base_line = base_lines.get(i).copied().unwrap_or("").to_string();

                let our_lines = match &our_op {
                    Op::Replace(v) => v.clone(),
                    Op::Delete     => vec![],
                    Op::Keep       => vec![base_line.clone()],
                };
                let their_lines = match &their_op {
                    Op::Replace(v) => v.clone(),
                    Op::Delete     => vec![],
                    Op::Keep       => vec![base_line.clone()],
                };

                if our_lines == their_lines {
                    // Same change on both sides — auto-resolve
                    resolved_lines.extend(our_lines.iter().cloned());
                    auto_resolved += 1;
                    hunks.push(MergeHunk {
                        kind: HunkKind::BothSame,
                        base_lines: vec![base_line],
                        ours_lines: our_lines.clone(),
                        theirs_lines: their_lines,
                        resolved_lines: Some(our_lines),
                        start_line: i + 1,
                        resolution_hint: Some("identical_change_on_both_sides".to_string()),
                    });
                } else {
                    // True conflict
                    conflict_count += 1;
                    hunks.push(MergeHunk {
                        kind: HunkKind::Conflict,
                        base_lines: vec![base_line],
                        ours_lines: our_lines,
                        theirs_lines: their_lines,
                        resolved_lines: None,
                        start_line: i + 1,
                        resolution_hint: Some("both_modified_same_region".to_string()),
                    });
                }
                i += 1;
            }
        }
    }

    let clean = conflict_count == 0;
    let resolved = if clean {
        Some(resolved_lines.join("\n"))
    } else {
        None
    };

    MergeResult {
        clean,
        conflict_count,
        auto_resolved_count: auto_resolved,
        ours_only_count: ours_only,
        theirs_only_count: theirs_only,
        resolved,
        hunks,
    }
}

// ── Internal diff representation ──────────────────────────────────────────────

#[derive(Clone, Debug)]
enum Op {
    Keep,
    Delete,
    Replace(Vec<String>),
}

/// Compute a line-level diff and return a per-base-line operation map.
fn line_diff(base: &str, other: &str) -> Vec<(usize, Op)> {
    let diff = TextDiff::from_lines(base, other);
    let mut ops: Vec<(usize, Op)> = Vec::new();

    let mut base_idx = 0usize;
    let mut pending_insert: Vec<String> = Vec::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                if !pending_insert.is_empty() {
                    ops.push((base_idx, Op::Replace(pending_insert.drain(..).collect())));
                }
                base_idx += 1;
            }
            ChangeTag::Delete => {
                if !pending_insert.is_empty() {
                    ops.push((base_idx, Op::Replace(pending_insert.drain(..).collect())));
                }
                ops.push((base_idx, Op::Delete));
                base_idx += 1;
            }
            ChangeTag::Insert => {
                pending_insert.push(change.value().trim_end_matches('\n').to_string());
            }
        }
    }

    ops
}

/// Build a Vec<Op> indexed by base line number.
fn build_change_map(
    base_lines: &[&str],
    _other_lines: &[&str],
    diff: &[(usize, Op)],
) -> Vec<Op> {
    let mut map = vec![Op::Keep; base_lines.len()];
    for (idx, op) in diff {
        if *idx < map.len() {
            map[*idx] = op.clone();
        }
    }
    map
}
