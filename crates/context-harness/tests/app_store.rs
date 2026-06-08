use chrono::Utc;
use context_harness::app_store::{AppStore, SqliteAppStore};
use context_harness::chunk::chunk_text;
use context_harness::config::Config;
use context_harness::models::{Document, SourceItem};
use context_harness::sqlite_store::SqliteStore;
use context_harness::vector_index::{
    self, BruteForceSqliteVectorIndex, DisabledVectorIndex, VectorIndex, VectorSearchOptions,
};
use context_harness_core::search::{search, SearchParams, SearchRequest};
use context_harness_core::store::Store;
use tempfile::TempDir;

fn test_config(tmp: &TempDir) -> Config {
    let db_path = tmp.path().join("ctx.sqlite");
    let config_content = format!(
        r#"
[db]
path = "{}"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:0"
"#,
        db_path.display()
    );
    toml::from_str(&config_content).unwrap()
}

fn test_config_without_vector_index(tmp: &TempDir) -> Config {
    let db_path = tmp.path().join("ctx.sqlite");
    let config_content = format!(
        r#"
[db]
path = "{}"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:0"
"#,
        db_path.display()
    );
    toml::from_str(&config_content).unwrap()
}

fn document(id: &str, source: &str, source_id: &str, body: &str) -> Document {
    Document {
        id: id.to_string(),
        source: source.to_string(),
        source_id: source_id.to_string(),
        source_url: None,
        title: Some(source_id.to_string()),
        author: None,
        created_at: 1,
        updated_at: 2,
        content_type: "text/plain".to_string(),
        body: body.to_string(),
        metadata_json: "{}".to_string(),
        raw_json: None,
        dedup_hash: format!("hash-{id}"),
    }
}

