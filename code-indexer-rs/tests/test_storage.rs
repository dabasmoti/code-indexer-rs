use code_indexer::storage::sqlite::SqliteStore;
use code_indexer::types::*;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_create_store() {
    let dir = TempDir::new().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    let status = store.get_status().unwrap();
    assert_eq!(status.total_symbols, 0);
    assert_eq!(status.total_files, 0);
}

#[test]
fn test_insert_and_search_symbols() {
    let dir = TempDir::new().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    let symbol = Symbol {
        name: "process_data".to_string(),
        kind: SymbolKind::Function,
        language: Language::Rust,
        file_path: PathBuf::from("src/main.rs"),
        line_start: 10,
        line_end: 25,
        signature: Some("fn process_data(input: &str) -> Result<Output>".to_string()),
        doc_comment: Some("Processes raw input data".to_string()),
        visibility: Some("pub".to_string()),
        parent: None,
    };
    store.insert_symbols(&[symbol]).unwrap();
    let results = store.search_symbols("process", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "process_data");
}

#[test]
fn test_insert_and_retrieve_chunks() {
    let dir = TempDir::new().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    let chunk = Chunk {
        file_path: PathBuf::from("src/main.rs"),
        content: "fn hello() { println!(\"hello\"); }".to_string(),
        preamble: "mod main".to_string(),
        content_hash: "abc123".to_string(),
        line_start: 1,
        line_end: 3,
        language: Language::Rust,
    };
    let ids = store.insert_chunks(&[chunk]).unwrap();
    assert_eq!(ids.len(), 1);
    let retrieved = store.get_chunks_without_embeddings(100).unwrap();
    assert_eq!(retrieved.len(), 1);
}

#[test]
fn test_file_tracking() {
    let dir = TempDir::new().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    store
        .upsert_indexed_file("src/main.rs", "hash123", Language::Rust, 5)
        .unwrap();
    let hash = store.get_file_content_hash("src/main.rs").unwrap();
    assert_eq!(hash, Some("hash123".to_string()));
    store
        .upsert_indexed_file("src/main.rs", "hash456", Language::Rust, 3)
        .unwrap();
    let hash = store.get_file_content_hash("src/main.rs").unwrap();
    assert_eq!(hash, Some("hash456".to_string()));
}

#[test]
fn test_delete_file_data() {
    let dir = TempDir::new().unwrap();
    let store = SqliteStore::open(dir.path()).unwrap();
    let symbol = Symbol {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        language: Language::Rust,
        file_path: PathBuf::from("src/old.rs"),
        line_start: 1,
        line_end: 5,
        signature: None,
        doc_comment: None,
        visibility: None,
        parent: None,
    };
    store.insert_symbols(&[symbol]).unwrap();
    store
        .upsert_indexed_file("src/old.rs", "hash", Language::Rust, 1)
        .unwrap();
    store.delete_file_data("src/old.rs").unwrap();
    let results = store.search_symbols("foo", 10).unwrap();
    assert!(results.is_empty());
    let hash = store.get_file_content_hash("src/old.rs").unwrap();
    assert!(hash.is_none());
}
