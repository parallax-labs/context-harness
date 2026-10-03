use chrono::{TimeZone, Utc};
use context_harness::{
    app_store::SqliteAppStore,
    config::Config,
    targeted_refresh::{
        CanonicalRefreshStatus, DerivedRefreshStatus, EnrolledSource, RefreshReason,
        TargetedRefresher, MAX_TARGETED_REFRESH_ITEMS,
    },
    SourceItem,
};
use sqlx::Row;
use tempfile::TempDir;

fn config(temp: &TempDir) -> Config {
    let content = temp.path().join("content");
    std::fs::create_dir_all(&content).unwrap();
    toml::from_str(&format!(
        r#"
[db]
path = "{}"

[chunking]
max_tokens = 3

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:0"

[connectors.filesystem.docs]
root = "{}"
include_globs = ["**/*.md"]
exclude_globs = ["private/**"]
max_extract_bytes = 1024
"#,
        temp.path().join("ctx.sqlite").display(),
        content.display()
    ))
    .unwrap()
}

#[tokio::test]
async fn empty_and_oversized_batches_are_rejected_before_writes() {
    let temp = TempDir::new().unwrap();
    let config = config(&temp);
    let enrolled = EnrolledSource::resolve(&config, temp.path(), "filesystem:docs").unwrap();
    let refresher = TargetedRefresher::new(config.clone(), temp.path())
        .await
        .unwrap();
    let inspection = SqliteAppStore::connect(&config).await.unwrap();

    let empty = refresher.refresh(&enrolled, Vec::new()).await;
    assert!(!empty.preflight_passed);
    assert_eq!(empty.reasons, vec![RefreshReason::EmptyBatch]);

    let items = (0..=MAX_TARGETED_REFRESH_ITEMS)
        .map(|index| item(&format!("notes/{index}.md"), "bounded body"))
        .collect();
    let oversized = refresher.refresh(&enrolled, items).await;
    assert!(!oversized.preflight_passed);
    assert_eq!(oversized.reasons, vec![RefreshReason::TooManyItems]);
    assert!(oversized
        .items
        .iter()
        .all(|item| item.canonical == CanonicalRefreshStatus::NotAttempted));

    let documents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents")
        .fetch_one(inspection.pool())
        .await
        .unwrap();
    assert_eq!(documents, 0);
}

fn item(source_id: &str, body: &str) -> SourceItem {
    SourceItem {
        source: "filesystem:docs".into(),
        source_id: source_id.into(),
        source_url: None,
        title: Some(source_id.into()),
        author: None,
        created_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        updated_at: Utc.timestamp_opt(1_700_000_001, 0).unwrap(),
        content_type: "text/markdown".into(),
        body: body.into(),
        metadata_json: "{}".into(),
        raw_json: None,
        raw_bytes: None,
    }
}

#[test]
fn enrollment_is_offline_opaque_and_applies_configured_namespace() {
    let temp = TempDir::new().unwrap();
    let config = config(&temp);
    let db = config.db.path.clone();

    let enrolled = EnrolledSource::resolve(&config, temp.path(), "filesystem:docs").unwrap();
    assert_eq!(enrolled.source_label(), "filesystem:docs");
    assert!(
        !db.exists(),
        "enrollment must not open or create the database"
    );
    assert!(EnrolledSource::resolve(&config, temp.path(), "filesystem:missing").is_err());
}

#[tokio::test]
async fn whole_batch_preflight_rejects_without_writes_or_checkpoint_changes() {
    let temp = TempDir::new().unwrap();
    let config = config(&temp);
    let enrolled = EnrolledSource::resolve(&config, temp.path(), "filesystem:docs").unwrap();
    let refresher = TargetedRefresher::new(config.clone(), temp.path())
        .await
        .unwrap();
    let inspection = SqliteAppStore::connect(&config).await.unwrap();

    let valid = item("notes/valid.md", "valid body");
    let mut invalid = item("../escape.md", "invalid body");
    invalid.metadata_json = "[]".into();
    let report = refresher.refresh(&enrolled, vec![valid, invalid]).await;

    assert!(!report.preflight_passed);
    assert_eq!(
        report.items[0].canonical,
        CanonicalRefreshStatus::NotAttempted
    );
    assert_eq!(report.items[1].canonical, CanonicalRefreshStatus::Rejected);
    assert!(report.items[1]
        .reasons
        .contains(&RefreshReason::InvalidMetadata));
    assert!(report.items[1]
        .reasons
        .contains(&RefreshReason::SourceIdOutsideScope));

    let counts: (i64, i64) = sqlx::query(
        "SELECT COUNT(*) AS docs, (SELECT COUNT(*) FROM checkpoints) AS checkpoints FROM documents",
    )
    .fetch_one(inspection.pool())
    .await
    .map(|row| (row.get("docs"), row.get("checkpoints")))
    .unwrap();
    assert_eq!(counts, (0, 0));
}

