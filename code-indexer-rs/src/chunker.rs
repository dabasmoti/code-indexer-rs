use crate::types::{Chunk, Language};
use sha2::{Digest, Sha256};
use std::path::Path;

const TARGET_CHUNK_TOKENS: usize = 400;
const CHARS_PER_TOKEN_ESTIMATE: usize = 4;
const TARGET_CHUNK_CHARS: usize = TARGET_CHUNK_TOKENS * CHARS_PER_TOKEN_ESTIMATE;

pub fn chunk_by_lines(
    source: &str,
    file_path: &Path,
    language: Language,
    boundaries: &[usize],
) -> Vec<Chunk> {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return vec![];
    }

    let mut sorted_boundaries = boundaries.to_vec();
    sorted_boundaries.sort();
    sorted_boundaries.dedup();

    if sorted_boundaries.is_empty() || sorted_boundaries[0] != 0 {
        sorted_boundaries.insert(0, 0);
    }

    let mut chunks = Vec::new();
    let mut i = 0;

    while i < sorted_boundaries.len() {
        let start_line = sorted_boundaries[i];
        if start_line >= lines.len() {
            break;
        }

        // Accumulate lines until we hit target size or next major boundary
        let mut end_line = if i + 1 < sorted_boundaries.len() {
            sorted_boundaries[i + 1].min(lines.len())
        } else {
            lines.len()
        };

        let content: String = lines[start_line..end_line].join("\n");
        if content.len() < TARGET_CHUNK_CHARS && i + 1 < sorted_boundaries.len() {
            // Chunk is small — try merging with next section
            let next_end = if i + 2 < sorted_boundaries.len() {
                sorted_boundaries[i + 2].min(lines.len())
            } else {
                lines.len()
            };
            let merged: String = lines[start_line..next_end].join("\n");
            if merged.len() <= TARGET_CHUNK_CHARS * 2 {
                end_line = next_end;
                i += 1; // skip next boundary since we merged
            }
        }

        let content = lines[start_line..end_line].join("\n");
        let content_hash = hash_content(&content);

        // Preamble: file path + language context
        let preamble = format!(
            "{}:{}-{} [{}]",
            file_path.display(),
            start_line + 1,
            end_line,
            language
        );

        chunks.push(Chunk {
            file_path: file_path.to_path_buf(),
            content,
            preamble,
            content_hash,
            line_start: start_line + 1, // 1-indexed
            line_end: end_line,
            language,
        });

        i += 1;
    }

    chunks
}

pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}
