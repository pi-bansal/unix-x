//! Extraction of named declarations (functions, classes, types, …) — the
//! `ctags`-style view that is the highest-value output for agents.

use crate::convert::Pos;
use crate::lang::Lang;
use serde::Serialize;
use tree_sitter::Node;

#[derive(Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub start: Pos,
    pub end: Pos,
}

/// Walk the tree and collect every declaration node defined for `lang`.
pub fn extract(root: Node, src: &[u8], lang: Lang) -> Vec<Symbol> {
    let kinds = lang.symbol_kinds();
    let mut out = Vec::new();
    walk(root, src, kinds, &mut out);
    out
}

fn walk(node: Node, src: &[u8], kinds: &[&str], out: &mut Vec<Symbol>) {
    if kinds.contains(&node.kind()) {
        out.push(Symbol {
            name: symbol_name(&node, src),
            kind: node.kind().to_string(),
            start: pos(node.start_position(), node.start_byte()),
            end: pos(node.end_position(), node.end_byte()),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, kinds, out);
    }
}

/// Best-effort name for a declaration node: prefer the `name` field, then the
/// first `identifier`/`type_identifier` child, then a trimmed first line.
fn symbol_name(node: &Node, src: &[u8]) -> String {
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(t) = name_node.utf8_text(src) {
            return t.to_string();
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let k = child.kind();
        if k == "identifier" || k == "type_identifier" || k == "field_identifier" {
            if let Ok(t) = child.utf8_text(src) {
                return t.to_string();
            }
        }
    }

    // Fallback: first line of the node's text, trimmed.
    node.utf8_text(src)
        .ok()
        .and_then(|t| t.lines().next())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn pos(p: tree_sitter::Point, byte: usize) -> Pos {
    Pos {
        row: p.row,
        col: p.column,
        byte,
    }
}
