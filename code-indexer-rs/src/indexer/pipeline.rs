use anyhow::{Context, Result};
use ignore::WalkBuilder;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::indexer::git::GitDetector;
use crate::parser::create_parser;
use crate::storage::sqlite::SqliteStore;
use crate::types::{DepEdge, Language};

#[derive(Debug, Default)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub symbols_found: usize,
    pub chunks_created: usize,
    pub files_skipped: usize,
}

pub struct IndexPipeline {
    repo_path: PathBuf,
    store: SqliteStore,
    git: GitDetector,
    config: Config,
}

impl IndexPipeline {
    pub fn new(repo_path: &Path, db_dir: &Path, config: &Config) -> Result<Self> {
        let store = SqliteStore::open(db_dir).context("Failed to open SQLite store")?;
        let git = GitDetector::open(repo_path);
        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            store,
            git,
            config: config.clone(),
        })
    }

    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    pub async fn run_full_index(&self) -> Result<IndexStats> {
        self.store.clear_all()?;
        let files = self.discover_files()?;
        self.index_files(files).await
    }

    pub async fn run_incremental_index(&self) -> Result<IndexStats> {
        let last_commit = self.store.get_meta("last_indexed_commit")?;

        if let Some(old_hash) = last_commit {
            if self.git.is_git_repo() {
                if let Ok(changed) = self.git.changed_files_since(&old_hash) {
                    let indexable: Vec<PathBuf> = changed
                        .into_iter()
                        .filter(|p| self.should_index(p))
                        .collect();
                    return self.index_files(indexable).await;
                }
            }
        }

        // Fall back to content hash comparison
        let all_files = self.discover_files()?;
        let mut changed_files = Vec::new();

        for file in all_files {
            let rel = file
                .strip_prefix(&self.repo_path)
                .unwrap_or(&file)
                .to_string_lossy()
                .to_string();

            let stored_hash = self.store.get_file_content_hash(&rel)?;
            let current_hash = content_hash(&file)?;

            match stored_hash {
                Some(h) if h == current_hash => {}
                _ => changed_files.push(file),
            }
        }

        self.index_files(changed_files).await
    }

    pub fn discover_files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        // WalkBuilder respects .gitignore, .ignore, and global git excludes automatically.
        let walker = WalkBuilder::new(&self.repo_path)
            .follow_links(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .hidden(true) // skip hidden files/dirs (dot-prefixed)
            .build();

        for entry in walker {
            match entry {
                Ok(e) if e.file_type().map(|t| t.is_file()).unwrap_or(false) => {
                    let path = e.into_path();
                    if self.should_index(&path) {
                        files.push(path);
                    }
                }
                _ => {}
            }
        }

        Ok(files)
    }

    fn is_excluded_dir_name(&self, name: &str) -> bool {
        self.config
            .indexer
            .exclude_dirs
            .iter()
            .any(|excluded| excluded == name)
    }

    pub fn is_excluded_dir(&self, path: &Path) -> bool {
        for component in path.components() {
            let name = component.as_os_str().to_string_lossy();
            if self.is_excluded_dir_name(&name) {
                return true;
            }
        }
        false
    }

    pub fn should_index(&self, path: &Path) -> bool {
        // Reject files in excluded directories
        if self.is_excluded_dir(path) {
            return false;
        }

        // Require a recognized extension
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => return false,
        };

        if Language::from_extension(ext).is_none() {
            return false;
        }

        // Reject files exceeding the configured size limit
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > self.config.indexer.max_file_size {
                return false;
            }
        }

        // Reject files matching exclude patterns
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        for pattern in &self.config.indexer.exclude_patterns {
            if simple_glob_match(pattern, &file_name) {
                return false;
            }
        }

        true
    }

    async fn index_files(&self, files: Vec<PathBuf>) -> Result<IndexStats> {
        let mut stats = IndexStats::default();

        for file_path in files {
            let rel_path = file_path
                .strip_prefix(&self.repo_path)
                .unwrap_or(&file_path)
                .to_path_buf();
            let rel_str = rel_path.to_string_lossy().to_string();

            let source_bytes = match std::fs::read(&file_path) {
                Ok(b) => b,
                Err(_) => {
                    stats.files_skipped += 1;
                    continue;
                }
            };

            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

            let language = match Language::from_extension(ext) {
                Some(l) => l,
                None => {
                    stats.files_skipped += 1;
                    continue;
                }
            };

            let hash = compute_bytes_hash(&source_bytes);

            // Remove stale data before re-indexing
            self.store.delete_file_data(&rel_str)?;

            let parser = create_parser(language);

            let symbols = parser.parse_symbols(&source_bytes, &rel_path);
            let imports = parser.parse_imports(&source_bytes, &rel_path);
            let chunks = parser.chunk_file(&source_bytes, &rel_path);

            let symbol_count = symbols.len();
            let chunk_count = chunks.len();

            if !symbols.is_empty() {
                self.store.insert_symbols(&symbols)?;
            }

            if !chunks.is_empty() {
                self.store.insert_chunks(&chunks)?;
            }

            let dep_edges: Vec<DepEdge> = imports
                .into_iter()
                .map(|imp| DepEdge {
                    source_file: imp.source_file,
                    target_path: imp.target_path,
                    kind: imp.kind,
                    line_number: imp.line_number,
                })
                .collect();

            if !dep_edges.is_empty() {
                self.store.insert_deps(&dep_edges)?;
            }

            self.store
                .upsert_indexed_file(&rel_str, &hash, language, symbol_count)?;

            stats.files_indexed += 1;
            stats.symbols_found += symbol_count;
            stats.chunks_created += chunk_count;
        }

        // Record the current HEAD commit hash after indexing
        if let Some(commit_hash) = self.git.head_commit_hash() {
            self.store.set_meta("last_indexed_commit", &commit_hash)?;
        }

        Ok(stats)
    }
}

fn content_hash(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))?;
    Ok(compute_bytes_hash(&bytes))
}

fn compute_bytes_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn simple_glob_match(pattern: &str, name: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix('*') {
        name.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        name.starts_with(prefix)
    } else {
        name.contains(pattern)
    }
}
