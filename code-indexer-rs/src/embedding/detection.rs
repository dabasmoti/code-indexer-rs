use crate::config::Config;
use crate::embedding::{
    jina::JinaProvider, ollama::OllamaProvider, openai_compat::OpenAiCompatProvider,
    EmbeddingProvider,
};
use tracing::info;

pub async fn detect_provider(config: &Config) -> Option<Box<dyn EmbeddingProvider>> {
    if !config.embedding.enabled {
        info!("Embedding disabled by config");
        return None;
    }

    let provider_pref = config.embedding.provider.as_str();

    if provider_pref == "auto" || provider_pref == "jina" {
        let jina = JinaProvider::new(&config.embedding.jina_grep);
        if jina.health_check().await {
            info!(
                "Using jina-grep embedding provider at {}",
                config.embedding.jina_grep.url
            );
            return Some(Box::new(jina));
        }
    }

    if provider_pref == "auto" || provider_pref == "ollama" {
        let ollama = OllamaProvider::new(&config.embedding.ollama);
        if ollama.health_check().await {
            info!(
                "Using Ollama embedding provider at {}",
                config.embedding.ollama.url
            );
            return Some(Box::new(ollama));
        }
    }

    if provider_pref == "auto" || provider_pref == "openai" {
        let openai = OpenAiCompatProvider::new(&config.embedding.openai_compat);
        if openai.health_check().await {
            let url = config
                .embedding
                .openai_compat
                .url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1/embeddings");
            info!("Using OpenAI-compatible embedding provider at {}", url);
            return Some(Box::new(openai));
        }
    }

    info!("No embedding provider available - running in BM25-only mode");
    None
}
