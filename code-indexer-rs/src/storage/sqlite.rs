use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

use crate::types::{
    Chunk, DepEdge, ImportKind, IndexStatus, Language, Symbol, SymbolKind, SymbolSearchResult,
};

pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open(db_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(db_dir)
            .with_context(|| format!("Failed to create db directory: {}", db_dir.display()))?;

        let db_path = db_dir.join("index.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open SQLite database: {}", db_path.display()))?;

        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .context("Failed to set WAL mode")?;

        let store = SqliteStore { conn };
        store.create_tables()?;
        Ok(store)
    }

    fn create_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS symbols (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                name        TEXT NOT NULL,
                kind        TEXT NOT NULL,
                language    TEXT NOT NULL,
                file_path   TEXT NOT NULL,
                line_start  INTEGER NOT NULL,
                line_end    INTEGER NOT NULL,
                signature   TEXT,
                doc_comment TEXT,
                visibility  TEXT,
                parent      TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_file_path ON symbols(file_path);
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);

            CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
                name,
                signature,
                doc_comment,
                content='symbols',
                content_rowid='id',
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS symbols_ai
            AFTER INSERT ON symbols BEGIN
                INSERT INTO symbols_fts(rowid, name, signature, doc_comment)
                VALUES (new.id, new.name, new.signature, new.doc_comment);
            END;

            CREATE TRIGGER IF NOT EXISTS symbols_ad
            AFTER DELETE ON symbols BEGIN
                INSERT INTO symbols_fts(symbols_fts, rowid, name, signature, doc_comment)
                VALUES ('delete', old.id, old.name, old.signature, old.doc_comment);
            END;

            CREATE TABLE IF NOT EXISTS indexed_files (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path    TEXT NOT NULL UNIQUE,
                content_hash TEXT NOT NULL,
                language     TEXT NOT NULL,
                symbol_count INTEGER NOT NULL DEFAULT 0,
                indexed_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_indexed_files_path ON indexed_files(file_path);

            CREATE TABLE IF NOT EXISTS chunks (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path    TEXT NOT NULL,
                content      TEXT NOT NULL,
                preamble     TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                line_start   INTEGER NOT NULL,
                line_end     INTEGER NOT NULL,
                language     TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_file_path ON chunks(file_path);

            CREATE TABLE IF NOT EXISTS chunk_vectors (
                chunk_id  INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
                embedding BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS embedding_cache (
                content_hash TEXT NOT NULL,
                provider TEXT NOT NULL,
                dimensions INTEGER NOT NULL,
                vector BLOB NOT NULL,
                PRIMARY KEY (content_hash, provider, dimensions)
            );

            CREATE TABLE IF NOT EXISTS file_deps (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                source_file TEXT NOT NULL,
                target_path TEXT NOT NULL,
                kind        TEXT NOT NULL,
                line_number INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_file_deps_source ON file_deps(source_file);
            CREATE INDEX IF NOT EXISTS idx_file_deps_target ON file_deps(target_path);

            CREATE TABLE IF NOT EXISTS pagerank (
                file_path TEXT PRIMARY KEY,
                score     REAL NOT NULL DEFAULT 0.0
            );

            CREATE TABLE IF NOT EXISTS index_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )
        .context("Failed to create tables")
    }

    pub fn insert_symbols(&self, symbols: &[Symbol]) -> Result<()> {
        let mut stmt = self.conn.prepare(
            r#"INSERT INTO symbols
               (name, kind, language, file_path, line_start, line_end,
                signature, doc_comment, visibility, parent)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
        )?;

        for sym in symbols {
            stmt.execute(params![
                sym.name,
                sym.kind.as_str(),
                sym.language.as_str(),
                sym.file_path.to_string_lossy().as_ref(),
                sym.line_start as i64,
                sym.line_end as i64,
                sym.signature,
                sym.doc_comment,
                sym.visibility,
                sym.parent,
            ])?;
        }
        Ok(())
    }

    pub fn search_symbols(&self, query: &str, limit: usize) -> Result<Vec<SymbolSearchResult>> {
        let fts_query = format!("{}*", query);
        let like_pattern = format!("%{}%", query);

        // Union FTS5 full-text search with a LIKE name search to also capture camelCase
        // symbol names (e.g. "DataProcessor" matched by query "process"). UNION (without ALL)
        // deduplicates rows that appear in both result sets.
        let mut stmt = self.conn.prepare(
            r#"SELECT s.name, s.kind, s.language, s.file_path,
                      s.line_start, s.line_end, s.signature, s.doc_comment,
                      s.visibility, f.rank AS score
               FROM symbols_fts f
               JOIN symbols s ON s.id = f.rowid
               WHERE symbols_fts MATCH ?1
               UNION
               SELECT s.name, s.kind, s.language, s.file_path,
                      s.line_start, s.line_end, s.signature, s.doc_comment,
                      s.visibility, 0.0 AS score
               FROM symbols s
               WHERE lower(s.name) LIKE lower(?2)
                 AND s.id NOT IN (
                     SELECT f2.rowid FROM symbols_fts f2 WHERE symbols_fts MATCH ?1
                 )
               ORDER BY score
               LIMIT ?3"#,
        )?;

        let results = stmt
            .query_map(params![fts_query, like_pattern, limit as i64], |row| {
                let kind_str: String = row.get(1)?;
                let lang_str: String = row.get(2)?;
                let file_str: String = row.get(3)?;
                let rank: f64 = row.get(9)?;

                Ok(SymbolSearchResult {
                    name: row.get(0)?,
                    kind: parse_symbol_kind(&kind_str),
                    language: parse_language(&lang_str),
                    file_path: std::path::PathBuf::from(file_str),
                    line_start: row.get::<_, i64>(4)? as usize,
                    line_end: row.get::<_, i64>(5)? as usize,
                    signature: row.get(6)?,
                    doc_comment: row.get(7)?,
                    visibility: row.get(8)?,
                    score: -rank,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(results)
    }

    pub fn insert_chunks(&self, chunks: &[Chunk]) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            r#"INSERT INTO chunks (file_path, content, preamble, content_hash, line_start, line_end, language)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
        )?;

        let mut ids = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            stmt.execute(params![
                chunk.file_path.to_string_lossy().as_ref(),
                chunk.content,
                chunk.preamble,
                chunk.content_hash,
                chunk.line_start as i64,
                chunk.line_end as i64,
                chunk.language.as_str(),
            ])?;
            ids.push(self.conn.last_insert_rowid());
        }
        Ok(ids)
    }

    pub fn get_chunks_without_embeddings(&self, limit: usize) -> Result<Vec<(i64, Chunk)>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT c.id, c.file_path, c.content, c.preamble, c.content_hash,
                      c.line_start, c.line_end, c.language
               FROM chunks c
               LEFT JOIN chunk_vectors cv ON cv.chunk_id = c.id
               WHERE cv.chunk_id IS NULL
               LIMIT ?1"#,
        )?;

        let results = stmt
            .query_map(params![limit as i64], |row| {
                let file_str: String = row.get(1)?;
                let lang_str: String = row.get(7)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    Chunk {
                        file_path: std::path::PathBuf::from(file_str),
                        content: row.get(2)?,
                        preamble: row.get(3)?,
                        content_hash: row.get(4)?,
                        line_start: row.get::<_, i64>(5)? as usize,
                        line_end: row.get::<_, i64>(6)? as usize,
                        language: parse_language(&lang_str),
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(results)
    }

    pub fn get_chunk_by_id(&self, chunk_id: i64) -> Result<Option<Chunk>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT file_path, content, preamble, content_hash, line_start, line_end, language
               FROM chunks WHERE id = ?1"#,
        )?;

        let mut rows = stmt.query_map(params![chunk_id], |row| {
            let file_str: String = row.get(0)?;
            let lang_str: String = row.get(6)?;
            Ok(Chunk {
                file_path: std::path::PathBuf::from(file_str),
                content: row.get(1)?,
                preamble: row.get(2)?,
                content_hash: row.get(3)?,
                line_start: row.get::<_, i64>(4)? as usize,
                line_end: row.get::<_, i64>(5)? as usize,
                language: parse_language(&lang_str),
            })
        })?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn store_chunk_vector(&self, chunk_id: i64, embedding: &[f32]) -> Result<()> {
        let blob = vec_to_bytes(embedding);
        self.conn.execute(
            r#"INSERT OR REPLACE INTO chunk_vectors (chunk_id, embedding) VALUES (?1, ?2)"#,
            params![chunk_id, blob],
        )?;
        Ok(())
    }

    pub fn get_all_chunk_vectors(&self) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT chunk_id, embedding FROM chunk_vectors")?;

        let results = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let blob: Vec<u8> = row.get(1)?;
                Ok((id, blob))
            })?
            .map(|r| {
                let (id, blob) = r?;
                Ok((id, bytes_to_vec(&blob)))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(results)
    }

    pub fn get_cached_embedding(&self, content_hash: &str, provider: &str, dimensions: usize) -> Result<Option<Vec<f32>>> {
        let mut stmt = self.conn.prepare(
            "SELECT vector FROM embedding_cache WHERE content_hash = ?1 AND provider = ?2 AND dimensions = ?3"
        )?;
        let result = stmt.query_row(params![content_hash, provider, dimensions], |row| {
            let blob: Vec<u8> = row.get(0)?;
            Ok(bytes_to_vec(&blob))
        });
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn store_cached_embedding(&self, content_hash: &str, provider: &str, dimensions: usize, vector: &[f32]) -> Result<()> {
        let blob = vec_to_bytes(vector);
        self.conn.execute(
            "INSERT OR REPLACE INTO embedding_cache (content_hash, provider, dimensions, vector) VALUES (?1, ?2, ?3, ?4)",
            params![content_hash, provider, dimensions, blob],
        )?;
        Ok(())
    }

    pub fn upsert_indexed_file(
        &self,
        file_path: &str,
        content_hash: &str,
        language: Language,
        symbol_count: usize,
    ) -> Result<()> {
        self.conn.execute(
            r#"INSERT INTO indexed_files (file_path, content_hash, language, symbol_count)
               VALUES (?1, ?2, ?3, ?4)
               ON CONFLICT(file_path) DO UPDATE SET
                   content_hash = excluded.content_hash,
                   language     = excluded.language,
                   symbol_count = excluded.symbol_count,
                   indexed_at   = datetime('now')"#,
            params![
                file_path,
                content_hash,
                language.as_str(),
                symbol_count as i64
            ],
        )?;
        Ok(())
    }

    pub fn get_file_content_hash(&self, file_path: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT content_hash FROM indexed_files WHERE file_path = ?1")?;

        let mut rows = stmt.query_map(params![file_path], |row| row.get(0))?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn delete_file_data(&self, file_path: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM symbols WHERE file_path = ?1", params![file_path])?;
        self.conn.execute(
            "DELETE FROM chunk_vectors WHERE chunk_id IN (SELECT id FROM chunks WHERE file_path = ?1)",
            params![file_path],
        )?;
        self.conn
            .execute("DELETE FROM chunks WHERE file_path = ?1", params![file_path])?;
        self.conn.execute(
            "DELETE FROM indexed_files WHERE file_path = ?1",
            params![file_path],
        )?;
        self.conn.execute(
            "DELETE FROM file_deps WHERE source_file = ?1",
            params![file_path],
        )?;
        Ok(())
    }

    pub fn insert_deps(&self, deps: &[DepEdge]) -> Result<()> {
        let mut stmt = self.conn.prepare(
            r#"INSERT INTO file_deps (source_file, target_path, kind, line_number)
               VALUES (?1, ?2, ?3, ?4)"#,
        )?;

        for dep in deps {
            stmt.execute(params![
                dep.source_file.to_string_lossy().as_ref(),
                dep.target_path,
                parse_import_kind_to_str(dep.kind),
                dep.line_number as i64,
            ])?;
        }
        Ok(())
    }

    pub fn get_deps_outgoing(&self, file_path: &str) -> Result<Vec<DepEdge>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT source_file, target_path, kind, line_number
               FROM file_deps WHERE source_file = ?1"#,
        )?;

        let results = stmt
            .query_map(params![file_path], |row| {
                let source: String = row.get(0)?;
                let kind_str: String = row.get(2)?;
                Ok(DepEdge {
                    source_file: std::path::PathBuf::from(source),
                    target_path: row.get(1)?,
                    kind: parse_import_kind(&kind_str),
                    line_number: row.get::<_, i64>(3)? as usize,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(results)
    }

    pub fn get_deps_incoming(&self, target_path: &str) -> Result<Vec<DepEdge>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT source_file, target_path, kind, line_number
               FROM file_deps WHERE target_path = ?1"#,
        )?;

        let results = stmt
            .query_map(params![target_path], |row| {
                let source: String = row.get(0)?;
                let kind_str: String = row.get(2)?;
                Ok(DepEdge {
                    source_file: std::path::PathBuf::from(source),
                    target_path: row.get(1)?,
                    kind: parse_import_kind(&kind_str),
                    line_number: row.get::<_, i64>(3)? as usize,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(results)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO index_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM index_meta WHERE key = ?1")?;

        let mut rows = stmt.query_map(params![key], |row| row.get(0))?;

        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn get_status(&self) -> Result<IndexStatus> {
        let total_symbols: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| {
                row.get::<_, i64>(0)
            })? as usize;

        let total_files: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM indexed_files", [], |row| {
                row.get::<_, i64>(0)
            })? as usize;

        let total_chunks: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                row.get::<_, i64>(0)
            })? as usize;

        let embedded_chunks: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunk_vectors", [], |row| {
                row.get::<_, i64>(0)
            })? as usize;

        let embedding_progress = if total_chunks == 0 {
            1.0
        } else {
            embedded_chunks as f64 / total_chunks as f64
        };

        let mut lang_stmt = self.conn.prepare(
            "SELECT language, COUNT(*) FROM symbols GROUP BY language ORDER BY COUNT(*) DESC",
        )?;

        let by_language = lang_stmt
            .query_map([], |row| {
                let lang_str: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((lang_str, count as usize))
            })?
            .filter_map(|r| r.ok())
            .map(|(lang_str, count)| (parse_language(&lang_str), count))
            .collect();

        let last_indexed_commit = self.get_meta("last_indexed_commit").unwrap_or(None);
        let active_provider = self.get_meta("active_provider").unwrap_or(None);

        Ok(IndexStatus {
            total_symbols,
            total_files,
            by_language,
            embedding_progress,
            active_provider,
            last_indexed_commit,
            is_stale: false,
        })
    }
}

fn vec_to_bytes(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &f in vec {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

fn bytes_to_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr = [chunk[0], chunk[1], chunk[2], chunk[3]];
            f32::from_le_bytes(arr)
        })
        .collect()
}

