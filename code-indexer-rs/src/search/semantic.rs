use crate::embedding::EmbeddingProvider;
use crate::storage::sqlite::SqliteStore;
use crate::storage::vector::VectorIndex;
use crate::types::*;
use anyhow::Result;

pub async fn vector_search(
    vector_index: &VectorIndex,
    store: &SqliteStore,
    provider: &dyn EmbeddingProvider,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let query_vectors = provider.embed_batch(&[query.to_string()]).await?;
    let query_vec = match query_vectors.into_iter().next() {
        Some(v) => v,
        None => return Ok(vec![]),
    };

    let results = vector_index.search(&query_vec, limit)?;

    let mut search_results = Vec::new();
    for (chunk_id, distance) in results {
        if let Some(chunk) = store.get_chunk_by_id(chunk_id as i64)? {
            search_results.push(SearchResult {
                file_path: chunk.file_path,
                line_start: chunk.line_start,
                line_end: chunk.line_end,
                snippet: chunk.content,
                symbol_name: None,
                symbol_kind: None,
                score: (1.0 - distance) as f64,
                language: chunk.language,
            });
        }
    }

    Ok(search_results)
}
