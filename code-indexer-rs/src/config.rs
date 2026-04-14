use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_MAX_FILE_SIZE: u64 = 1_048_576;
const DEFAULT_EMBEDDING_PROVIDER: &str = "auto";
const DEFAULT_JINA_GREP_URL: &str = "http://localhost:8089";
const DEFAULT_JINA_GREP_MODEL: &str = "jina-code-embeddings-0.5b";
const DEFAULT_JINA_GREP_TRUNCATE_DIM: u32 = 256;
const DEFAULT_JINA_GREP_BATCH_SIZE: usize = 64;
const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";
const DEFAULT_OLLAMA_MODEL: &str = "nomic-embed-text";
const DEFAULT_OLLAMA_BATCH_SIZE: usize = 64;
const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_SEARCH_MAX_LIMIT: usize = 100;
const DEFAULT_RRF_K: usize = 60;
const DEFAULT_BM25_SYMBOL_BOOST: f64 = 1.5;
const DEFAULT_DB_DIR: &str = ".code-indexer";

fn default_exclude_dirs() -> Vec<String> {
    vec![
        "target".to_string(),
        "node_modules".to_string(),
        ".git".to_string(),
        "dist".to_string(),
        "build".to_string(),
        "vendor".to_string(),
        "__pycache__".to_string(),
    ]
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        "*.min.js".to_string(),
        "*.generated.*".to_string(),
        "*.pb.go".to_string(),
    ]
}

fn default_max_file_size() -> u64 {
    DEFAULT_MAX_FILE_SIZE
}

fn default_embedding_provider() -> String {
    DEFAULT_EMBEDDING_PROVIDER.to_string()
}

fn default_embedding_enabled() -> bool {
    true
}

fn default_jina_grep_url() -> String {
    DEFAULT_JINA_GREP_URL.to_string()
}

fn default_jina_grep_model() -> String {
    DEFAULT_JINA_GREP_MODEL.to_string()
}

fn default_jina_grep_truncate_dim() -> Option<u32> {
    Some(DEFAULT_JINA_GREP_TRUNCATE_DIM)
}

fn default_jina_grep_batch_size() -> usize {
    DEFAULT_JINA_GREP_BATCH_SIZE
}

fn default_ollama_url() -> String {
    DEFAULT_OLLAMA_URL.to_string()
}

fn default_ollama_model() -> String {
    DEFAULT_OLLAMA_MODEL.to_string()
}

fn default_ollama_batch_size() -> usize {
    DEFAULT_OLLAMA_BATCH_SIZE
}

fn default_search_limit() -> usize {
    DEFAULT_SEARCH_LIMIT
}

fn default_search_max_limit() -> usize {
    DEFAULT_SEARCH_MAX_LIMIT
}

fn default_rrf_k() -> usize {
    DEFAULT_RRF_K
}

fn default_bm25_symbol_boost() -> f64 {
    DEFAULT_BM25_SYMBOL_BOOST
}

