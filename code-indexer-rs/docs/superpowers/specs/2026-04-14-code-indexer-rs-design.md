# code-indexer-rs Design Spec

## Context

AI code agents (Claude Code, Cursor, custom MCP clients) need fast, semantic understanding of codebases to explore repositories effectively. Existing tools either lack semantic search (ripgrep), are tied to specific monorepos (mave-indexer), or don't persist indexes (jina-grep-cli). code-indexer-rs fills this gap: a zero-config, local-first MCP server that indexes any repository with hybrid keyword + semantic search, configurable embedding providers (jina-grep on Apple Silicon, Ollama cross-platform, any OpenAI-compatible API), and support for 8 programming languages via tree-sitter.

## Requirements

- **Generic MCP server** for any MCP-compatible AI agent (not tied to a specific repo or tool)
- **Zero-config**: point at any directory and it works with smart defaults
- **Configurable**: TOML config file + env vars + CLI flags, layered precedence
- **Local-first embedding**: jina-grep (Apple Silicon) > Ollama (cross-platform) > OpenAI-compat (fallback)
- **8 languages**: Rust, TypeScript, Python, Go, Java, C/C++, Ruby, Swift via tree-sitter
- **Hybrid search**: BM25 keyword + HNSW vector + RRF fusion (proven mave-indexer approach)
- **Single-file storage**: SQLite + HNSW index file in `.code-indexer/` directory
- **Incremental indexing**: git-based change detection + content hashing
- **Graceful degradation**: BM25-only mode when no embedding provider is available

## Architecture

### Module Layout

```
code-indexer-rs/src/
  main.rs                 CLI entry (clap) -- serve, index, status, search commands
  config.rs               Layered config: TOML + env + CLI flags (serde)
  server.rs               MCP server (rmcp) -- stdio + SSE transport

  indexer/
    mod.rs                Indexing orchestrator + background embedding coordinator
    pipeline.rs           File discovery, change detection, parse, chunk, embed pipeline
    git.rs                Git-based change detection (diff + content hashing)
    watcher.rs            File watcher with debounce (notify crate)

  parser/
    mod.rs                LanguageParser trait + dispatcher
    rust.rs               Rust tree-sitter: symbols, imports, chunks
    typescript.rs         TypeScript/TSX tree-sitter
    python.rs             Python tree-sitter
    go.rs                 Go tree-sitter
    java.rs               Java tree-sitter
    c_cpp.rs              C/C++ tree-sitter
    ruby.rs               Ruby tree-sitter
    swift.rs              Swift tree-sitter

  embedding/
    mod.rs                EmbeddingProvider trait + provider chain
    jina_grep.rs          jina-grep local (localhost:8089, code models)
    ollama.rs             Ollama (localhost:11434, /api/embed)
    openai_compat.rs      Any OpenAI-compatible API
    detection.rs          Auto-detect available providers at startup

  storage/
    mod.rs                Storage coordinator
    sqlite.rs             SQLite schema, FTS5 for symbols, file tracking
    vector.rs             HNSW index (usearch) + SQLite BLOB backing
    cache.rs              Embedding content-hash cache

  search/
    mod.rs                Search coordinator
    bm25.rs               BM25 symbol search via FTS5
    semantic.rs           Vector KNN via HNSW
    hybrid.rs             RRF fusion of BM25 + vector results
    router.rs             Query classification heuristic

  deps/
    mod.rs                Dependency graph storage + PageRank

  symbols.rs              Symbol types, kinds, metadata
  chunker.rs              AST-aware chunking (generalized across languages)
```

### Key Traits

```rust
trait LanguageParser: Send + Sync {
    fn language(&self) -> Language;
    fn extensions(&self) -> &[&str];
    fn parse_symbols(&self, source: &[u8], file_path: &Path) -> Vec<Symbol>;
    fn parse_imports(&self, source: &[u8], file_path: &Path) -> Vec<Import>;
    fn chunk_file(&self, source: &[u8], file_path: &Path) -> Vec<Chunk>;
}

trait EmbeddingProvider: Send + Sync {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    async fn health_check(&self) -> bool;
    fn dimensions(&self) -> usize;
    fn provider_name(&self) -> &str;
    fn max_batch_size(&self) -> usize;
}
```

### Data Flow

