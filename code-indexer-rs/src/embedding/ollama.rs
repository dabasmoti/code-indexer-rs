use crate::config::OllamaConfig;
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const NOMIC_EMBED_TEXT_DIMENSIONS: usize = 768;

pub struct OllamaProvider {
    client: Client,
    url: String,
    model: String,
    batch_size: usize,
    dimensions: usize,
}

#[derive(Serialize)]
struct OllamaEmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl OllamaProvider {
    pub fn new(config: &OllamaConfig) -> Self {
        Self {
            client: Client::new(),
            url: config.url.clone(),
            model: config.model.clone(),
            batch_size: config.batch_size,
            dimensions: NOMIC_EMBED_TEXT_DIMENSIONS,
        }
    }
}

#[async_trait::async_trait]
impl super::EmbeddingProvider for OllamaProvider {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let request = OllamaEmbedRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
        };

        let response = self.client
            .post(format!("{}/api/embed", self.url))
            .json(&request)
            .send()
            .await?
            .json::<OllamaEmbedResponse>()
            .await?;

        Ok(response.embeddings)
    }

    async fn health_check(&self) -> bool {
        self.client
            .get(format!("{}/api/tags", self.url))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }

    fn max_batch_size(&self) -> usize {
        self.batch_size
    }
}
