use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG_TOML: &str = r#"# code-indexer configuration
# Full reference: https://github.com/dabasmoti/code-indexer-rs

[embedding]
# "jina"   - jina-grep CLI on Apple Silicon (auto-starts/stops, no server needed)
# "ollama" - local Ollama
# "auto"   - try jina > ollama > BM25-only
provider = "jina"
enabled = true

[embedding.jina_grep]
url = "http://localhost:8099"
model = "jina-code-embeddings-0.5b"
truncate_dim = 256
task = "code"
batch_size = 64

[embedding.ollama]
url = "http://localhost:11434"
model = "nomic-embed-text"

[indexer]
max_file_size = 1_048_576

[search]
default_limit = 20
max_limit = 100
"#;

use code_indexer::config::Config;
use code_indexer::embedding::detection::detect_provider;
use code_indexer::indexer::pipeline::IndexPipeline;
use code_indexer::server::CodeIndexerServer;
use code_indexer::storage::sqlite::SqliteStore;
use code_indexer::storage::vector::VectorIndex;

#[derive(Parser)]
#[command(
    name = "code-indexer",
    version,
    about = "AI code indexer with hybrid search"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install the binary to ~/.cargo/bin via cargo install
    Install,
    /// Initialize a project: creates .code-indexer.toml and updates .mcp.json
    Init {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Overwrite existing .code-indexer.toml if present
        #[arg(long)]
        force: bool,
    },
    /// Index a repository (parse symbols and chunks)
    Index {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        full: bool,
        /// Also generate embeddings after indexing
        #[arg(long)]
        embed: bool,
    },
    /// Start the MCP server for Claude Code / Cursor
    Serve {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "stdio")]
        transport: String,
        #[arg(long, default_value_t = 3000)]
        port: u16,
        #[arg(long)]
        no_embedding: bool,
        #[arg(long)]
        no_watch: bool,
    },
    /// Show index status
    Status {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// Search the index
    Search {
        query: String,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// "hybrid" (BM25 + semantic), "symbol" (BM25 names), "deps" (dependency graph)
        #[arg(long, default_value = "hybrid")]
        kind: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

// ---------------------------------------------------------------------------
// Install
// ---------------------------------------------------------------------------

fn run_install() -> Result<()> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    println!("Installing code-indexer to ~/.cargo/bin ...");

    let status = std::process::Command::new("cargo")
        .args(["install", "--path", manifest_dir])
        .status()?;

    if !status.success() {
        anyhow::bail!("cargo install failed");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Init: create .code-indexer.toml + update .mcp.json
// ---------------------------------------------------------------------------

fn run_init(repo: &std::path::Path, force: bool) -> Result<()> {
    std::fs::create_dir_all(repo)?;
    let repo = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());

    // Write .code-indexer.toml
    let config_path = repo.join(".code-indexer.toml");
    if config_path.exists() && !force {
        println!(".code-indexer.toml already exists (use --force to overwrite)");
    } else {
        std::fs::write(&config_path, DEFAULT_CONFIG_TOML)?;
        println!("Created {}", config_path.display());
    }

    // Determine binary path for MCP config
    let binary_path = which_code_indexer();

    // Update .mcp.json
    update_mcp_json(&repo, &binary_path)?;

    println!();
    println!("Next steps:");
    println!("  code-indexer index --full --embed");
    println!();
    println!("Add to .gitignore:");
    println!("  .code-indexer/");

    Ok(())
}

/// Find the installed code-indexer binary path.
fn which_code_indexer() -> String {
    // Prefer ~/.cargo/bin (cargo install target), then current exe
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin").join("code-indexer");
        if cargo_bin.exists() {
            return cargo_bin.to_string_lossy().to_string();
        }
    }
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "code-indexer".to_string())
}

