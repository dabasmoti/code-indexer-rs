use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::embedding::detection::detect_provider;
use crate::embedding::EmbeddingProvider;
use crate::indexer::pipeline::IndexPipeline;
use crate::search::SearchCoordinator;
use crate::storage::sqlite::SqliteStore;
use crate::storage::vector::VectorIndex;

// ---------------------------------------------------------------------------
// Parameter structs for each tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindSymbolParams {
    /// Symbol name or partial name to search for (e.g. "create_user", "AuthService", "handle_")
    pub query: String,
    /// Filter by symbol kind: "function", "method", "struct", "class", "interface", "trait", "enum", "type_alias", "constant", "module"
    pub kind: Option<String>,
    /// Filter by language: "rust", "typescript", "python", "go", "java", "c", "cpp", "ruby", "swift"
    pub language: Option<String>,
    /// Filter by file path substring (e.g. "routers/" or "auth")
    pub file_path_contains: Option<String>,
    /// Maximum results to return (default: 20)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchCodeParams {
    /// Natural language query describing what you're looking for (e.g. "how are JWT tokens validated", "database connection pooling", "error handling in API routes"). Can also be a keyword or code snippet.
    pub query: String,
    /// Filter by language: "rust", "typescript", "python", "go", "java", "c", "cpp", "ruby", "swift"
    pub language: Option<String>,
    /// Filter by file path substring (e.g. "backend/app" or "tests/")
    pub file_path_contains: Option<String>,
    /// Maximum results to return (default: 20)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryDepsParams {
    /// File path relative to the repository root (e.g. "src/main.rs", "backend/app/deps.py")
    pub file: Option<String>,
    /// "outgoing" = what this file imports, "incoming" = what imports this file, "both" = both directions (default: "both")
    pub direction: Option<String>,
    /// Maximum results per direction (default: 50)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReindexParams {
    /// Pass true to force a full re-index (clears existing data). Default false = incremental, only re-indexes changed files.
    pub full: Option<bool>,
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CodeIndexerServer {
    repo_path: PathBuf,
    db_dir: PathBuf,
    config: Config,
    store: Arc<Mutex<SqliteStore>>,
    vector_index: Arc<RwLock<Option<VectorIndex>>>,
    provider: Arc<RwLock<Option<Box<dyn EmbeddingProvider>>>>,
}

impl CodeIndexerServer {
    pub async fn new(repo_path: PathBuf, config: Config) -> Result<Self> {
        let db_dir = repo_path.join(&config.storage.db_dir);

        let store = SqliteStore::open(&db_dir)?;

        let provider = detect_provider(&config).await;

        let vector_index = if let Some(ref p) = provider {
            let dims = p.dimensions();
            VectorIndex::open(&db_dir, dims).ok()
        } else {
            None
        };

        Ok(Self {
            repo_path,
            db_dir,
            config,
            store: Arc::new(Mutex::new(store)),
            vector_index: Arc::new(RwLock::new(vector_index)),
            provider: Arc::new(RwLock::new(provider)),
        })
    }
}

// ---------------------------------------------------------------------------
// Tool implementations via rmcp macros
// ---------------------------------------------------------------------------

