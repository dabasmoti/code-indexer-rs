use crate::chunker;
use crate::types::*;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

pub struct TypeScriptParser {
    language: tree_sitter::Language,
}

impl TypeScriptParser {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }
}

impl super::LanguageParser for TypeScriptParser {
    fn language(&self) -> Language {
        Language::TypeScript
    }

    fn extensions(&self) -> &[&str] {
        &["ts", "tsx"]
    }

    fn parse_symbols(&self, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .expect("failed to set TypeScript language");
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return vec![],
        };

        let query_src = r#"
            (function_declaration name: (identifier) @name) @func
            (class_declaration name: (type_identifier) @name) @cls
            (interface_declaration name: (type_identifier) @name) @iface
            (enum_declaration name: (identifier) @name) @enu
            (type_alias_declaration name: (type_identifier) @name) @typ
            (lexical_declaration
                (variable_declarator name: (identifier) @name)) @var
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
                    "function_declaration" => SymbolKind::Function,
                    "class_declaration" => SymbolKind::Class,
                    "interface_declaration" => SymbolKind::Interface,
                    "enum_declaration" => SymbolKind::Enum,
                    "type_alias_declaration" => SymbolKind::Type,
                    "lexical_declaration" => SymbolKind::Variable,
                    _ => continue,
                };
                let sig = {
                    let text = node.utf8_text(source).unwrap_or("");
                    Some(text.lines().next().unwrap_or(text).to_string())
                };
                symbols.push(Symbol {
                    name: name_text.to_string(),
                    kind,
                    language: Language::TypeScript,
                    file_path: file_path.to_path_buf(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                    signature: sig,
                    doc_comment: None,
                    visibility: None,
                    parent: None,
                });
            }
        }
        symbols
    }

    fn parse_imports(&self, source: &[u8], file_path: &Path) -> Vec<Import> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .expect("failed to set TypeScript language");
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return vec![],
        };

        let query_src =
            "(import_statement source: (string (string_fragment) @path)) @imp";
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
            let mut imp_node = None;
            for cap in m.captures {
                let cap_name = capture_names[cap.index as usize];
                if cap_name == "path" {
                    path_text = cap.node.utf8_text(source).unwrap_or("");
                } else {
                    imp_node = Some(cap.node);
                }
            }
            if let Some(node) = imp_node {
                imports.push(Import {
                    source_file: file_path.to_path_buf(),
                    target_path: path_text.to_string(),
                    kind: ImportKind::Import,
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
            .expect("failed to set TypeScript language");
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => {
                return chunker::chunk_by_lines(
                    source_str,
                    file_path,
                    Language::TypeScript,
                    &[],
                )
            }
        };
        let mut boundaries = Vec::new();
        let root = tree.root_node();
        for i in 0..root.child_count() {
            if let Some(child) = root.child(i) {
                if child.is_named() {
                    boundaries.push(child.start_position().row);
                }
            }
        }
        chunker::chunk_by_lines(source_str, file_path, Language::TypeScript, &boundaries)
    }
}
