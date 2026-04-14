use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use code_indexer::config::Config;
use code_indexer::indexer::pipeline::IndexPipeline;
use code_indexer::server::CodeIndexerServer;
use code_indexer::storage::sqlite::SqliteStore;

#[derive(Parser)]
#[command(name = "code-indexer", version, about = "AI code indexer with hybrid search")]
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
                use rmcp::{ServiceExt, transport::stdio};
                let service = server.serve(stdio()).await?;
                service.waiting().await?;
            } else {
                tracing::warn!(
                    transport = %transport,
                    "HTTP transport not yet implemented, falling back to stdio"
                );
                use rmcp::{ServiceExt, transport::stdio};
                let service = server.serve(stdio()).await?;
                service.waiting().await?;
            }
        }

        Commands::Index { repo, full, embed: _ } => {
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
