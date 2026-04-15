use crate::storage::sqlite::SqliteStore;
use crate::types::*;
use anyhow::Result;

pub fn symbol_search(
    store: &SqliteStore,
    query: &str,
    _kind: Option<&str>,
    _language: Option<&str>,
    _file_path_contains: Option<&str>,
    limit: usize,
) -> Result<Vec<SymbolSearchResult>> {
    store.search_symbols(query, limit)
}

pub fn code_search(store: &SqliteStore, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    // Search both symbols and code chunks, merge results
    let symbol_results: Vec<SearchResult> = store.search_symbols(query, limit)?
        .into_iter()
        .map(|s| SearchResult {
            file_path: s.file_path,
            line_start: s.line_start,
            line_end: s.line_end,
            snippet: s.signature.unwrap_or_default(),
            symbol_name: Some(s.name),
            symbol_kind: Some(s.kind),
            score: s.score,
            language: s.language,
        })
        .collect();

    let chunk_results = store.search_chunks(query, limit).unwrap_or_default();

    // Merge: symbols first (higher relevance for exact name matches), then chunks
    let mut merged = symbol_results;
    merged.extend(chunk_results);

    // Deduplicate by file_path + line_start
    let mut seen = std::collections::HashSet::new();
    merged.retain(|r| seen.insert((r.file_path.clone(), r.line_start)));
    merged.truncate(limit);

    Ok(merged)
}
