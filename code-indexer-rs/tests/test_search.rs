use code_indexer::search::hybrid::rrf_fuse;
use code_indexer::storage::sqlite::SqliteStore;
use code_indexer::types::*;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_rrf_fusion() {
    let bm25_results = vec![
        ("file_a".to_string(), 1),
        ("file_b".to_string(), 2),
        ("file_c".to_string(), 3),
    ];
    let vector_results = vec![
        ("file_b".to_string(), 1),
        ("file_d".to_string(), 2),
        ("file_a".to_string(), 3),
    ];
    let fused = rrf_fuse(&bm25_results, &vector_results, 60, 1.0);
    assert!(fused.len() >= 3);
    assert_eq!(fused[0].0, "file_b");
}

#[test]
fn test_bm25_search_integration() {
    let dir = TempDir::new().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    let symbols = vec![
        Symbol {
            name: "process_data".to_string(),
            kind: SymbolKind::Function,
            language: Language::Rust,
            file_path: PathBuf::from("src/pipeline.rs"),
            line_start: 10,
            line_end: 30,
            signature: Some("pub fn process_data(input: &str) -> Result<Output>".to_string()),
            doc_comment: Some("Process raw data into output format".to_string()),
            visibility: Some("pub".to_string()),
            parent: None,
        },
        Symbol {
            name: "DataProcessor".to_string(),
            kind: SymbolKind::Struct,
            language: Language::Rust,
            file_path: PathBuf::from("src/processor.rs"),
            line_start: 5,
            line_end: 15,
            signature: Some("pub struct DataProcessor".to_string()),
            doc_comment: None,
            visibility: Some("pub".to_string()),
            parent: None,
        },
    ];
    store.insert_symbols(&symbols).unwrap();
    let results = store.search_symbols("process", 10).unwrap();
    assert_eq!(results.len(), 2);
}
