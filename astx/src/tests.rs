//! Unit tests for the astx parsing/conversion/symbol layers.

use crate::convert::{collect_kinds, convert};
use crate::lang::Lang;
use crate::symbols::extract;

fn parse(lang: Lang, src: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang.language()).unwrap();
    parser.parse(src, None).unwrap()
}

#[test]
fn detects_language_by_extension() {
    assert_eq!(Lang::from_ext("rs"), Some(Lang::Rust));
    assert_eq!(Lang::from_ext("py"), Some(Lang::Python));
    assert_eq!(Lang::from_ext("tsx"), Some(Lang::Tsx));
    assert_eq!(Lang::from_ext("zig"), None);
}

#[test]
fn rust_root_kind_and_function() {
    let src = "fn main() { let x = 1; }";
    let tree = parse(Lang::Rust, src);
    let node = convert(tree.root_node(), src.as_bytes(), None, None);
    assert_eq!(node.kind, "source_file");
    assert!(node.named);
    // Somewhere in the tree there is a function_item.
    fn has_kind(n: &crate::convert::AstNode, k: &str) -> bool {
        n.kind == k || n.children.iter().any(|c| has_kind(c, k))
    }
    assert!(has_kind(&node, "function_item"));
}

#[test]
fn rust_symbols_extracted() {
    let src = "fn alpha() {}\nstruct Beta;\nfn gamma() {}";
    let tree = parse(Lang::Rust, src);
    let syms = extract(tree.root_node(), src.as_bytes(), Lang::Rust);
    let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"Beta"));
    assert!(names.contains(&"gamma"));
}

#[test]
fn python_symbols_extracted() {
    let src = "def foo():\n    pass\n\nclass Bar:\n    def method(self):\n        pass\n";
    let tree = parse(Lang::Python, src);
    let syms = extract(tree.root_node(), src.as_bytes(), Lang::Python);
    let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"foo"));
    assert!(names.contains(&"Bar"));
    assert!(names.contains(&"method"));
}

#[test]
fn collect_kinds_flattens_matches() {
    let src = "fn a() {}\nfn b() {}\nfn c() {}";
    let tree = parse(Lang::Rust, src);
    let mut out = Vec::new();
    collect_kinds(
        tree.root_node(),
        src.as_bytes(),
        &["function_item".to_string()],
        None,
        &mut out,
    );
    assert_eq!(out.len(), 3);
}

#[test]
fn depth_limit_caps_children() {
    let src = "fn main() { let x = 1; }";
    let tree = parse(Lang::Rust, src);
    let node = convert(tree.root_node(), src.as_bytes(), Some(0), None);
    assert!(node.children.is_empty(), "depth 0 should yield no children");
}