fn parse_symbol_kind(s: &str) -> SymbolKind {
    match s {
        "function" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "struct" => SymbolKind::Struct,
        "class" => SymbolKind::Class,
        "interface" => SymbolKind::Interface,
        "trait" => SymbolKind::Trait,
        "enum" => SymbolKind::Enum,
        "constant" => SymbolKind::Constant,
        "variable" => SymbolKind::Variable,
        "module" => SymbolKind::Module,
        "type" => SymbolKind::Type,
        "impl" => SymbolKind::Impl,
        _ => SymbolKind::Function,
    }
}

fn parse_language(s: &str) -> Language {
    match s {
        "rust" => Language::Rust,
        "typescript" => Language::TypeScript,
        "python" => Language::Python,
        "go" => Language::Go,
        "java" => Language::Java,
        "c" => Language::C,
        "cpp" => Language::Cpp,
        "ruby" => Language::Ruby,
        "swift" => Language::Swift,
        _ => Language::Rust,
    }
}

fn parse_import_kind(s: &str) -> ImportKind {
    match s {
        "use" => ImportKind::Use,
        "import" => ImportKind::Import,
        "include" => ImportKind::Include,
        "require" => ImportKind::Require,
        _ => ImportKind::Import,
    }
}

fn parse_import_kind_to_str(kind: ImportKind) -> &'static str {
    match kind {
        ImportKind::Use => "use",
        ImportKind::Import => "import",
        ImportKind::Include => "include",
        ImportKind::Require => "require",
    }
}
