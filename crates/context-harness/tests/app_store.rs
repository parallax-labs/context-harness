use chrono::Utc;
use context_harness::app_store::{AppStore, SqliteAppStore};
use context_harness::chunk::chunk_text;
use context_harness::config::Config;
use context_harness::models::{Document, SourceItem};
use context_harness::sqlite_store::SqliteStore;
use context_harness::vector_index::{
    BruteForceSqliteVectorIndex, DisabledVectorIndex, VectorIndex, VectorSearchOptions,
};
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
    let cfg = test_config(&tmp);

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
