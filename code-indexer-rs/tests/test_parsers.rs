use code_indexer::parser::create_parser;
use code_indexer::types::*;
use std::path::Path;

#[test]
fn test_rust_parser_symbols() {
    let parser = create_parser(Language::Rust);
    let source = std::fs::read("tests/fixtures/rust_sample.rs").unwrap();
    let symbols = parser.parse_symbols(&source, Path::new("tests/fixtures/rust_sample.rs"));

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Config"), "should find struct Config");
    assert!(names.contains(&"Status"), "should find enum Status");
    assert!(
        names.contains(&"process_data"),
        "should find fn process_data"
    );
    assert!(
        names.contains(&"MAX_RETRIES"),
        "should find const MAX_RETRIES"
    );
    assert!(names.contains(&"new"), "should find method new");
    assert!(names.contains(&"Processor"), "should find trait Processor");

    // Check symbol details
    let process_data = symbols.iter().find(|s| s.name == "process_data").unwrap();
    assert_eq!(process_data.kind, SymbolKind::Function);
    assert_eq!(process_data.language, Language::Rust);
    assert!(process_data
        .signature
        .as_deref()
        .unwrap()
        .contains("process_data"));
    assert!(process_data
        .doc_comment
        .as_deref()
        .unwrap()
        .contains("Process the given input"));
    assert_eq!(process_data.visibility.as_deref(), Some("pub"));
}

#[test]
fn test_rust_parser_imports() {
    let parser = create_parser(Language::Rust);
    let source = std::fs::read("tests/fixtures/rust_sample.rs").unwrap();
    let imports = parser.parse_imports(&source, Path::new("tests/fixtures/rust_sample.rs"));

    assert!(imports.len() >= 2);
    let targets: Vec<&str> = imports.iter().map(|i| i.target_path.as_str()).collect();
    assert!(targets.contains(&"std::collections::HashMap"));
    assert!(targets.contains(&"std::path::Path"));
}

#[test]
fn test_rust_parser_chunks() {
    let parser = create_parser(Language::Rust);
    let source = std::fs::read("tests/fixtures/rust_sample.rs").unwrap();
    let chunks = parser.chunk_file(&source, Path::new("tests/fixtures/rust_sample.rs"));

    assert!(!chunks.is_empty(), "should produce at least one chunk");
    for chunk in &chunks {
        assert!(!chunk.content.is_empty());
        assert!(!chunk.content_hash.is_empty());
        assert!(chunk.line_start <= chunk.line_end);
    }
}

#[test]
fn test_parser_dispatch() {
    // All languages should have a parser
    for lang in [
        Language::Rust,
        Language::TypeScript,
        Language::Python,
        Language::Go,
        Language::Java,
        Language::C,
        Language::Cpp,
        Language::Ruby,
        Language::Swift,
    ] {
        let parser = create_parser(lang);
        assert_eq!(parser.language(), lang);
        assert!(!parser.extensions().is_empty());
    }
}