#[tool_router]
impl CodeIndexerServer {
    /// Search for symbols (functions, structs, classes, etc.) by name using BM25 full-text search.
    #[tool(
        description = "Find code symbols (functions, classes, structs, methods, traits, enums) by name. Use this when you know the symbol name or part of it. Best for: locating definitions, finding all functions matching a pattern, discovering API surface area. Returns symbol metadata including signature, doc comments, visibility, and file location."
    )]
    pub async fn find_symbol(&self, Parameters(params): Parameters<FindSymbolParams>) -> String {
        let limit = params.limit.unwrap_or(self.config.search.default_limit);

        let store = self.store.lock().await;
        let coordinator = SearchCoordinator::new(&store, None, None, self.config.search.clone());

        match coordinator.find_symbol(
            &params.query,
            params.kind.as_deref(),
            params.language.as_deref(),
            params.file_path_contains.as_deref(),
            limit,
        ) {
            Ok(results) => {
                let output: Vec<serde_json::Value> = results
                    .into_iter()
                    .map(|r| {
                        serde_json::json!({
                            "name": r.name,
                            "kind": r.kind.as_str(),
                            "file_path": r.file_path.to_string_lossy(),
                            "line_start": r.line_start,
                            "line_end": r.line_end,
                            "signature": r.signature,
                            "doc_comment": r.doc_comment,
                            "visibility": r.visibility,
                        })
                    })
                    .collect();
                serde_json::to_string(&output).unwrap_or_else(|_| "[]".to_string())
            }
            Err(e) => format!("{{\"error\": \"{}\"}}", e),
        }
    }

    /// Perform a hybrid semantic + keyword search over indexed code chunks.
    #[tool(
        description = "Search code using natural language or keywords with hybrid semantic + keyword matching. Use this when you want to understand how something works, find implementations of a concept, or locate code by describing its behavior. Best for: 'how does X work', 'where is Y handled', conceptual searches. Returns ranked code snippets with file paths and line numbers."
    )]
    pub async fn search_code(&self, Parameters(params): Parameters<SearchCodeParams>) -> String {
        use crate::search::bm25;
        use crate::search::hybrid;

        let limit = params.limit.unwrap_or(self.config.search.default_limit);
        let limit = limit.min(self.config.search.max_limit);
        let query = params.query.clone();

        // Step 1: obtain the BM25 results while holding the store lock, then release it.
        let bm25_results = {
            let store = self.store.lock().await;
            match bm25::code_search(&store, &query, limit * 2) {
                Ok(r) => r,
                Err(e) => return format!("{{\"error\": \"{}\"}}", e),
            }
        };

        // Step 2: vector search (async, does not need the store lock for the embed call).
        let vector_results = {
            let prov_guard = self.provider.read().await;
            if let Some(provider) = prov_guard.as_ref() {
                // Embed the query first (no store/index locks held).
                let embedding = match provider.embed_query(&query).await {
                    Ok(vec) => vec,
                    Err(_) => vec![],
                };

                if embedding.is_empty() {
                    vec![]
                } else {
                    // Then search the vector index.
                    let vi_guard = self.vector_index.read().await;
                    if let Some(vi) = vi_guard.as_ref() {
                        let store = self.store.lock().await;
                        match vi.search(&embedding, limit * 2) {
                            Ok(hits) => hits
                                .into_iter()
                                .filter_map(|(chunk_id, dist)| {
                                    let chunk = store.get_chunk_by_id(chunk_id as i64).ok()??;
                                    Some(crate::types::SearchResult {
                                        file_path: chunk.file_path,
                                        line_start: chunk.line_start,
                                        line_end: chunk.line_end,
                                        snippet: chunk.content,
                                        symbol_name: None,
                                        symbol_kind: None,
                                        score: 1.0 - dist as f64,
                                        language: chunk.language,
                                    })
                                })
                                .collect::<Vec<_>>(),
                            Err(_) => vec![],
                        }
                    } else {
                        vec![]
                    }
                }
            } else {
                vec![]
            }
        };

        // Step 3: fuse or return BM25 results.
        let results = if vector_results.is_empty() {
            bm25_results.into_iter().take(limit).collect::<Vec<_>>()
        } else {
            hybrid::fuse_search_results(
                &bm25_results,
                &vector_results,
                self.config.search.rrf_k,
                self.config.search.bm25_symbol_boost,
                limit,
            )
        };

        let output: Vec<serde_json::Value> = results
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "file_path": r.file_path.to_string_lossy(),
                    "line_start": r.line_start,
                    "line_end": r.line_end,
                    "snippet": r.snippet,
                    "symbol_name": r.symbol_name,
                    "score": r.score,
                })
            })
            .collect();
        serde_json::to_string(&output).unwrap_or_else(|_| "[]".to_string())
    }

    /// Query the dependency graph for a file or the entire repository.
    #[tool(
        description = "Explore file dependencies and import relationships. Use this to understand how files are connected: what a file imports (outgoing) and what other files depend on it (incoming). Best for: impact analysis before refactoring, understanding module boundaries, tracing data flow between files."
    )]
    pub async fn query_deps(&self, Parameters(params): Parameters<QueryDepsParams>) -> String {
        let limit = params.limit.unwrap_or(50);
        let direction = params.direction.as_deref().unwrap_or("both");

        let store = self.store.lock().await;

        let file = params.file.as_deref().unwrap_or("");

        let outgoing = if direction == "outgoing" || direction == "both" {
            match store.get_deps_outgoing(file) {
                Ok(deps) => deps
                    .into_iter()
                    .take(limit)
                    .map(|d| {
                        serde_json::json!({
                            "source_file": d.source_file.to_string_lossy(),
                            "target_path": d.target_path,
                            "kind": format!("{:?}", d.kind).to_lowercase(),
                            "line_number": d.line_number,
                        })
                    })
                    .collect::<Vec<_>>(),
                Err(_) => vec![],
            }
        } else {
            vec![]
        };

        let incoming = if direction == "incoming" || direction == "both" {
            match store.get_deps_incoming(file) {
                Ok(deps) => deps
                    .into_iter()
                    .take(limit)
                    .map(|d| {
                        serde_json::json!({
                            "source_file": d.source_file.to_string_lossy(),
                            "target_path": d.target_path,
                            "kind": format!("{:?}", d.kind).to_lowercase(),
                            "line_number": d.line_number,
                        })
                    })
                    .collect::<Vec<_>>(),
                Err(_) => vec![],
            }
        } else {
            vec![]
        };

        let result = serde_json::json!({
            "outgoing": outgoing,
            "incoming": incoming,
        });
        serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
    }

    /// Return current index health and statistics.
    #[tool(
        description = "Check the index status: total files, symbols, languages detected, embedding coverage percentage, and active embedding provider. Use this to verify the index is healthy before searching, or to understand the scope of the indexed codebase."
    )]
    pub async fn index_status(&self) -> String {
        let store = self.store.lock().await;
        let prov_guard = self.provider.read().await;
        let active_provider = prov_guard.as_ref().map(|p| p.provider_name().to_string());

        match store.get_status() {
            Ok(mut status) => {
                if active_provider.is_some() {
                    status.active_provider = active_provider;
                }
                let by_lang: serde_json::Map<String, serde_json::Value> = status
                    .by_language
                    .iter()
                    .map(|(lang, count)| (lang.as_str().to_string(), serde_json::json!(count)))
                    .collect();

                let result = serde_json::json!({
                    "total_symbols": status.total_symbols,
                    "total_files": status.total_files,
                    "by_language": by_lang,
                    "embedding_progress_percent": (status.embedding_progress * 100.0).round() as u32,
                    "active_provider": status.active_provider,
                    "last_indexed_commit": status.last_indexed_commit,
                });
                serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
            }
            Err(e) => format!("{{\"error\": \"{}\"}}", e),
        }
    }

    /// Trigger a re-index of the repository.
    #[tool(
        description = "Re-index the repository to pick up new or changed files. Use incremental (default) after small changes, or full=true after major refactors, branch switches, or when search results seem stale."
    )]
    pub async fn reindex(&self, Parameters(params): Parameters<ReindexParams>) -> String {
        let full = params.full.unwrap_or(false);

        // IndexPipeline is !Sync because SqliteStore wraps Connection (which uses RefCell).
        // We run the async pipeline on a dedicated OS thread with its own single-threaded
        // Tokio runtime so the future stays local and never needs to be Send.
        let repo_path = self.repo_path.clone();
        let db_dir = self.db_dir.clone();
        let config = self.config.clone();

        let (tx, rx) = tokio::sync::oneshot::channel();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(format!("runtime error: {e}")));
                    return;
                }
            };

            rt.block_on(async move {
                let pipeline = match IndexPipeline::new(&repo_path, &db_dir, &config) {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                };

                let result = if full {
                    pipeline.run_full_index().await
                } else {
                    pipeline.run_incremental_index().await
                };

                match result {
                    Ok(stats) => {
                        let _ = tx.send(Ok(stats));
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                    }
                }
            });
        });

        match rx.await {
            Ok(Ok(stats)) => {
                let output = serde_json::json!({
                    "files_indexed": stats.files_indexed,
                    "symbols_found": stats.symbols_found,
                    "chunks_created": stats.chunks_created,
                });
                serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string())
            }
            Ok(Err(e)) => format!("{{\"error\": \"{e}\"}}"),
            Err(_) => "{\"error\": \"reindex thread disconnected\"}".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// ServerHandler with MCP instructions for the AI model
// ---------------------------------------------------------------------------

#[tool_handler(
    router = Self::tool_router(),
    name = "code-indexer",
    version = "0.1.0",
    instructions = "code-indexer provides semantic and keyword code search over an indexed repository. Use it to understand codebases, find implementations, and trace dependencies.

Workflow:
1. Call `index_status` first to verify the index is healthy and see what's indexed.
2. Use `search_code` for natural language questions about the codebase (\"how does auth work\", \"where are API routes defined\").
3. Use `find_symbol` when you know a specific function/class/type name or want to browse the API surface.
4. Use `query_deps` to understand file relationships before refactoring or to trace data flow.
5. Call `reindex` after making changes if search results seem stale.

Tips:
- `search_code` uses hybrid semantic + keyword matching — describe what you're looking for in plain English for best results.
- `find_symbol` is exact name search — use partial names or prefixes to discover related symbols.
- Combine `file_path_contains` filter with queries to scope results to specific modules or directories.
- Check `index_status` embedding_progress_percent — 100% means semantic search is fully available."
)]
impl rmcp::ServerHandler for CodeIndexerServer {}