#[tokio::test]
async fn duplicate_ids_and_raw_body_conflicts_are_ordered_preflight_rejections() {
    let temp = TempDir::new().unwrap();
    let config = config(&temp);
    let enrolled = EnrolledSource::resolve(&config, temp.path(), "filesystem:docs").unwrap();
    let refresher = TargetedRefresher::new(config, temp.path()).await.unwrap();
    let first = item("notes/duplicate.md", "first");
    let mut second = item("notes/duplicate.md", "must be empty with raw bytes");
    second.raw_bytes = Some(vec![1, 2, 3]);

    let report = refresher.refresh(&enrolled, vec![first, second]).await;
    assert!(!report.preflight_passed);
    assert_eq!(report.items[0].canonical, CanonicalRefreshStatus::Rejected);
    assert_eq!(report.items[1].canonical, CanonicalRefreshStatus::Rejected);
    assert!(report.items[0]
        .reasons
        .contains(&RefreshReason::DuplicateSourceId));
    assert!(report.items[1]
        .reasons
        .contains(&RefreshReason::DuplicateSourceId));
    assert!(report.items[1]
        .reasons
        .contains(&RefreshReason::RawBodyConflict));
}

#[tokio::test]
async fn create_update_and_retry_preserve_identity_checkpoint_and_unrelated_documents() {
    let temp = TempDir::new().unwrap();
    let config = config(&temp);
    let enrolled = EnrolledSource::resolve(&config, temp.path(), "filesystem:docs").unwrap();
    let refresher = TargetedRefresher::new(config.clone(), temp.path())
        .await
        .unwrap();
    let inspection = SqliteAppStore::connect(&config).await.unwrap();

    sqlx::query("INSERT INTO checkpoints (source, cursor, updated_at) VALUES (?, ?, ?)")
        .bind("filesystem:docs")
        .bind("123")
        .bind(456_i64)
        .execute(inspection.pool())
        .await
        .unwrap();

    let first = refresher
        .refresh(
            &enrolled,
            vec![
                item("notes/a.md", "old searchable words"),
                item("notes/b.md", "unrelated retained body"),
            ],
        )
        .await;
    assert!(first.preflight_passed);
    assert_eq!(first.items[0].canonical, CanonicalRefreshStatus::Indexed);
    assert_eq!(
        first.items[0].embedding,
        DerivedRefreshStatus::NotConfigured
    );
    let document_id = first.items[0].document_id.clone().unwrap();

    let second = refresher
        .refresh(&enrolled, vec![item("notes/a.md", "new replacement words")])
        .await;
    assert_eq!(
        second.items[0].document_id.as_deref(),
        Some(document_id.as_str())
    );

    let retry = refresher
        .refresh(&enrolled, vec![item("notes/a.md", "new replacement words")])
        .await;
    assert_eq!(
        retry.items[0].document_id.as_deref(),
        Some(document_id.as_str())
    );

    let row = sqlx::query("SELECT body, (SELECT COUNT(*) FROM documents WHERE source = 'filesystem:docs' AND source_id = 'notes/a.md') AS docs, (SELECT cursor FROM checkpoints WHERE source = 'filesystem:docs') AS cursor FROM documents WHERE id = ?")
        .bind(&document_id)
        .fetch_one(inspection.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("body"), "new replacement words");
    assert_eq!(row.get::<i64, _>("docs"), 1);
    assert_eq!(row.get::<String, _>("cursor"), "123");

    let old_hits: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH 'old'")
            .fetch_one(inspection.pool())
            .await
            .unwrap();
    assert_eq!(old_hits, 0);
    let unrelated: String = sqlx::query_scalar(
        "SELECT body FROM documents WHERE source = 'filesystem:docs' AND source_id = 'notes/b.md'",
    )
    .fetch_one(inspection.pool())
    .await
    .unwrap();
    assert_eq!(unrelated, "unrelated retained body");
}

#[tokio::test]
async fn canonical_replacement_rolls_back_when_chunk_write_fails() {
    let temp = TempDir::new().unwrap();
    let config = config(&temp);
    let enrolled = EnrolledSource::resolve(&config, temp.path(), "filesystem:docs").unwrap();
    let refresher = TargetedRefresher::new(config.clone(), temp.path())
        .await
        .unwrap();
    let inspection = SqliteAppStore::connect(&config).await.unwrap();

    let initial = refresher
        .refresh(&enrolled, vec![item("notes/a.md", "stable original")])
        .await;
    assert_eq!(initial.items[0].canonical, CanonicalRefreshStatus::Indexed);

    sqlx::query(
        "CREATE TRIGGER fail_targeted_chunk BEFORE INSERT ON chunks BEGIN SELECT RAISE(FAIL, 'fixture failure'); END",
    )
    .execute(inspection.pool())
    .await
    .unwrap();
    let failed = refresher
        .refresh(
            &enrolled,
            vec![item("notes/a.md", "forced_failure replacement")],
        )
        .await;
    assert_eq!(failed.items[0].canonical, CanonicalRefreshStatus::Failed);
    assert_eq!(failed.items[0].reasons, vec![RefreshReason::StorageFailed]);

    let body: String = sqlx::query_scalar(
        "SELECT body FROM documents WHERE source = 'filesystem:docs' AND source_id = 'notes/a.md'",
    )
    .fetch_one(inspection.pool())
    .await
    .unwrap();
    assert_eq!(body, "stable original");
}
