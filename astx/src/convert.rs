//! Conversion of a tree-sitter parse tree into serializable JSON nodes.

use serde::Serialize;
use tree_sitter::Node;

#[derive(Serialize)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
    pub byte: usize,
}

impl Pos {
    fn from(p: tree_sitter::Point, byte: usize) -> Self {
        Pos {
            row: p.row,
            col: p.column,
            byte,
        }
    }
}

#[derive(Serialize)]
pub struct AstNode {
    pub kind: String,
    pub named: bool,
    pub start: Pos,
    pub end: Pos,
    /// Source text — only included for leaf nodes to avoid duplicating the
    /// whole file at every level of the tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Field name this node occupies in its parent (e.g. "name", "body"), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<AstNode>,
}

/// Convert a node (and its subtree) into an `AstNode`.
///
/// `depth_limit` caps recursion: `Some(0)` yields just this node with no
/// children, `None` means unlimited. `field` is the node's field name in its
/// parent, if any.
pub fn convert(
    node: Node,
    src: &[u8],
    depth_limit: Option<usize>,
    field: Option<String>,
) -> AstNode {
    let leaf = node.child_count() == 0;
    let text = if leaf {
        node.utf8_text(src).ok().map(|s| s.to_string())
    } else {
        None
    };

    let mut children = Vec::new();
    let recurse = depth_limit.map(|d| d > 0).unwrap_or(true);
    if recurse {
        let next_limit = depth_limit.map(|d| d - 1);
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                let child_field = cursor.field_name().map(|s| s.to_string());
                children.push(convert(child, src, next_limit, child_field));
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    AstNode {
        kind: node.kind().to_string(),
        named: node.is_named(),
        start: Pos::from(node.start_position(), node.start_byte()),
        end: Pos::from(node.end_position(), node.end_byte()),
        text,
        field,
        children,
    }
}

/// Walk the tree and collect every node whose kind is in `kinds` as a flat list.
pub fn collect_kinds(
    node: Node,
    src: &[u8],
    kinds: &[String],
    depth_limit: Option<usize>,
    out: &mut Vec<AstNode>,
) {
    if kinds.iter().any(|k| k == node.kind()) {
        out.push(convert(node, src, depth_limit, None));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_kinds(child, src, kinds, depth_limit, out);
    }
}
