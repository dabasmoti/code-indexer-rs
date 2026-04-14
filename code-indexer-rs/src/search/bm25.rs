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
    let symbol_results = store.search_symbols(query, limit)?;
    Ok(symbol_results
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
        .collect())
}
