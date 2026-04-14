pub mod jina;
pub mod ollama;
pub mod openai_compat;
pub mod detection;

use anyhow::Result;

#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    async fn health_check(&self) -> bool;
    fn dimensions(&self) -> usize;
    fn provider_name(&self) -> &str;
    fn max_batch_size(&self) -> usize;
}

pub use detection::detect_provider;
