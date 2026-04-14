use crate::storage::sqlite::SqliteStore;
use anyhow::Result;

pub struct EmbeddingCache<'a> {
    store: &'a SqliteStore,
    provider: String,
    dimensions: usize,
}

impl<'a> EmbeddingCache<'a> {
    pub fn new(store: &'a SqliteStore, provider: &str, dimensions: usize) -> Self {
        Self {
            store,
            provider: provider.to_string(),
            dimensions,
        }
    }

    pub fn get(&self, content_hash: &str) -> Result<Option<Vec<f32>>> {
        self.store
            .get_cached_embedding(content_hash, &self.provider, self.dimensions)
    }

    pub fn put(&self, content_hash: &str, vector: &[f32]) -> Result<()> {
        self.store
            .store_cached_embedding(content_hash, &self.provider, self.dimensions, vector)
    }
}
