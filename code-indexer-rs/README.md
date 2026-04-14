# code-indexer-rs

A zero-config MCP server that provides hybrid keyword + semantic code search for AI agents. Indexes any repository with tree-sitter parsing across 8 languages, BM25 full-text search, and optional vector semantic search via local embedding providers.

## Quick Start

### 1. Build

```bash
cargo build --release
```

The binary is at `target/release/code-indexer`.

### 2. Index a project

```bash
# From inside the project directory
code-indexer index

# Or point at a path
code-indexer index --repo /path/to/project

# Full re-index (ignores change detection)
code-indexer index --repo /path/to/project --full
```

### 3. Search

```bash
# Symbol search (functions, structs, classes, etc.)
code-indexer search "handleRequest" --repo /path/to/project

# Dependency search
code-indexer search "src/auth.rs" --repo /path/to/project --kind deps
```

### 4. Run as MCP server

```bash
# stdio transport (default) - for Claude Code, Cursor, etc.
code-indexer serve --repo /path/to/project

# Without embeddings (BM25-only, no external dependencies)
code-indexer serve --repo /path/to/project --no-embedding
```

### 5. Connect to Claude Code

Add to your Claude Code MCP config (`~/.claude.json` or project `.mcp.json`):

```json
{
  "mcpServers": {
    "code-indexer": {
      "command": "/path/to/code-indexer",
      "args": ["serve", "--repo", "/path/to/your/project"]
    }
  }
}
```

### 6. Connect to Cursor

Add to `.cursor/mcp.json` in your project:

```json
{
  "mcpServers": {
    "code-indexer": {
      "command": "/path/to/code-indexer",
      "args": ["serve", "--repo", "."]
    }
  }
}
```

## MCP Tools

Once connected, AI agents can use these tools:

| Tool | Description |
|------|-------------|
| `find_symbol` | BM25 keyword search over symbol definitions (functions, classes, types) |
| `search_code` | Hybrid semantic + keyword search over code chunks |
| `query_deps` | Query import/dependency graph for a file |
| `index_status` | Get index health, file counts, embedding progress |
| `reindex` | Trigger incremental or full re-indexing |

## Supported Languages

Rust, TypeScript/TSX, Python, Go, Java, C/C++, Ruby, Swift

## Embedding Providers

Semantic search requires an embedding provider. The server auto-detects in this order:

| Priority | Provider | How to run | Endpoint |
|----------|----------|-----------|----------|
| 1 | jina-grep | [jina-grep CLI](https://github.com/jina-ai/jina-grep) on Apple Silicon | `localhost:8089` |
| 2 | Ollama | `ollama pull nomic-embed-text && ollama serve` | `localhost:11434` |
| 3 | OpenAI-compatible | Any API (LiteLLM, vLLM, etc.) | Configured via env/config |

If no provider is available, the server runs in **BM25-only mode** -- symbol search works fully, semantic search is disabled.

## Configuration

### Zero-config

Just run it. Smart defaults handle:
- Language detection by file extension
- Excluded directories: `target/`, `node_modules/`, `.git/`, `dist/`, `build/`, `vendor/`, `__pycache__/`
- Max file size: 1 MB
- Git-based incremental indexing

### Optional config file

Create `.code-indexer.toml` in your project root:

```toml
[indexer]
max_file_size = 2_000_000
exclude_dirs = ["target", "node_modules", ".git", "dist", "build", "vendor"]
exclude_patterns = ["*.min.js", "*.generated.*"]

[embedding]
provider = "auto"  # "auto", "jina", "ollama", "openai", or disable with enabled = false

[embedding.ollama]
model = "nomic-embed-text"

[search]
default_limit = 20
max_limit = 100
```

### Environment variables

| Variable | Description |
|----------|-------------|
| `CODE_INDEXER_EMBEDDING_PROVIDER` | Override provider selection |
| `CODE_INDEXER_JINA_GREP_URL` | jina-grep endpoint (default: `http://localhost:8089`) |
| `CODE_INDEXER_OLLAMA_URL` | Ollama endpoint (default: `http://localhost:11434`) |
| `CODE_INDEXER_API_KEY` | API key for OpenAI-compatible provider |
| `CODE_INDEXER_EMBEDDING_ENABLED` | `true`/`false` to enable/disable embeddings |

### Config precedence

CLI flags > environment variables > project `.code-indexer.toml` > global `~/.config/code-indexer/config.toml` > built-in defaults

## Storage

Index data is stored in `.code-indexer/` inside the project directory:
- `index.sqlite` -- symbols, chunks, dependencies, metadata (SQLite with FTS5)
- `hnsw.index` -- vector index for semantic search (usearch HNSW)

Add `.code-indexer/` to your `.gitignore`.

## How It Works

1. **Parse** -- tree-sitter extracts symbols (functions, classes, types) and produces AST-aware code chunks
2. **Index** -- symbols go into SQLite FTS5 for keyword search; chunks are stored for embedding
3. **Embed** -- chunks are sent to the active embedding provider; vectors stored in HNSW index
4. **Search** -- queries hit both BM25 (keyword) and HNSW (vector), fused with Reciprocal Rank Fusion

## Development

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=info cargo run -- serve --repo .

# Check for issues
cargo clippy
```