async fn initialized_store(tmp: &TempDir) -> SqliteAppStore {
    let cfg = test_config(tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    SqliteAppStore::connect(&cfg).await.unwrap()
}

async fn seed_document(
    store: &SqliteAppStore,
    id: &str,
    source: &str,
    source_id: &str,
    body: &str,
) {
    let doc = document(id, source, source_id, body);
    store.upsert_document(&doc).await.unwrap();
    let chunks = chunk_text(id, body, 700);
    store.replace_chunks(id, &chunks, None).await.unwrap();
}

async fn seed_vector_documents(store: &SqliteAppStore) {
    seed_document(
        store,
        "doc-a",
        "filesystem:test",
        "a.md",
        "alpha beta deployment local-first MCP-compatible multi-repo",
    )
    .await;
    seed_document(
        store,
        "doc-b",
        "filesystem:test",
        "b.md",
        "gamma delta security",
    )
    .await;

    let pending = store.find_pending_chunks("model-a", None).await.unwrap();
    for item in pending {
        let vector = if item.document_id == "doc-a" {
            vec![1.0, 0.0]
        } else {
            vec![0.0, 1.0]
        };
        store
            .upsert_embedding(
                &item.chunk_id,
                &item.document_id,
                &vector,
                "model-a",
                2,
                &item.text_hash,
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn sqlite_app_store_checkpoint_round_trip() {
    let tmp = TempDir::new().unwrap();
    let store = initialized_store(&tmp).await;

    assert_eq!(store.get_checkpoint("filesystem:docs").await.unwrap(), None);
    store
        .set_checkpoint("filesystem:docs", 1_700_000_000)
        .await
        .unwrap();
    assert_eq!(
        store.get_checkpoint("filesystem:docs").await.unwrap(),
        Some(1_700_000_000)
    );
}

#[tokio::test]
async fn find_pending_chunks_detects_missing_and_stale_embeddings() {
    let tmp = TempDir::new().unwrap();
    let store = initialized_store(&tmp).await;
    seed_document(&store, "doc-a", "filesystem:test", "a.md", "alpha beta").await;

    let pending = store.find_pending_chunks("model-a", None).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].document_id, "doc-a");

    store
        .upsert_embedding(
            &pending[0].chunk_id,
            &pending[0].document_id,
            &[1.0, 0.0],
            "model-a",
            2,
            "stale-hash",
        )
        .await
        .unwrap();
    let stale = store.find_pending_chunks("model-a", None).await.unwrap();
    assert_eq!(stale.len(), 1);

    store
        .upsert_embedding(
            &pending[0].chunk_id,
            &pending[0].document_id,
            &[1.0, 0.0],
            "model-a",
            2,
            &stale[0].text_hash,
        )
        .await
        .unwrap();
    assert!(store
        .find_pending_chunks("model-a", None)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn clear_embeddings_removes_vectors_but_keeps_documents_and_chunks() {
    let tmp = TempDir::new().unwrap();
    let store = initialized_store(&tmp).await;
    seed_document(&store, "doc-a", "filesystem:test", "a.md", "alpha beta").await;
    let pending = store.find_pending_chunks("model-a", None).await.unwrap();
    store
        .upsert_embedding(
            &pending[0].chunk_id,
            "doc-a",
            &[1.0, 0.0],
            "model-a",
            2,
            &pending[0].text_hash,
        )
        .await
        .unwrap();

    let before = store.stats().await.unwrap();
    assert_eq!(before.total_docs, 1);
    assert_eq!(before.total_chunks, 1);
    assert_eq!(before.total_embedded, 1);

    store.clear_embeddings().await.unwrap();
    let after = store.stats().await.unwrap();
    assert_eq!(after.total_docs, 1);
    assert_eq!(after.total_chunks, 1);
    assert_eq!(after.total_embedded, 0);
}

#[tokio::test]
async fn export_index_matches_ctx_export_json_shape() {
    let tmp = TempDir::new().unwrap();
    let store = initialized_store(&tmp).await;
    seed_document(&store, "doc-a", "filesystem:test", "a.md", "alpha beta").await;

    let exported = store.export_index().await.unwrap();
    let json = serde_json::to_value(&exported).unwrap();

    assert_eq!(json["documents"][0]["id"], "doc-a");
    assert_eq!(json["documents"][0]["source"], "filesystem:test");
    assert_eq!(json["documents"][0]["source_id"], "a.md");
    assert_eq!(json["documents"][0]["updated_at"], 2);
    assert_eq!(json["documents"][0]["body"], "alpha beta");
    assert_eq!(json["chunks"][0]["document_id"], "doc-a");
    assert_eq!(json["chunks"][0]["chunk_index"], 0);
    assert_eq!(json["chunks"][0]["text"], "alpha beta");
}

#[tokio::test]
async fn keyword_search_treats_hyphenated_terms_as_user_text() {
    let tmp = TempDir::new().unwrap();
    let store = initialized_store(&tmp).await;
    seed_document(
        &store,
        "doc-a",
        "filesystem:test",
        "a.md",
        "Context Harness provides local-first retrieval for MCP-compatible, multi-repo work.",
    )
    .await;

    let sqlite = SqliteStore::new(store.pool().clone());
    let candidates = sqlite
        .keyword_search(
            "Context Harness local-first MCP-compatible multi-repo",
            10,
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].document_id, "doc-a");
}

#[tokio::test]
async fn hybrid_search_treats_hyphenated_terms_as_user_text() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_without_vector_index(&tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let store = SqliteAppStore::connect(&cfg).await.unwrap();
    seed_vector_documents(&store).await;

    let sqlite = SqliteStore::new(store.pool().clone());
    let params = SearchParams {
        hybrid_alpha: 0.6,
        candidate_k_keyword: 10,
        candidate_k_vector: 10,
        final_limit: 10,
    };
    let req = SearchRequest {
        query: "deployment local-first MCP-compatible multi-repo",
        query_vec: Some(&[0.9, 0.1]),
        mode: "hybrid",
        source_filter: None,
        since: None,
        params,
        explain: true,
    };

    let results = search(&sqlite, &req).await.unwrap();

    assert_eq!(results[0].id, "doc-a");
    assert_eq!(results[0].explain.as_ref().unwrap().keyword_candidates, 1);
}

#[tokio::test]
async fn stats_match_document_chunk_and_embedding_counts() {
    let tmp = TempDir::new().unwrap();
    let store = initialized_store(&tmp).await;
    seed_document(&store, "doc-a", "filesystem:test", "a.md", "alpha beta").await;
    seed_document(&store, "doc-b", "filesystem:test", "b.md", "gamma delta").await;

    let pending = store.find_pending_chunks("model-a", None).await.unwrap();
    store
        .upsert_embedding(
            &pending[0].chunk_id,
            &pending[0].document_id,
            &[1.0, 0.0],
            "model-a",
            2,
            &pending[0].text_hash,
        )
        .await
        .unwrap();

    let stats = store.stats().await.unwrap();
    assert_eq!(stats.total_docs, 2);
    assert_eq!(stats.total_chunks, 2);
    assert_eq!(stats.total_embedded, 1);
    assert_eq!(stats.sources.len(), 1);
    assert_eq!(stats.sources[0].source, "filesystem:test");
    assert_eq!(stats.sources[0].doc_count, 2);
    assert_eq!(stats.sources[0].chunk_count, 2);
    assert_eq!(stats.sources[0].embedded_count, 1);
}

#[tokio::test]
async fn brute_force_vector_index_matches_sqlite_vector_search_ordering() {
    let tmp = TempDir::new().unwrap();
    let store = initialized_store(&tmp).await;
    seed_document(&store, "doc-a", "filesystem:test", "a.md", "alpha beta").await;
    seed_document(&store, "doc-b", "filesystem:test", "b.md", "gamma delta").await;

    let pending = store.find_pending_chunks("model-a", None).await.unwrap();
    store
        .upsert_embedding(
            &pending[0].chunk_id,
            &pending[0].document_id,
            &[1.0, 0.0],
            "model-a",
            2,
            &pending[0].text_hash,
        )
        .await
        .unwrap();
    store
        .upsert_embedding(
            &pending[1].chunk_id,
            &pending[1].document_id,
            &[0.0, 1.0],
            "model-a",
            2,
            &pending[1].text_hash,
        )
        .await
        .unwrap();

    let sqlite = SqliteStore::new(store.pool().clone());
    let baseline = sqlite
        .vector_search(&[0.9, 0.1], 2, None, None)
        .await
        .unwrap();
    let vector_index = BruteForceSqliteVectorIndex::new(SqliteStore::new(store.pool().clone()));
    let accelerated = vector_index
        .search(
            &[0.9, 0.1],
            VectorSearchOptions {
                limit: 2,
                source: None,
                since: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(baseline.len(), accelerated.len());
    for (left, right) in baseline.iter().zip(accelerated.iter()) {
        assert_eq!(left.chunk_id, right.chunk_id);
        assert_eq!(left.document_id, right.document_id);
        assert_eq!(left.snippet, right.snippet);
        assert!((left.raw_score - right.raw_score).abs() < f64::EPSILON);
    }
}

#[tokio::test]
async fn auto_vector_index_config_preserves_sqlite_fallback_defaults() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_without_vector_index(&tmp);

    assert_eq!(cfg.vector_index.backend, "auto");
    assert_eq!(cfg.vector_index.path, std::path::PathBuf::from("auto"));
    assert_eq!(cfg.vector_index.index, "hnsw");
    assert_eq!(cfg.vector_index.fallback, "sqlite");

    let disabled = DisabledVectorIndex;
    let health = disabled.health().await.unwrap();
    assert!(!health.enabled);
    assert!(!health.available);
    assert_eq!(health.backend, "disabled");
    assert!(disabled
        .search(
            &[1.0, 0.0],
            VectorSearchOptions {
                limit: 10,
                ..Default::default()
            }
        )
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn auto_vector_index_uses_sqlite_fallback_without_explicit_config() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_without_vector_index(&tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let store = SqliteAppStore::connect(&cfg).await.unwrap();
    seed_vector_documents(&store).await;

    let indexed = vector_index::configured_vector_store(&cfg, store.pool().clone())
        .await
        .unwrap();
    let candidates = indexed
        .vector_search(&[0.9, 0.1], 2, None, None)
        .await
        .unwrap();

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].document_id, "doc-a");
}

#[tokio::test]
async fn vector_index_auto_path_resolves_beside_sqlite_database() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_without_vector_index(&tmp);

    assert_eq!(
        vector_index::resolve_vector_index_path(&cfg),
        tmp.path().join("vector-index").join("zvec")
    );

    let minimal = Config::minimal();
    assert_eq!(
        vector_index::resolve_vector_index_path(&minimal),
        std::path::PathBuf::from(".ctx/data/vector-index/zvec")
    );
}

#[tokio::test]
async fn explicit_disabled_backend_still_preserves_sqlite_fallback_search() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = test_config_without_vector_index(&tmp);
    cfg.vector_index.backend = "disabled".to_string();
    cfg.vector_index.fallback = "sqlite".to_string();
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let store = SqliteAppStore::connect(&cfg).await.unwrap();
    seed_vector_documents(&store).await;

    let indexed = vector_index::configured_vector_store(&cfg, store.pool().clone())
        .await
        .unwrap();
    let candidates = indexed
        .vector_search(&[0.1, 0.9], 2, None, None)
        .await
        .unwrap();

    assert_eq!(candidates[0].document_id, "doc-b");
}

#[cfg(feature = "zvec-bundled")]
#[tokio::test]
async fn zvec_sidecar_builds_and_queries_candidates() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_without_vector_index(&tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let store = SqliteAppStore::connect(&cfg).await.unwrap();
    seed_vector_documents(&store).await;

    let indexed = vector_index::configured_vector_store(&cfg, store.pool().clone())
        .await
        .unwrap();
    let candidates = indexed
        .vector_search(&[0.9, 0.1], 2, None, None)
        .await
        .unwrap();
    let status = vector_index::vector_index_status(&cfg).await.unwrap();

    assert_eq!(candidates[0].document_id, "doc-a");
    assert_eq!(status.health.backend, "zvec");
    assert!(status.health.available);
    assert!(status.fresh);
    assert!(status.path.join("manifest.json").exists());
}

#[cfg(feature = "zvec-bundled")]
#[tokio::test]
async fn zvec_semantic_and_hybrid_search_remain_compatible() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_without_vector_index(&tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let store = SqliteAppStore::connect(&cfg).await.unwrap();
    seed_vector_documents(&store).await;

    let sqlite = SqliteStore::new(store.pool().clone());
    let indexed = vector_index::configured_vector_store(&cfg, store.pool().clone())
        .await
        .unwrap();
    let params = SearchParams {
        hybrid_alpha: 0.6,
        candidate_k_keyword: 10,
        candidate_k_vector: 10,
        final_limit: 10,
    };
    let semantic_req = SearchRequest {
        query: "deployment",
        query_vec: Some(&[0.9, 0.1]),
        mode: "semantic",
        source_filter: None,
        since: None,
        params: params.clone(),
        explain: true,
    };
    let sqlite_semantic = search(&sqlite, &semantic_req).await.unwrap();
    let zvec_semantic = search(&indexed, &semantic_req).await.unwrap();

    assert_eq!(sqlite_semantic[0].id, zvec_semantic[0].id);
    assert_eq!(zvec_semantic[0].id, "doc-a");

    let hybrid_req = SearchRequest {
        query: "deployment",
        query_vec: Some(&[0.9, 0.1]),
        mode: "hybrid",
        source_filter: None,
        since: None,
        params,
        explain: true,
    };
    let hybrid = search(&indexed, &hybrid_req).await.unwrap();
    let explain = hybrid[0].explain.as_ref().unwrap();

    assert_eq!(hybrid[0].id, "doc-a");
    assert_eq!(explain.keyword_candidates, 1);
    assert_eq!(explain.vector_candidates, 2);
    assert!((explain.alpha - 0.6).abs() < f64::EPSILON);
}

#[cfg(feature = "zvec-bundled")]
#[tokio::test]
async fn zvec_rebuild_handles_missing_and_stale_sidecar_state() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_without_vector_index(&tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let store = SqliteAppStore::connect(&cfg).await.unwrap();
    seed_vector_documents(&store).await;

    let indexed = vector_index::configured_vector_store(&cfg, store.pool().clone())
        .await
        .unwrap();
    assert_eq!(
        indexed
            .vector_search(&[0.9, 0.1], 2, None, None)
            .await
            .unwrap()[0]
            .document_id,
        "doc-a"
    );
    drop(indexed);

    vector_index::remove_configured_sidecar(&cfg).unwrap();
    let rebuilt = vector_index::configured_vector_store(&cfg, store.pool().clone())
        .await
        .unwrap();
    assert_eq!(
        rebuilt
            .vector_search(&[0.9, 0.1], 2, None, None)
            .await
            .unwrap()[0]
            .document_id,
        "doc-a"
    );
    drop(rebuilt);

    std::fs::write(
        vector_index::resolve_vector_index_path(&cfg).join("manifest.json"),
        r#"{"version":1,"backend":"zvec","vector_count":0,"model":null,"dims":null,"metric":"cosine","index":"hnsw","digest":"stale"}"#,
    )
    .unwrap();
    let status = vector_index::rebuild_configured_vector_index(&cfg)
        .await
        .unwrap();
    assert!(status.fresh);
    assert_eq!(status.manifest.unwrap().vector_count, 2);
}

#[cfg(feature = "zvec-bundled")]
#[tokio::test]
async fn auto_zvec_falls_back_to_sqlite_when_sidecar_is_unhealthy() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = test_config_without_vector_index(&tmp);
    cfg.vector_index.path = tmp.path().join("not-a-directory");
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let store = SqliteAppStore::connect(&cfg).await.unwrap();
    seed_vector_documents(&store).await;
    std::fs::write(&cfg.vector_index.path, "blocking file").unwrap();

    let indexed = vector_index::configured_vector_store(&cfg, store.pool().clone())
        .await
        .unwrap();
    let candidates = indexed
        .vector_search(&[0.1, 0.9], 2, None, None)
        .await
        .unwrap();

    assert_eq!(candidates[0].document_id, "doc-b");
}

#[tokio::test]
async fn sqlite_app_store_upserts_source_items_as_canonical_documents() {
    let tmp = TempDir::new().unwrap();
    let store = initialized_store(&tmp).await;
    let now = Utc::now();
    let item = SourceItem {
        source: "filesystem:test".to_string(),
        source_id: "source.md".to_string(),
        source_url: Some("file:///source.md".to_string()),
        title: Some("Source".to_string()),
        author: Some("tester".to_string()),
        created_at: now,
        updated_at: now,
        content_type: "text/plain".to_string(),
        body: "source body".to_string(),
        metadata_json: "{}".to_string(),
        raw_json: None,
        raw_bytes: None,
    };

    let first_id = store.upsert_source_item(&item).await.unwrap();
    let second_id = store.upsert_source_item(&item).await.unwrap();
    assert_eq!(first_id, second_id);

    let doc = store.get_document(&first_id).await.unwrap().unwrap();
    assert_eq!(doc.source, "filesystem:test");
    assert_eq!(doc.source_id, "source.md");
    assert_eq!(doc.body, "source body");
}
