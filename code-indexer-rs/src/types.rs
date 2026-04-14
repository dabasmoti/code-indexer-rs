use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Swift,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "ts" | "tsx" => Some(Self::TypeScript),
            "py" => Some(Self::Python),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some(Self::Cpp),
            "rb" => Some(Self::Ruby),
            "swift" => Some(Self::Swift),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Ruby => "ruby",
            Self::Swift => "swift",
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Class,
    Interface,
    Trait,
    Enum,
    Constant,
    Variable,
    Module,
    Type,
    Impl,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Enum => "enum",
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::Module => "module",
            Self::Type => "type",
            Self::Impl => "impl",
        }
    }
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub language: Language,
    pub file_path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub visibility: Option<String>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub file_path: PathBuf,
    pub content: String,
    pub preamble: String,
    pub content_hash: String,
    pub line_start: usize,
    pub line_end: usize,
    pub language: Language,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub source_file: PathBuf,
    pub target_path: String,
    pub kind: ImportKind,
    pub line_number: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportKind {
    Use,
    Import,
    Include,
    Require,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub snippet: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<SymbolKind>,
    pub score: f64,
    pub language: Language,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSearchResult {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub visibility: Option<String>,
    pub score: f64,
    pub language: Language,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepEdge {
    pub source_file: PathBuf,
    pub target_path: String,
    pub kind: ImportKind,
    pub line_number: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatus {
    pub total_symbols: usize,
    pub total_files: usize,
    pub by_language: Vec<(Language, usize)>,
    pub embedding_progress: f64,
    pub active_provider: Option<String>,
    pub last_indexed_commit: Option<String>,
    pub is_stale: bool,
}
