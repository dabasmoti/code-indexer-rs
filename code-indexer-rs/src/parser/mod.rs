use crate::types::*;
use std::path::Path;

mod c_cpp;
mod go;
mod java;
mod python;
mod ruby;
mod rust;
mod swift;
mod typescript;

pub trait LanguageParser: Send + Sync {
    fn language(&self) -> Language;
    fn extensions(&self) -> &[&str];
    fn parse_symbols(&self, source: &[u8], file_path: &Path) -> Vec<Symbol>;
    fn parse_imports(&self, source: &[u8], file_path: &Path) -> Vec<Import>;
    fn chunk_file(&self, source: &[u8], file_path: &Path) -> Vec<Chunk>;
}

pub fn create_parser(language: Language) -> Box<dyn LanguageParser> {
    match language {
        Language::Rust => Box::new(rust::RustParser::new()),
        Language::TypeScript => Box::new(typescript::TypeScriptParser::new()),
        Language::Python => Box::new(python::PythonParser::new()),
        Language::Go => Box::new(go::GoParser::new()),
        Language::Java => Box::new(java::JavaParser::new()),
        Language::C => Box::new(c_cpp::CParser::new()),
        Language::Cpp => Box::new(c_cpp::CppParser::new()),
        Language::Ruby => Box::new(ruby::RubyParser::new()),
        Language::Swift => Box::new(swift::SwiftParser::new()),
    }
}

pub fn parser_for_extension(ext: &str) -> Option<Box<dyn LanguageParser>> {
    Language::from_extension(ext).map(create_parser)
}