**Indexing pipeline:**
1. File discovery: walk directory, apply exclude filters, detect language by extension
2. Change detection: compare git HEAD with last indexed commit + content hashing (non-git directories: full-scan with content hashing against previous index)
3. Parse: tree-sitter extracts symbols (name, kind, signature, docs, line range) and produces AST-aware chunks (~400 tokens each, respecting function/class boundaries)
4. Store: symbols into SQLite with FTS5; chunks into SQLite
5. Embed (background): chunks sent in batches to active provider; vectors stored as BLOBs; HNSW index updated incrementally
6. Dependencies: extract imports/impl relationships per language; compute PageRank on full index

**Search pipeline:**
1. Query router classifies input (symbol-lookup vs semantic vs dependency)
2. BM25 search via FTS5 (CamelCase splitting, prefix matching)
3. Vector KNN via HNSW (query embedded with active provider)
4. RRF fusion: `score = sum(1/(60 + rank))` across both result lists
5. Symbol-lookup queries get 1.5x BM25 boost

## Embedding Provider Chain

### Auto-Detection Order
1. Probe `http://localhost:8089/health` -- jina-grep
2. Probe `http://localhost:11434/api/tags` -- Ollama
3. Check config/env for OpenAI-compatible endpoint
4. First available wins; log choice

### Provider Details

| Provider | Default Model | Dimensions | Code-Aware Tasks | Batch Size |
|----------|--------------|-----------|-----------------|-----------|
| jina-grep | `jina-code-embeddings-0.5b` | 896 (256 with Matryoshka) | nl2code, code2code, code2nl | 256 |
| Ollama | `nomic-embed-text` | varies by model | generic | 64 |
| OpenAI-compat | `text-embedding-3-small` | 1536 | generic | 64 |

### jina-grep Specific
- Uses `prompt_name: "document"` for indexing, `prompt_name: "query"` for search
- Supports Matryoshka dimension reduction (`truncate_dim: 256`) for 70% storage savings
- Code-specific task types for better relevance

### Graceful Degradation
- Provider down mid-index: pause embedding, continue BM25 indexing, retry on timer
- No provider at startup: BM25-only mode (symbol search fully functional)
- Provider change with dimension mismatch: detect automatically, warn user, re-embed required

## Storage

### SQLite Schema

Single database at `.code-indexer/index.sqlite`, WAL mode, NORMAL sync.

| Table | Columns | Purpose |
|-------|---------|---------|
| `symbols` | name, kind, language, file_path, line_start, line_end, signature, doc_comment, visibility, parent | Symbol definitions |
| `symbols_fts` | FTS5 virtual table (Porter stemmer, Unicode61) | BM25 search |
| `indexed_files` | file_path, content_hash, language, last_indexed, symbol_count | Change tracking |
| `chunks` | file_path, content, preamble, content_hash, line_start, line_end, language | Code chunks for embedding |
| `chunk_vectors` | chunk_id, vector BLOB (f32 LE), provider, dimensions | Stored embeddings |
| `embedding_cache` | content_hash, vector BLOB, provider, dimensions | Avoid re-embedding identical content |
| `file_deps` | source_file, target_path, kind, line_number | Dependency edges |
| `pagerank` | file_path, score | File importance |
| `index_meta` | key, value | Last commit, repo root, provider, timestamps |

### HNSW Vector Index
- Persisted to `.code-indexer/hnsw.index` via usearch
- In-memory graph loaded on startup from persisted file
- Incremental updates when chunks change
- Search: <5ms for top-20 at 100K+ vectors

## Configuration

### Config File (`.code-indexer.toml`)

```toml
[indexer]
languages = ["rust", "typescript", "python", "go", "java", "c", "cpp", "ruby", "swift"]
max_file_size = 1_048_576
exclude_dirs = ["target", "node_modules", ".git", "dist", "build", "vendor", "__pycache__"]
exclude_patterns = ["*.min.js", "*.generated.*", "*.pb.go"]
watch = true
watch_debounce_ms = 500

[embedding]
provider = "auto"
enabled = true

[embedding.jina_grep]
url = "http://localhost:8089"
model = "jina-code-embeddings-0.5b"
task = "nl2code"
truncate_dim = 256
batch_size = 256

[embedding.ollama]
url = "http://localhost:11434"
model = "nomic-embed-text"
batch_size = 64

[embedding.openai_compat]
url = "http://localhost:4000/v1/embeddings"
model = "text-embedding-3-small"
api_key_env = "CODE_INDEXER_API_KEY"
batch_size = 64

[search]
default_limit = 20
max_limit = 100
rrf_k = 60
bm25_symbol_boost = 1.5

[storage]
db_dir = ".code-indexer"
```

