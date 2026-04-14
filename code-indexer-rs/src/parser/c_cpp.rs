use crate::chunker;
use crate::types::*;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

pub struct CParser {
    language: tree_sitter::Language,
}

pub struct CppParser {
    language: tree_sitter::Language,
}

impl CParser {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_c::LANGUAGE.into(),
        }
    }
}

impl CppParser {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_cpp::LANGUAGE.into(),
        }
    }
}

fn parse_c_family_symbols(
    language: &tree_sitter::Language,
    lang: Language,
    source: &[u8],
    file_path: &Path,
) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .expect("failed to set C/C++ language");
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return vec![],
    };

    let query_src = r#"
        (function_definition declarator: (function_declarator declarator: (identifier) @name)) @func
        (struct_specifier name: (type_identifier) @name) @struc
        (enum_specifier name: (type_identifier) @name) @enu
        (declaration declarator: (function_declarator declarator: (identifier) @name)) @decl
    "#;
    let query = match Query::new(language, query_src) {
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
                "function_definition" => SymbolKind::Function,
                "struct_specifier" => SymbolKind::Struct,
                "enum_specifier" => SymbolKind::Enum,
                "declaration" => SymbolKind::Function,
                _ => continue,
            };
            let sig = Some(
                node.utf8_text(source)
                    .unwrap_or("")
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string(),
            );
            symbols.push(Symbol {
                name: name_text.to_string(),
                kind,
                language: lang,
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

fn parse_c_family_imports(
    language: &tree_sitter::Language,
    source: &[u8],
    file_path: &Path,
) -> Vec<Import> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .expect("failed to set C/C++ language");
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return vec![],
    };

    let query_src = r#"(preproc_include path: (_) @path) @inc"#;
    let query = match Query::new(language, query_src) {
        Ok(q) => q,
        Err(_) => return vec![],
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source);
    let capture_names = query.capture_names();
    let mut imports = Vec::new();
    while let Some(m) = matches.next() {
        let mut path_text = "";
        let mut inc_node = None;
        for cap in m.captures {
            let cap_name = capture_names[cap.index as usize];
            if cap_name == "path" {
                path_text = cap.node.utf8_text(source).unwrap_or("");
            } else {
                inc_node = Some(cap.node);
            }
        }
        if let Some(node) = inc_node {
            imports.push(Import {
                source_file: file_path.to_path_buf(),
                target_path: path_text
                    .trim_matches(|c| c == '"' || c == '<' || c == '>')
                    .to_string(),
                kind: ImportKind::Include,
                line_number: node.start_position().row + 1,
            });
        }
    }
    imports
}

fn chunk_c_family(
    language: &tree_sitter::Language,
    lang: Language,
    source: &[u8],
    file_path: &Path,
) -> Vec<Chunk> {
    let source_str = std::str::from_utf8(source).unwrap_or("");
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .expect("failed to set C/C++ language");
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return chunker::chunk_by_lines(source_str, file_path, lang, &[]),
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
    chunker::chunk_by_lines(source_str, file_path, lang, &boundaries)
}

impl super::LanguageParser for CParser {
    fn language(&self) -> Language {
        Language::C
    }

    fn extensions(&self) -> &[&str] {
        &["c", "h"]
    }

    fn parse_symbols(&self, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        parse_c_family_symbols(&self.language, Language::C, source, file_path)
    }

    fn parse_imports(&self, source: &[u8], file_path: &Path) -> Vec<Import> {
        parse_c_family_imports(&self.language, source, file_path)
    }

    fn chunk_file(&self, source: &[u8], file_path: &Path) -> Vec<Chunk> {
        chunk_c_family(&self.language, Language::C, source, file_path)
    }
}

impl super::LanguageParser for CppParser {
    fn language(&self) -> Language {
        Language::Cpp
    }

    fn extensions(&self) -> &[&str] {
        &["cpp", "cc", "cxx", "hpp", "hxx"]
    }

    fn parse_symbols(&self, source: &[u8], file_path: &Path) -> Vec<Symbol> {
        parse_c_family_symbols(&self.language, Language::Cpp, source, file_path)
    }

    fn parse_imports(&self, source: &[u8], file_path: &Path) -> Vec<Import> {
        parse_c_family_imports(&self.language, source, file_path)
    }

    fn chunk_file(&self, source: &[u8], file_path: &Path) -> Vec<Chunk> {
        chunk_c_family(&self.language, Language::Cpp, source, file_path)
    }
}
