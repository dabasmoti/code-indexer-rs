use crate::config::OpenAiCompatConfig;
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const OPENAI_EMBED_DIMENSIONS: usize = 1536;

pub struct OpenAiCompatProvider {
    client: Client,
    url: String,
    model: String,
    api_key: Option<String>,
    batch_size: usize,
}

#[derive(Serialize)]
struct OpenAiEmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct OpenAiEmbedResponse {
    data: Vec<OpenAiEmbedding>,
}

#[derive(Deserialize)]
struct OpenAiEmbedding {
    embedding: Vec<f32>,
}

impl OpenAiCompatProvider {
    pub fn new(config: &OpenAiCompatConfig) -> Self {
        let api_key = config.api_key_env
            .as_deref()
            .and_then(|env_var| std::env::var(env_var).ok());

        let url = config.url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1/embeddings".to_string());

        Self {
            client: Client::new(),
            url,
            model: config.model.clone(),
            api_key,
            batch_size: config.batch_size,
        }
    }
}

#[async_trait::async_trait]
impl super::EmbeddingProvider for OpenAiCompatProvider {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let request = OpenAiEmbedRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
        };

        let mut req = self.client.post(&self.url).json(&request);
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req
            .send()
            .await?
            .json::<OpenAiEmbedResponse>()
            .await?;

        Ok(response.data.into_iter().map(|e| e.embedding).collect())
    }

    async fn health_check(&self) -> bool {
        // OpenAI-compat endpoints don't have a standard health check.
        // Try a minimal embed request to verify connectivity and auth.
        let request = OpenAiEmbedRequest {
            model: self.model.clone(),
            input: vec!["health check".to_string()],
        };
        let mut req = self.client.post(&self.url).json(&request);
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }
        req.timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    fn dimensions(&self) -> usize {
        OPENAI_EMBED_DIMENSIONS
    }

    fn provider_name(&self) -> &str {
        "openai-compat"
    }

    fn max_batch_size(&self) -> usize {
        self.batch_size
    }
}
