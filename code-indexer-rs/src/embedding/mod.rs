pub mod detection;
pub mod jina;
pub mod ollama;
pub mod openai_compat;

use anyhow::Result;

#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed documents (code chunks) for indexing.
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    /// Embed a search query. Uses query-optimized prompt for providers that support it.
    /// Default falls back to embed_batch.
    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.embed_batch(&[text.to_string()]).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("empty embedding response"))
    }
    async fn health_check(&self) -> bool;
    fn dimensions(&self) -> usize;
    fn provider_name(&self) -> &str;
    fn max_batch_size(&self) -> usize;
}

pub use detection::detect_provider;
