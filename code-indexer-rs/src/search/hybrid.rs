use crate::types::SearchResult;
use std::collections::HashMap;

pub fn rrf_fuse(
    bm25_ranked: &[(String, usize)],
    vector_ranked: &[(String, usize)],
    k: usize,
    bm25_boost: f64,
) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();

    for (id, rank) in bm25_ranked {
        *scores.entry(id.clone()).or_default() += bm25_boost / (k as f64 + *rank as f64);
    }
    for (id, rank) in vector_ranked {
        *scores.entry(id.clone()).or_default() += 1.0 / (k as f64 + *rank as f64);
    }

    let mut results: Vec<_> = scores.into_iter().collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results
}

pub fn fuse_search_results(
    bm25_results: &[SearchResult],
    vector_results: &[SearchResult],
    rrf_k: usize,
    bm25_boost: f64,
    limit: usize,
) -> Vec<SearchResult> {
    let make_id =
        |r: &SearchResult| -> String { format!("{}:{}:{}", r.file_path.display(), r.line_start, r.line_end) };

    let bm25_ranked: Vec<(String, usize)> = bm25_results
        .iter()
        .enumerate()
        .map(|(i, r)| (make_id(r), i + 1))
        .collect();

    let vector_ranked: Vec<(String, usize)> = vector_results
        .iter()
        .enumerate()
        .map(|(i, r)| (make_id(r), i + 1))
        .collect();

    let fused = rrf_fuse(&bm25_ranked, &vector_ranked, rrf_k, bm25_boost);

    let mut result_map: HashMap<String, SearchResult> = HashMap::new();
    for r in bm25_results.iter().chain(vector_results.iter()) {
        result_map.entry(make_id(r)).or_insert_with(|| r.clone());
    }

    fused
        .into_iter()
        .take(limit)
        .filter_map(|(id, score)| {
            result_map.remove(&id).map(|mut r| {
                r.score = score;
                r
            })
        })
        .collect()
}
