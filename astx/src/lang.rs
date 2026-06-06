//! Language detection and per-language tree-sitter grammar wiring.

use tree_sitter::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
}

impl Lang {
    /// Resolve a language from an explicit `--lang` value.
    pub fn from_name(name: &str) -> Option<Lang> {
        match name.to_lowercase().as_str() {
            "rust" | "rs" => Some(Lang::Rust),
            "python" | "py" => Some(Lang::Python),
            "javascript" | "js" | "jsx" => Some(Lang::JavaScript),
            "typescript" | "ts" => Some(Lang::TypeScript),
            "tsx" => Some(Lang::Tsx),
            "go" | "golang" => Some(Lang::Go),
            _ => None,
        }
    }

    /// Resolve a language from a file extension (without the dot).
    pub fn from_ext(ext: &str) -> Option<Lang> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(Lang::Rust),
            "py" | "pyi" => Some(Lang::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
            "ts" | "mts" | "cts" => Some(Lang::TypeScript),
            "tsx" => Some(Lang::Tsx),
            "go" => Some(Lang::Go),
            _ => None,
        }
    }

    /// Human-readable name used in output and messages.
    pub fn name(&self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::Go => "go",
        }
    }

    /// The tree-sitter grammar for this language.
    pub fn language(&self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    /// The set of node kinds treated as declarations for `--symbols`.
    pub fn symbol_kinds(&self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "function_item",
                "struct_item",
                "enum_item",
                "trait_item",
                "impl_item",
                "mod_item",
                "const_item",
                "static_item",
                "type_item",
                "macro_definition",
                "union_item",
            ],
            Lang::Python => &["function_definition", "class_definition"],
            Lang::JavaScript => &[
                "function_declaration",
                "generator_function_declaration",
                "class_declaration",
                "method_definition",
            ],
            Lang::TypeScript | Lang::Tsx => &[
                "function_declaration",
                "generator_function_declaration",
                "class_declaration",
                "method_definition",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "abstract_class_declaration",
            ],
            Lang::Go => &[
                "function_declaration",
                "method_declaration",
                "type_declaration",
                "const_declaration",
                "var_declaration",
            ],
        }
    }

    /// Comma-separated list of all supported languages, for messages.
    pub fn supported() -> &'static str {
        "rust, python, javascript, typescript, tsx, go"
    }
}
