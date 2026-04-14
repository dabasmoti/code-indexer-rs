
#[test]
fn test_default_config() {
    let config = code_indexer::config::Config::default();
    assert_eq!(config.indexer.max_file_size, 1_048_576);
    assert!(config
        .indexer
        .exclude_dirs
        .contains(&"node_modules".to_string()));
    assert!(config.indexer.exclude_dirs.contains(&"target".to_string()));
    assert_eq!(config.embedding.provider, "auto");
    assert_eq!(config.search.rrf_k, 60);
    assert_eq!(config.search.default_limit, 20);
}

#[test]
fn test_config_from_toml() {
    let toml_content = r#"
[indexer]
max_file_size = 2_000_000
exclude_dirs = ["vendor", "build"]

[embedding]
provider = "ollama"

[search]
default_limit = 50
"#;
    let config = code_indexer::config::Config::from_toml_str(toml_content).unwrap();
    assert_eq!(config.indexer.max_file_size, 2_000_000);
    assert_eq!(config.indexer.exclude_dirs, vec!["vendor", "build"]);
    assert_eq!(config.embedding.provider, "ollama");
    assert_eq!(config.search.default_limit, 50);
    assert_eq!(config.search.rrf_k, 60);
}

#[test]
fn test_config_env_override() {
    std::env::set_var("CODE_INDEXER_EMBEDDING_PROVIDER", "jina");
    let config =
        code_indexer::config::Config::from_env_over(code_indexer::config::Config::default());
    assert_eq!(config.embedding.provider, "jina");
    std::env::remove_var("CODE_INDEXER_EMBEDDING_PROVIDER");
}
