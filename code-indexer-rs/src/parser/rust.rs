use crate::chunker;
use crate::types::*;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

pub struct RustParser {
    language: tree_sitter::Language,
}

impl RustParser {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_rust::LANGUAGE.into(),
        }
    }

    fn get_doc_comment(&self, source: &[u8], node: tree_sitter::Node) -> Option<String> {
        let mut comments = Vec::new();
        let mut sibling = node.prev_sibling();
        while let Some(s) = sibling {
            if s.kind() == "line_comment" {
                let text = s.utf8_text(source).unwrap_or("");
                if text.starts_with("///") || text.starts_with("//!") {
                    comments.push(
                        text.trim_start_matches("///")
                            .trim_start_matches("//!")
                            .trim()
                            .to_string(),
                    );
                } else {
                    break;
                }
            } else if s.kind() == "block_comment" {
                let text = s.utf8_text(source).unwrap_or("");
                comments.push(text.to_string());
                break;
            } else {
                break;
            }
            sibling = s.prev_sibling();
        }
        if comments.is_empty() {
            None
        } else {
            comments.reverse();
            Some(comments.join(" "))
        }
    }

    fn get_visibility(&self, source: &[u8], node: tree_sitter::Node) -> Option<String> {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                if child.kind() == "visibility_modifier" {
                    return Some(child.utf8_text(source).unwrap_or("pub").to_string());
                }
            }
        }
        None
    }
}

impl super::LanguageParser for RustParser {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn parse_symbols(&self, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .expect("failed to set Rust language");
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return vec![],
        };

        let query_src = r#"
            (function_item name: (identifier) @name) @func
            (struct_item name: (type_identifier) @name) @struc
            (enum_item name: (type_identifier) @name) @enu
            (trait_item name: (type_identifier) @name) @trt
            (const_item name: (identifier) @name) @cnst
            (static_item name: (identifier) @name) @stat
            (type_item name: (type_identifier) @name) @typ
            (impl_item type: (type_identifier) @name) @imp
        "#;

        let query = match Query::new(&self.language, query_src) {
            Ok(q) => q,
            Err(_) => return vec![],
        };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source);
        let capture_names = query.capture_names();

        let mut symbols = Vec::new();

        while let Some(m) = matches.next() {
            let mut name_text = "";
            let mut outer_node = None;

            for cap in m.captures {
                let cap_name = capture_names[cap.index as usize];
                if cap_name == "name" {
                    name_text = cap.node.utf8_text(source).unwrap_or("");
                } else {
                    outer_node = Some(cap.node);
                }
            }

            if let Some(node) = outer_node {
                let kind = match node.kind() {
                    "function_item" => SymbolKind::Function,
                    "struct_item" => SymbolKind::Struct,
                    "enum_item" => SymbolKind::Enum,
                    "trait_item" => SymbolKind::Trait,
                    "const_item" | "static_item" => SymbolKind::Constant,
                    "type_item" => SymbolKind::Type,
                    "impl_item" => SymbolKind::Impl,
                    _ => continue,
                };

                let signature = {
                    let text = node.utf8_text(source).unwrap_or("");
                    let first_line = text.lines().next().unwrap_or(text);
                    Some(first_line.to_string())
                };

                symbols.push(Symbol {
                    name: name_text.to_string(),
                    kind,
                    language: Language::Rust,
                    file_path: file_path.to_path_buf(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                    signature,
                    doc_comment: self.get_doc_comment(source, node),
                    visibility: self.get_visibility(source, node),
                    parent: None,
                });
            }
        }

        // Also find methods inside impl blocks
        let method_query_src = r#"
            (impl_item
                type: (type_identifier) @impl_name
                body: (declaration_list
                    (function_item name: (identifier) @method_name) @method))
        "#;

        if let Ok(mq) = Query::new(&self.language, method_query_src) {
            let mut mcursor = QueryCursor::new();
            let mut mmatches = mcursor.matches(&mq, tree.root_node(), source);
            let mcap_names = mq.capture_names();

            while let Some(m) = mmatches.next() {
                let mut impl_name = "";
                let mut method_name = "";
                let mut method_node = None;

                for cap in m.captures {
                    let cap_name = &mcap_names[cap.index as usize];
                    match *cap_name {
                        "impl_name" => impl_name = cap.node.utf8_text(source).unwrap_or(""),
                        "method_name" => method_name = cap.node.utf8_text(source).unwrap_or(""),
                        "method" => method_node = Some(cap.node),
                        _ => {}
                    }
                }

                if let Some(node) = method_node {
                    let signature = {
                        let text = node.utf8_text(source).unwrap_or("");
                        let first_line = text.lines().next().unwrap_or(text);
                        Some(first_line.to_string())
                    };

                    symbols.push(Symbol {
                        name: method_name.to_string(),
                        kind: SymbolKind::Method,
                        language: Language::Rust,
                        file_path: file_path.to_path_buf(),
                        line_start: node.start_position().row + 1,
                        line_end: node.end_position().row + 1,
                        signature,
                        doc_comment: self.get_doc_comment(source, node),
                        visibility: self.get_visibility(source, node),
                        parent: Some(impl_name.to_string()),
                    });
                }
            }
        }

        symbols
    }

    fn parse_imports(&self, source: &[u8], file_path: &Path) -> Vec<Import> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .expect("failed to set Rust language");
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return vec![],
        };

        let query_src = "(use_declaration argument: (_) @path) @use_decl";
        let query = match Query::new(&self.language, query_src) {
            Ok(q) => q,
            Err(_) => return vec![],
        };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source);
        let capture_names = query.capture_names();

        let mut imports = Vec::new();
        while let Some(m) = matches.next() {
            let mut path_text = "";
            let mut use_node = None;
            for cap in m.captures {
                let cap_name = capture_names[cap.index as usize];
                if cap_name == "path" {
                    path_text = cap.node.utf8_text(source).unwrap_or("");
                } else {
                    use_node = Some(cap.node);
                }
            }
            if let Some(node) = use_node {
                imports.push(Import {
                    source_file: file_path.to_path_buf(),
                    target_path: path_text.to_string(),
                    kind: ImportKind::Use,
                    line_number: node.start_position().row + 1,
                });
            }
        }

        imports
    }

    fn chunk_file(&self, source: &[u8], file_path: &Path) -> Vec<Chunk> {
        let source_str = std::str::from_utf8(source).unwrap_or("");

        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .expect("failed to set Rust language");
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return chunker::chunk_by_lines(source_str, file_path, Language::Rust, &[]),
        };

        // Collect top-level item start lines as chunk boundaries
        let mut boundaries = Vec::new();
        let root = tree.root_node();
        for i in 0..root.child_count() {
            if let Some(child) = root.child(i) {
                if child.is_named() {
                    boundaries.push(child.start_position().row);
                }
            }
        }

        chunker::chunk_by_lines(source_str, file_path, Language::Rust, &boundaries)
    }
}