### Precedence
CLI flags > env vars (`CODE_INDEXER_*`) > project config (`.code-indexer.toml`) > global config (`~/.config/code-indexer/config.toml`) > built-in defaults.

### Environment Variables
- `CODE_INDEXER_EMBEDDING_PROVIDER` -- override provider selection
- `CODE_INDEXER_JINA_GREP_URL` -- jina-grep endpoint
- `CODE_INDEXER_OLLAMA_URL` -- Ollama endpoint
- `CODE_INDEXER_API_KEY` -- API key for OpenAI-compat provider
- `CODE_INDEXER_EMBEDDING_ENABLED` -- enable/disable embeddings

## MCP Interface

### Tools

**`find_symbol`** -- BM25 search over symbol definitions
- Params: `query` (required), `kind?`, `language?`, `file_path_contains?`, `limit?` (default 20, max 100)
- Returns: symbol name, kind, file path, line range, signature, doc comment, visibility

**`search_code`** -- Hybrid semantic + keyword search
- Params: `query` (required), `language?`, `file_path_contains?`, `limit?` (default 10, max 50)
- Returns: file path, line range, code snippet, symbol name, similarity score, RRF score

**`query_deps`** -- Dependency graph queries
- Params: `file?`, `module_name?`, `direction?` (incoming/outgoing/both), `kind?`, `limit?` (default 50, max 200)
- Returns: outgoing/incoming dependency edges, PageRank importance score

**`index_status`** -- Index health and progress
- Returns: total symbols, files indexed, by-language breakdown, embedding progress %, active provider, staleness

**`reindex`** -- Trigger incremental or full re-index
- Params: `full?` (boolean, default false)

### Transport
- **stdio** (primary): for Claude Code, Cursor, and other local MCP clients
- **HTTP + SSE** (optional): for remote agents, enabled via `--transport http --port 3000`

### CLI Commands
```bash
code-indexer serve [--repo PATH] [--transport stdio|http] [--port PORT] [--no-embedding] [--no-watch]
code-indexer index [--repo PATH] [--full] [--embed]
code-indexer status [--repo PATH]
code-indexer search <QUERY> [--repo PATH] [--kind symbol|code|deps]
```

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `clap` | 4.x | CLI parsing with derive |
| `rmcp` | latest | MCP server (stdio + SSE) |
| `rusqlite` | 0.31+ | SQLite with bundled + FTS5 |
| `tree-sitter` | 0.24+ | AST parsing framework |
| `tree-sitter-rust` | latest | Rust grammar |
| `tree-sitter-typescript` | latest | TypeScript/TSX grammar |
| `tree-sitter-python` | latest | Python grammar |
| `tree-sitter-go` | latest | Go grammar |
| `tree-sitter-java` | latest | Java grammar |
| `tree-sitter-c` | latest | C grammar |
| `tree-sitter-cpp` | latest | C++ grammar |
| `tree-sitter-ruby` | latest | Ruby grammar |
| `tree-sitter-swift` | latest | Swift grammar |
| `usearch` | latest | HNSW approximate nearest neighbor |
| `reqwest` | 0.12+ | HTTP client for providers |
| `tokio` | 1.x | Async runtime |
| `notify` | 6.x | File system watcher |
| `serde` + `toml` | latest | Config deserialization |
| `git2` | 0.19+ | Git change detection |
| `tracing` | 0.1 | Structured logging |

## Verification Plan

1. **Unit tests**: each parser extracts correct symbols from fixture files; each provider correctly formats API requests; config layering precedence works correctly
2. **Integration tests**: index a small test repo, verify symbol search returns correct results; verify semantic search finds relevant code; verify dependency graph is accurate
3. **Manual verification**: run `code-indexer serve` against a real repo, use Claude Code with MCP to call all 5 tools, verify results are relevant and fast
4. **Performance**: benchmark indexing speed (files/sec), search latency (p50/p99), and memory usage on repos of varying sizes (100, 1K, 10K files)
5. **Provider fallback**: test with jina-grep running (verify it's selected), stop jina-grep (verify fallback to Ollama), test with no providers (verify BM25-only mode)