fn default_db_dir() -> String {
    DEFAULT_DB_DIR.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct JinaGrepConfig {
    #[serde(default = "default_jina_grep_url")]
    pub url: String,
    #[serde(default = "default_jina_grep_model")]
    pub model: String,
    pub task: Option<String>,
    #[serde(default = "default_jina_grep_truncate_dim")]
    pub truncate_dim: Option<u32>,
    #[serde(default = "default_jina_grep_batch_size")]
    pub batch_size: usize,
}

impl Default for JinaGrepConfig {
    fn default() -> Self {
        Self {
            url: default_jina_grep_url(),
            model: default_jina_grep_model(),
            task: None,
            truncate_dim: default_jina_grep_truncate_dim(),
            batch_size: default_jina_grep_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_url")]
    pub url: String,
    #[serde(default = "default_ollama_model")]
    pub model: String,
    #[serde(default = "default_ollama_batch_size")]
    pub batch_size: usize,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            url: default_ollama_url(),
            model: default_ollama_model(),
            batch_size: default_ollama_batch_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAiCompatConfig {
    pub url: Option<String>,
    pub model: String,
    pub api_key_env: Option<String>,
    pub batch_size: usize,
}

impl Default for OpenAiCompatConfig {
    fn default() -> Self {
        Self {
            url: None,
            model: "text-embedding-ada-002".to_string(),
            api_key_env: None,
            batch_size: 512,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embedding_provider")]
    pub provider: String,
    #[serde(default = "default_embedding_enabled")]
    pub enabled: bool,
    pub jina_grep: JinaGrepConfig,
    pub ollama: OllamaConfig,
    pub openai_compat: OpenAiCompatConfig,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_embedding_provider(),
            enabled: default_embedding_enabled(),
            jina_grep: JinaGrepConfig::default(),
            ollama: OllamaConfig::default(),
            openai_compat: OpenAiCompatConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexerConfig {
    pub languages: Vec<String>,
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,
    #[serde(default = "default_exclude_dirs")]
    pub exclude_dirs: Vec<String>,
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
    pub watch: bool,
    pub watch_debounce_ms: u64,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            languages: vec![],
            max_file_size: default_max_file_size(),
            exclude_dirs: default_exclude_dirs(),
            exclude_patterns: default_exclude_patterns(),
            watch: false,
            watch_debounce_ms: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    #[serde(default = "default_search_limit")]
    pub default_limit: usize,
    #[serde(default = "default_search_max_limit")]
    pub max_limit: usize,
    #[serde(default = "default_rrf_k")]
    pub rrf_k: usize,
    #[serde(default = "default_bm25_symbol_boost")]
    pub bm25_symbol_boost: f64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: default_search_limit(),
            max_limit: default_search_max_limit(),
            rrf_k: default_rrf_k(),
            bm25_symbol_boost: default_bm25_symbol_boost(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    #[serde(default = "default_db_dir")]
    pub db_dir: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            db_dir: default_db_dir(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub indexer: IndexerConfig,
    pub embedding: EmbeddingConfig,
    pub search: SearchConfig,
    pub storage: StorageConfig,
}

impl Config {
    /// Parse a TOML string into a Config, filling missing fields with defaults.
    pub fn from_toml_str(s: &str) -> Result<Self> {
        let config: Config = toml::from_str(s)?;
        Ok(config)
    }

    /// Layered loading: global config -> project config -> env var overrides.
    pub fn load(repo_path: &Path) -> Result<Self> {
        let mut config = Config::default();

        if let Some(global_path) = dirs_config_path() {
            if global_path.exists() {
                let content = std::fs::read_to_string(&global_path)?;
                let global = Config::from_toml_str(&content)?;
                config = merge_config(config, global);
            }
        }

        let project_config_path = repo_path.join(".code-indexer.toml");
        if project_config_path.exists() {
            let content = std::fs::read_to_string(&project_config_path)?;
            let project = Config::from_toml_str(&content)?;
            config = merge_config(config, project);
        }

        config = Config::from_env_over(config);

        Ok(config)
    }

    /// Apply environment variable overrides on top of an existing Config.
    pub fn from_env_over(mut config: Config) -> Config {
        if let Ok(val) = std::env::var("CODE_INDEXER_EMBEDDING_PROVIDER") {
            config.embedding.provider = val;
        }
        if let Ok(val) = std::env::var("CODE_INDEXER_JINA_GREP_URL") {
            config.embedding.jina_grep.url = val;
        }
        if let Ok(val) = std::env::var("CODE_INDEXER_OLLAMA_URL") {
            config.embedding.ollama.url = val;
        }
        if let Ok(val) = std::env::var("CODE_INDEXER_API_KEY") {
            config.embedding.openai_compat.api_key_env = Some(val);
        }
        if let Ok(val) = std::env::var("CODE_INDEXER_EMBEDDING_ENABLED") {
            if let Ok(enabled) = val.parse::<bool>() {
                config.embedding.enabled = enabled;
            }
        }
        config
    }
}

/// Returns the global config file path: ~/.config/code-indexer/config.toml
fn dirs_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("code-indexer").join("config.toml"))
}

/// Merge two configs: overlay wins over base for all fields.
/// Because serde(default) cannot distinguish "explicitly set" from "defaulted",
/// overlay always fully replaces base sections when the overlay was parsed from user input.
fn merge_config(_base: Config, overlay: Config) -> Config {
    overlay
}
