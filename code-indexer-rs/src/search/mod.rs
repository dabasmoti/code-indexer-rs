pub mod bm25;
pub mod hybrid;
pub mod semantic;

use crate::config::SearchConfig;
use crate::embedding::EmbeddingProvider;
use crate::storage::sqlite::SqliteStore;
use crate::storage::vector::VectorIndex;
use crate::types::*;
use anyhow::Result;

pub struct SearchCoordinator<'a> {
    store: &'a SqliteStore,
    vector_index: Option<&'a VectorIndex>,
    provider: Option<&'a dyn EmbeddingProvider>,
    config: SearchConfig,
}

impl<'a> SearchCoordinator<'a> {
    pub fn new(
        store: &'a SqliteStore,
        vector_index: Option<&'a VectorIndex>,
        provider: Option<&'a dyn EmbeddingProvider>,
        config: SearchConfig,
    ) -> Self {
        Self {
            store,
            vector_index,
            provider,
            config,
        }
    }

    pub fn find_symbol(
        &self,
        query: &str,
        kind: Option<&str>,
        language: Option<&str>,
        file_path_contains: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SymbolSearchResult>> {
        let limit = limit.min(self.config.max_limit);
        bm25::symbol_search(self.store, query, kind, language, file_path_contains, limit)
    }

    pub async fn search_code(
        &self,
        query: &str,
        language: Option<&str>,
        file_path_contains: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let _ = language;
        let _ = file_path_contains;
        let limit = limit.min(self.config.max_limit);

        let bm25_results = bm25::code_search(self.store, query, limit * 2)?;

        let vector_results = if let (Some(vi), Some(prov)) = (self.vector_index, self.provider) {
            semantic::vector_search(vi, self.store, prov, query, limit * 2).await?
        } else {
            vec![]
        };

        if vector_results.is_empty() {
            Ok(bm25_results.into_iter().take(limit).collect())
        } else {
            let fused = hybrid::fuse_search_results(
                &bm25_results,
                &vector_results,
                self.config.rrf_k,
                self.config.bm25_symbol_boost,
                limit,
            );
            Ok(fused)
        }
    }
}
