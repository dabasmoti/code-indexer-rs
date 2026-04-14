use code_indexer::config::Config;
use code_indexer::indexer::pipeline::IndexPipeline;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_index_small_repo() {
    let dir = TempDir::new().unwrap();
    let repo_dir = dir.path().join("repo");
    fs::create_dir(&repo_dir).unwrap();
    fs::write(
        repo_dir.join("main.rs"),
        r#"
        pub fn hello() -> String {
            "hello".to_string()
        }
        pub struct App {
            name: String,
        }
    "#,
    )
    .unwrap();
    fs::write(
        repo_dir.join("lib.py"),
        r#"
def process(data):
    return data.strip()

class Handler:
    def handle(self, request):
        pass
    "#,
    )
    .unwrap();

    let config = Config::default();
    let db_dir = dir.path().join(".code-indexer");
    let pipeline = IndexPipeline::new(&repo_dir, &db_dir, &config).unwrap();
    let stats = pipeline.run_full_index().await.unwrap();
    assert!(stats.files_indexed >= 2);
    assert!(stats.symbols_found >= 4);
}

#[tokio::test]
async fn test_index_respects_excludes() {
    let dir = TempDir::new().unwrap();
    let repo_dir = dir.path().join("repo");
    fs::create_dir_all(repo_dir.join("node_modules")).unwrap();
    fs::write(repo_dir.join("app.ts"), "export function main() {}").unwrap();
    fs::write(
        repo_dir.join("node_modules/dep.ts"),
        "export function dep() {}",
    )
    .unwrap();

    let config = Config::default();
    let db_dir = dir.path().join(".code-indexer");
    let pipeline = IndexPipeline::new(&repo_dir, &db_dir, &config).unwrap();
    let stats = pipeline.run_full_index().await.unwrap();
    assert_eq!(stats.files_indexed, 1);
}

#[tokio::test]
async fn test_full_pipeline_index_then_search() {
    let dir = TempDir::new().unwrap();
    let repo_dir = dir.path().join("repo");
    fs::create_dir(&repo_dir).unwrap();

    // Create a mini codebase
    fs::write(
        repo_dir.join("lib.rs"),
        r#"
use std::collections::HashMap;

/// A cache for storing key-value pairs.
pub struct Cache {
    data: HashMap<String, String>,
}

impl Cache {
    /// Create a new Cache with the given name.
    pub fn new() -> Self {
        Self { data: HashMap::new() }
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    pub fn set(&mut self, key: String, value: String) {
        self.data.insert(key, value);
    }
}

pub fn create_cache() -> Cache {
    Cache::new()
}
    "#,
    )
    .unwrap();

    let config = Config::default();
    let db_dir = dir.path().join(".code-indexer");
    let pipeline = IndexPipeline::new(&repo_dir, &db_dir, &config).unwrap();
    let stats = pipeline.run_full_index().await.unwrap();

    assert!(stats.files_indexed >= 1);
    assert!(stats.symbols_found >= 4); // Cache, new, get, set, create_cache

    // Now search
    let store = pipeline.store();
    let results = store.search_symbols("Cache", 10).unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|r| r.name == "Cache"));

    let results = store.search_symbols("create", 10).unwrap();
    assert!(results.iter().any(|r| r.name == "create_cache"));
}