/// Add or update the code-indexer entry in .mcp.json, preserving existing servers.
fn update_mcp_json(repo: &std::path::Path, binary_path: &str) -> Result<()> {
    let mcp_path = repo.join(".mcp.json");

    // Read existing or start fresh
    let mut root: serde_json::Value = if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Ensure mcpServers key exists
    if root.get("mcpServers").is_none() {
        root["mcpServers"] = serde_json::json!({});
    }

    // Add/replace code-indexer entry
    root["mcpServers"]["code-indexer"] = serde_json::json!({
        "command": binary_path,
        "args": ["serve", "--repo", repo.to_string_lossy()]
    });

    let content = serde_json::to_string_pretty(&root)?;
    std::fs::write(&mcp_path, content)?;
    println!("Updated  {}", mcp_path.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// Embed pipeline (auto-starts jina-grep if needed)
// ---------------------------------------------------------------------------

async fn run_embed(db_dir: &std::path::Path, config: &Config) -> Result<()> {
    // Auto-start jina-grep if configured but not running
    let jina_started = if config.embedding.provider == "jina" || config.embedding.provider == "auto" {
        maybe_start_jina(&config.embedding.jina_grep.url).await
    } else {
        false
    };

    let provider = match detect_provider(config).await {
        Some(p) => p,
        None => {
            let hint = match config.embedding.provider.as_str() {
                "jina" => "  Install jina-grep: uv tool install jina-grep --from git+https://github.com/jina-ai/jina-grep-cli.git",
                "ollama" => "  Start Ollama: ollama serve",
                _ => "  Start your configured embedding provider",
            };
            println!("Embedding skipped: '{}' provider not available", config.embedding.provider);
            println!("{}", hint);
            return Ok(());
        }
    };

    println!("Embedding: provider={} dims={}", provider.provider_name(), provider.dimensions());

    let store = SqliteStore::open(db_dir)?;
    let vector_index = VectorIndex::open(db_dir, provider.dimensions())?;

    let batch_size = provider.max_batch_size();
    let total_chunks = store.count_chunks_without_embeddings()?;

    if total_chunks == 0 {
        println!("Embedding: all chunks already have vectors");
        if jina_started {
            stop_jina();
        }
        return Ok(());
    }

    println!("Embedding: {} chunks to process", total_chunks);
    let mut total_embedded = 0usize;

    let mut retries = 0u32;
    loop {
        let batch = store.get_chunks_without_embeddings(batch_size)?;
        if batch.is_empty() {
            break;
        }

        let texts: Vec<String> = batch
            .iter()
            .map(|(_, chunk)| {
                if chunk.preamble.is_empty() {
                    chunk.content.clone()
                } else {
                    format!("{}\n{}", chunk.preamble, chunk.content)
                }
            })
            .collect();

        let embeddings = match provider.embed_batch(&texts).await {
            Ok(e) => {
                retries = 0;
                e
            }
            Err(e) => {
                retries += 1;
                if retries > 3 {
                    anyhow::bail!("embedding failed after 3 retries: {e}");
                }
                eprintln!("\n  batch failed (attempt {retries}/3): {e}, retrying in 5s...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        store.conn.execute_batch("BEGIN")?;
        for ((chunk_id, _), embedding) in batch.iter().zip(embeddings.iter()) {
            store.store_chunk_vector(*chunk_id, embedding)?;
            vector_index.add(*chunk_id as u64, embedding)?;
        }
        store.conn.execute_batch("COMMIT")?;

        total_embedded += batch.len();
        eprintln!("  embedded {}/{} chunks", total_embedded, total_chunks);
    }

    vector_index.save()?;
    println!("\nEmbedding complete: {}/{} chunks embedded", total_embedded, total_chunks);

    // Stop jina-grep if we started it
    if jina_started {
        stop_jina();
    }

    Ok(())
}

/// Ensure jina-grep is running and the model is warm. Returns true if we started it.
async fn maybe_start_jina(url: &str) -> bool {
    let already_running = jina_health_check(url).await;

    if !already_running {
        let started = std::process::Command::new("jina-grep")
            .args(["serve", "start"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();

        if !started {
            return false;
        }

        eprint!("Starting jina-grep...");
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if jina_health_check(url).await {
                break;
            }
            eprint!(".");
        }
    }

    eprint!(" warming up model...");
    if jina_warmup(url).await {
        eprintln!(" ready");
        return !already_running;
    }

    eprintln!(" warmup failed");
    false
}

async fn jina_health_check(url: &str) -> bool {
    reqwest::Client::new()
        .get(format!("{}/health", url))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Sends a single-item embedding request to verify the model is loaded and serving.
async fn jina_warmup(url: &str) -> bool {
    let body = serde_json::json!({
        "model": "jina-code-embeddings-0.5b",
        "input": ["warmup"],
        "task": "code2code",
        "truncate_dim": 256
    });
    reqwest::Client::new()
        .post(format!("{}/v1/embeddings", url))
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

fn stop_jina() {
    let _ = std::process::Command::new("jina-grep")
        .args(["serve", "stop"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    println!("Stopped jina-grep server");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Install => {
            run_install()?;
        }

        Commands::Init { repo, force } => {
            run_init(&repo, force)?;
        }

        Commands::Index { repo, full, embed } => {
            let mut config = Config::load(&repo)?;
            config.indexer.watch = false;

            let db_dir = repo.join(&config.storage.db_dir);
            let pipeline = IndexPipeline::new(&repo, &db_dir, &config)?;

            let stats = if full {
                pipeline.run_full_index().await?
            } else {
                pipeline.run_incremental_index().await?
            };

            println!("Index complete:");
            println!("  files indexed : {}", stats.files_indexed);
            println!("  symbols found : {}", stats.symbols_found);
            println!("  chunks created: {}", stats.chunks_created);
            println!("  files skipped : {}", stats.files_skipped);

            if embed {
                run_embed(&db_dir, &config).await?;
            }
        }

        Commands::Serve {
            repo,
            transport,
            port: _port,
            no_embedding,
            no_watch,
        } => {
            let repo = repo.canonicalize()?;
            let mut config = Config::load(&repo)?;

            if no_embedding {
                config.embedding.enabled = false;
            }
            if no_watch {
                config.indexer.watch = false;
            }

            let db_dir = repo.join(&config.storage.db_dir);
            let pipeline = IndexPipeline::new(&repo, &db_dir, &config)?;
            let stats = pipeline.run_incremental_index().await?;
            tracing::info!(
                files = stats.files_indexed,
                symbols = stats.symbols_found,
                "initial incremental index complete"
            );

            let server = CodeIndexerServer::new(repo, config).await?;

            use rmcp::{transport::stdio, ServiceExt};
            if transport != "stdio" {
                tracing::warn!("HTTP transport not yet implemented, falling back to stdio");
            }
            let service = server.serve(stdio()).await?;
            service.waiting().await?;
        }

        Commands::Status { repo } => {
            let config = Config::load(&repo)?;
            let db_dir = repo.join(&config.storage.db_dir);
            let store = SqliteStore::open(&db_dir)?;
            let status = store.get_status()?;

            println!("Index status:");
            println!("  files         : {}", status.total_files);
            println!("  symbols       : {}", status.total_symbols);
            println!("  embedding     : {:.1}%", status.embedding_progress * 100.0);
            println!("  provider      : {}", status.active_provider.as_deref().unwrap_or("none"));
            println!("  last commit   : {}", status.last_indexed_commit.as_deref().unwrap_or("none"));
            if !status.by_language.is_empty() {
                println!("  languages:");
                for (lang, count) in &status.by_language {
                    println!("    {}: {}", lang.as_str(), count);
                }
            }
        }

        Commands::Search { query, repo, kind, limit } => {
            let repo = repo.canonicalize().unwrap_or(repo);
            let config = Config::load(&repo)?;
            let db_dir = repo.join(&config.storage.db_dir);
            let store = SqliteStore::open(&db_dir)?;

            match kind.as_str() {
                "hybrid" | "semantic" => {
                    use code_indexer::search::{bm25, hybrid};

                    // Auto-start jina if needed for query embedding
                    let jina_started = if config.embedding.provider == "jina" || config.embedding.provider == "auto" {
                        maybe_start_jina(&config.embedding.jina_grep.url).await
                    } else {
                        false
                    };

                    let provider = detect_provider(&config).await;

                    let bm25_results = bm25::code_search(&store, &query, limit * 2).unwrap_or_default();

                    let vector_results = if let Some(ref p) = provider {
                        if let Ok(vi) = VectorIndex::open(&db_dir, p.dimensions()) {
                            let query_vec = p.embed_query(&query).await.unwrap_or_default();
                            if !query_vec.is_empty() {
                                vi.search(&query_vec, limit * 2)
                                    .unwrap_or_default()
                                    .into_iter()
                                    .filter_map(|(chunk_id, distance)| {
                                        store.get_chunk_by_id(chunk_id as i64).ok().flatten()
                                            .map(|chunk| code_indexer::types::SearchResult {
                                                file_path: chunk.file_path,
                                                line_start: chunk.line_start,
                                                line_end: chunk.line_end,
                                                snippet: chunk.content,
                                                symbol_name: None,
                                                symbol_kind: None,
                                                score: (1.0 - distance) as f64,
                                                language: chunk.language,
                                            })
                                    })
                                    .collect()
                            } else { vec![] }
                        } else { vec![] }
                    } else { vec![] };

                    if jina_started {
                        stop_jina();
                    }

                    let results = hybrid::fuse_search_results(
                        &bm25_results,
                        &vector_results,
                        config.search.rrf_k,
                        config.search.bm25_symbol_boost,
                        limit,
                    );

                    if results.is_empty() {
                        println!("No results for '{}'", query);
                    } else {
                        let mode = if provider.is_some() { "hybrid" } else { "BM25-only" };
                        println!("Results for '{}' [{}]:\n", query, mode);
                        for r in &results {
                            println!("  {} (lines {}-{})", r.file_path.display(), r.line_start, r.line_end);
                            for line in r.snippet.lines().take(5) {
                                println!("    {}", line);
                            }
                            println!();
                        }
                    }
                }

                "deps" => {
                    let outgoing = store.get_deps_outgoing(&query)?;
                    let incoming = store.get_deps_incoming(&query)?;
                    println!("Outgoing from '{}':", query);
                    for dep in outgoing.iter().take(limit) {
                        println!("  {} -> {} ({})", dep.source_file.display(), dep.target_path, format!("{:?}", dep.kind).to_lowercase());
                    }
                    println!("Incoming to '{}':", query);
                    for dep in incoming.iter().take(limit) {
                        println!("  {} -> {} ({})", dep.source_file.display(), dep.target_path, format!("{:?}", dep.kind).to_lowercase());
                    }
                }

                _ => {
                    let results = store.search_symbols(&query, limit)?;
                    if results.is_empty() {
                        println!("No symbols found for '{}'", query);
                    } else {
                        println!("Symbols matching '{}':", query);
                        for r in &results {
                            println!("  {} ({}) - {}:{}", r.name, r.kind.as_str(), r.file_path.display(), r.line_start);
                            if let Some(sig) = &r.signature {
                                println!("    {}", sig);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
