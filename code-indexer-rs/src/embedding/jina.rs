use crate::config::JinaGrepConfig;
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct JinaProvider {
    client: Client,
    url: String,
    model: String,
    truncate_dim: Option<usize>,
    batch_size: usize,
}

#[derive(Serialize)]
struct JinaRequest {
    model: String,
    input: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncate_dim: Option<usize>,
}

#[derive(Deserialize)]
struct JinaResponse {
    data: Vec<JinaEmbedding>,
}

#[derive(Deserialize)]
struct JinaEmbedding {
    embedding: Vec<f32>,
}

impl JinaProvider {
    pub fn new(config: &JinaGrepConfig) -> Self {
        Self {
            client: Client::new(),
            url: config.url.clone(),
            model: config.model.clone(),
            truncate_dim: config.truncate_dim.map(|d| d as usize),
            batch_size: config.batch_size,
        }
    }
}

#[async_trait::async_trait]
impl super::EmbeddingProvider for JinaProvider {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let request = JinaRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
            prompt_name: Some("document".to_string()),
            truncate_dim: self.truncate_dim,
        };

        let response = self.client
            .post(format!("{}/v1/embeddings", self.url))
            .json(&request)
            .send()
            .await?
            .json::<JinaResponse>()
            .await?;

        Ok(response.data.into_iter().map(|e| e.embedding).collect())
    }

    async fn health_check(&self) -> bool {
        self.client
            .get(format!("{}/health", self.url))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    fn dimensions(&self) -> usize {
        self.truncate_dim.unwrap_or(896)
    }

    fn provider_name(&self) -> &str {
        "jina-grep"
    }

    fn max_batch_size(&self) -> usize {
        self.batch_size
    }
}
