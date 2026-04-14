use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG_TOML: &str = r#"# code-indexer configuration
# Full reference: https://github.com/your-org/code-indexer-rs

[embedding]
# "auto"  - detect jina-grep > ollama > openai-compat in order
# "jina"  - jina-grep local server (Apple Silicon MLX, recommended)
# "ollama" - local Ollama
# "none"  - BM25-only, no semantic search
provider = "auto"
enabled = true

# ── jina-grep (Apple Silicon MLX) ────────────────────────────────────────────
# Install: https://github.com/jina-ai/jina-grep
# Run:     jina-grep serve
[embedding.jina_grep]
url = "http://localhost:8089"
# Recommended for code: jina-code-embeddings-0.5b (fast) or 1.5b (higher quality)
# Recommended for prose/docs: jina-embeddings-v5-small or v5-nano
model = "jina-code-embeddings-0.5b"
# Matryoshka dimension reduction (64-896 for code models, 32-1024 for v5)
# Lower = faster search + less disk. 256 is a good default for code.
truncate_dim = 256
# task drives prompt selection: "code" for code repos, leave unset for general text
task = "code"

# ── Ollama (cross-platform) ───────────────────────────────────────────────────
# Install: https://ollama.ai  then: ollama pull nomic-embed-text
[embedding.ollama]
url = "http://localhost:11434"
model = "nomic-embed-text"

# ── Indexer ───────────────────────────────────────────────────────────────────
[indexer]
max_file_size = 1_048_576  # 1 MB

# ── Search ────────────────────────────────────────────────────────────────────
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
    Index {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        full: bool,
        #[arg(long)]
        embed: bool,
    },
    Init {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Overwrite existing .code-indexer.toml if present
        #[arg(long)]
        force: bool,
    },
    Status {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    Search {
        query: String,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "symbol")]
        kind: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

async fn run_embed(db_dir: &std::path::Path, config: &Config) -> Result<()> {
    let provider = match detect_provider(config).await {
        Some(p) => p,
        None => {
            println!("Embedding: no provider available, skipping");
            return Ok(());
        }
    };

    println!("Embedding: provider={} dims={}", provider.provider_name(), provider.dimensions());

    let store = SqliteStore::open(db_dir)?;
    let vector_index = VectorIndex::open(db_dir, provider.dimensions())?;

    let batch_size = provider.max_batch_size();
    let mut total_embedded = 0usize;

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

        let embeddings = provider.embed_batch(&texts).await?;

        for ((chunk_id, _chunk), embedding) in batch.iter().zip(embeddings.iter()) {
            store.store_chunk_vector(*chunk_id, embedding)?;
            vector_index.add(*chunk_id as u64, embedding)?;
        }

        total_embedded += batch.len();
        println!("  embedded {} chunks so far...", total_embedded);
    }

    vector_index.save()?;
    println!("Embedding complete: {} chunks embedded", total_embedded);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
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

            if transport == "stdio" {
                use rmcp::{transport::stdio, ServiceExt};
                let service = server.serve(stdio()).await?;
                service.waiting().await?;
            } else {
                tracing::warn!(
                    transport = %transport,
                    "HTTP transport not yet implemented, falling back to stdio"
                );
                use rmcp::{transport::stdio, ServiceExt};
                let service = server.serve(stdio()).await?;
                service.waiting().await?;
            }
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

        Commands::Init { repo, force } => {
            std::fs::create_dir_all(&repo)?;
            let config_path = repo.join(".code-indexer.toml");
            if config_path.exists() && !force {
                println!(
                    ".code-indexer.toml already exists. Use --force to overwrite."
                );
            } else {
                std::fs::write(&config_path, DEFAULT_CONFIG_TOML)?;
                println!("Created {}", config_path.display());
                println!();
                println!("Next steps:");
                println!("  1. Edit .code-indexer.toml to choose your embedding provider");
                println!("  2. Run: code-indexer index --embed");
                println!("  3. Add to Claude Code / Cursor MCP config to start searching");
                println!();
                println!("Add to .gitignore:");
                println!("  .code-indexer/");
            }
        }

        Commands::Status { repo } => {
            let config = Config::load(&repo)?;
            let db_dir = repo.join(&config.storage.db_dir);
            let store = SqliteStore::open(&db_dir)?;
            let status = store.get_status()?;

            println!("Index status:");
            println!("  files         : {}", status.total_files);
            println!("  symbols       : {}", status.total_symbols);
            println!(
                "  embedding     : {:.1}%",
                status.embedding_progress * 100.0
            );
            println!(
                "  provider      : {}",
                status.active_provider.as_deref().unwrap_or("none")
            );
            println!(
                "  last commit   : {}",
                status.last_indexed_commit.as_deref().unwrap_or("none")
            );
            if !status.by_language.is_empty() {
                println!("  languages:");
                for (lang, count) in &status.by_language {
                    println!("    {:?}: {}", lang, count);
                }
            }
        }

        Commands::Search {
            query,
            repo,
            kind,
            limit,
        } => {
            let config = Config::load(&repo)?;
            let db_dir = repo.join(&config.storage.db_dir);
            let store = SqliteStore::open(&db_dir)?;

            match kind.as_str() {
                "deps" => {
                    let outgoing = store.get_deps_outgoing(&query)?;
                    let incoming = store.get_deps_incoming(&query)?;
                    println!("Outgoing dependencies for '{}':", query);
                    for dep in outgoing.iter().take(limit) {
                        println!(
                            "  {} -> {} ({})",
                            dep.source_file.display(),
                            dep.target_path,
                            format!("{:?}", dep.kind).to_lowercase()
                        );
                    }
                    println!("Incoming dependencies for '{}':", query);
                    for dep in incoming.iter().take(limit) {
                        println!(
                            "  {} -> {} ({})",
                            dep.source_file.display(),
                            dep.target_path,
                            format!("{:?}", dep.kind).to_lowercase()
                        );
                    }
                }
                _ => {
                    // Default: symbol search
                    let results = store.search_symbols(&query, limit)?;
                    if results.is_empty() {
                        println!("No symbols found for '{}'", query);
                    } else {
                        println!("Symbols matching '{}':", query);
                        for r in &results {
                            println!(
                                "  {} ({:?}) - {}:{}",
                                r.name,
                                r.kind,
                                r.file_path.display(),
                                r.line_start
                            );
                            if let Some(sig) = &r.signature {
                                println!("    sig: {}", sig);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
