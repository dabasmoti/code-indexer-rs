use code_indexer::embedding::{EmbeddingProvider, detect_provider};
use code_indexer::config::Config;

#[tokio::test]
async fn test_provider_detection_graceful_fallback() {
    // With no providers running, detection should return None
    let config = Config::default();
    let provider = detect_provider(&config).await;
    // In CI with no services, this returns None (BM25-only mode)
    // We just verify it doesn't panic
    if provider.is_none() {
        println!("No embedding provider available - BM25-only mode");
    }
}

#[tokio::test]
async fn test_jina_provider_health_check_offline() {
    use code_indexer::embedding::jina::JinaProvider;
    use code_indexer::config::JinaGrepConfig;

    let config = JinaGrepConfig {
        url: "http://localhost:19999".to_string(), // unlikely to be running
        ..Default::default()
    };
    let provider = JinaProvider::new(&config);
    assert!(!provider.health_check().await);
}

#[tokio::test]
async fn test_ollama_provider_health_check_offline() {
    use code_indexer::embedding::ollama::OllamaProvider;
    use code_indexer::config::OllamaConfig;

    let config = OllamaConfig {
        url: "http://localhost:19998".to_string(),
        ..Default::default()
    };
    let provider = OllamaProvider::new(&config);
    assert!(!provider.health_check().await);
}
