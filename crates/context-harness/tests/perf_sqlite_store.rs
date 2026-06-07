//! Ignored performance probes for the current SQLite-backed store.
//!
//! These tests are intentionally not part of normal CI. They generate a
//! synthetic corpus, exercise the real SQLite schema and `SqliteStore`, and
//! print timing data that can be compared against future storage backends.

use std::env;
use std::time::{Duration, Instant};

use context_harness::config::Config;
use context_harness::db;
use context_harness::migrate;
use context_harness::sqlite_store::SqliteStore;
use context_harness_core::embedding::vec_to_blob;
use context_harness_core::search::{self, SearchParams, SearchRequest};
use context_harness_core::store::Store;
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;

const DEFAULT_DOCS: usize = 1_000;
const DEFAULT_CHUNKS_PER_DOC: usize = 10;
const DEFAULT_DIMS: usize = 384;
const DEFAULT_REPEAT: usize = 5;

#[tokio::test]
#[ignore = "performance probe; run with `cargo test -p context-harness --test perf_sqlite_store -- --ignored --nocapture`"]
async fn perf_sqlite_keyword_vector_and_hybrid_search() {
    let docs = env_usize("CTX_PERF_DOCS", DEFAULT_DOCS);
    let chunks_per_doc = env_usize("CTX_PERF_CHUNKS_PER_DOC", DEFAULT_CHUNKS_PER_DOC);
    let dims = env_usize("CTX_PERF_DIMS", DEFAULT_DIMS);
    let repeat = env_usize("CTX_PERF_REPEAT", DEFAULT_REPEAT);
    let total_chunks = docs * chunks_per_doc;

    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::minimal();
    cfg.db.path = tmp.path().join("ctx.sqlite");
    cfg.retrieval.candidate_k_keyword = 80;
    cfg.retrieval.candidate_k_vector = 80;
    cfg.retrieval.final_limit = 12;

    migrate::run_migrations(&cfg).await.unwrap();
    let pool = db::connect(&cfg).await.unwrap();

    let populate_elapsed = timed(|| async {
        populate_synthetic_corpus(&pool, docs, chunks_per_doc, dims)
            .await
            .unwrap();
    })
    .await;

    let store = SqliteStore::new(pool.clone());
    let query_vec = synthetic_vector(chunk_seed(docs / 2, chunks_per_doc / 2), dims);
    let params = SearchParams {
        hybrid_alpha: 0.6,
        candidate_k_keyword: 80,
        candidate_k_vector: 80,
        final_limit: 12,
    };

    // Warm the OS and SQLite page cache before collecting timings.
    let _ = store
        .keyword_search("needle", 80, None, None)
        .await
        .unwrap();
    let _ = store
        .vector_search(&query_vec, 80, None, None)
        .await
        .unwrap();

    let keyword = measure_repeat(repeat, || async {
        store
            .keyword_search("needle", 80, None, None)
            .await
            .unwrap()
            .len()
    })
    .await;

    let vector = measure_repeat(repeat, || async {
        store
            .vector_search(&query_vec, 80, None, None)
            .await
            .unwrap()
            .len()
    })
    .await;

    let hybrid = measure_repeat(repeat, || async {
        let req = SearchRequest {
            query: "needle",
            query_vec: Some(&query_vec),
            mode: "hybrid",
            source_filter: None,
            since: None,
            params: params.clone(),
            explain: true,
        };
        search::search(&store, &req).await.unwrap().len()
    })
    .await;

    let db_size = std::fs::metadata(&cfg.db.path).map(|m| m.len()).unwrap_or(0);
    let vector_bytes = total_chunks * dims * std::mem::size_of::<f32>();
    let counts = corpus_counts(&pool).await.unwrap();

    println!();
    println!("sqlite_store_perf");
    println!("  docs:              {}", docs);
    println!("  chunks_per_doc:    {}", chunks_per_doc);
    println!("  chunks:            {}", total_chunks);
    println!("  dims:              {}", dims);
    println!("  repeat:            {}", repeat);
    println!("  db_size_bytes:     {}", db_size);
    println!("  vector_bytes_raw:  {}", vector_bytes);
    println!("  populate_ms:       {:.2}", ms(populate_elapsed));
    println!(
        "  counts:            documents={} chunks={} vectors={}",
        counts.documents, counts.chunks, counts.vectors
    );
    print_measurement("keyword_search", &keyword);
    print_measurement("vector_search", &vector);
    print_measurement("hybrid_search", &hybrid);
    println!();

    pool.close().await;
}

async fn populate_synthetic_corpus(
    pool: &SqlitePool,
    docs: usize,
    chunks_per_doc: usize,
    dims: usize,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    let now = chrono::Utc::now().timestamp();

    for doc_idx in 0..docs {
        let doc_id = format!("doc-{doc_idx:06}");
        let source_id = format!("synthetic/{doc_idx:06}.md");
        let title = format!("Synthetic Document {doc_idx}");
        let body = format!(
            "# {title}\n\nSynthetic corpus document {doc_idx}. This document helps benchmark retrieval."
        );

        sqlx::query(
            r#"
            INSERT INTO documents (
                id, source, source_id, source_url, title, author, created_at,
                updated_at, content_type, body, metadata_json, raw_json, dedup_hash
            )
            VALUES (?, 'perf:synthetic', ?, NULL, ?, 'perf', ?, ?, 'text/plain', ?, '{}', NULL, ?)
            "#,
        )
        .bind(&doc_id)
        .bind(&source_id)
        .bind(&title)
        .bind(now)
        .bind(now + doc_idx as i64)
        .bind(&body)
        .bind(format!("hash-{doc_idx:06}"))
        .execute(&mut *tx)
        .await?;

        for chunk_idx in 0..chunks_per_doc {
            let chunk_id = format!("{doc_id}-chunk-{chunk_idx:03}");
            let seed = chunk_seed(doc_idx, chunk_idx);
            let text = format!(
                "needle common term document {doc_idx} chunk {chunk_idx}. \
                 This benchmark text includes rust sqlite fts5 vector storage topic-{topic}. \
                 Unique marker marker-{doc_idx:06}-{chunk_idx:03}.",
                topic = doc_idx % 32,
            );

            sqlx::query(
                "INSERT INTO chunks (id, document_id, chunk_index, text, hash) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&chunk_id)
            .bind(&doc_id)
            .bind(chunk_idx as i64)
            .bind(&text)
            .bind(format!("chunk-hash-{doc_idx:06}-{chunk_idx:03}"))
            .execute(&mut *tx)
            .await?;

            sqlx::query("INSERT INTO chunks_fts (chunk_id, document_id, text) VALUES (?, ?, ?)")
                .bind(&chunk_id)
                .bind(&doc_id)
                .bind(&text)
                .execute(&mut *tx)
                .await?;

            let vector = synthetic_vector(seed, dims);
            let blob = vec_to_blob(&vector);
            sqlx::query(
                r#"
                INSERT INTO embeddings (chunk_id, model, dims, created_at, hash)
                VALUES (?, 'perf-model', ?, ?, ?)
                "#,
            )
            .bind(&chunk_id)
            .bind(dims as i64)
            .bind(now)
            .bind(format!("chunk-hash-{doc_idx:06}-{chunk_idx:03}"))
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                "INSERT INTO chunk_vectors (chunk_id, document_id, embedding) VALUES (?, ?, ?)",
            )
            .bind(&chunk_id)
            .bind(&doc_id)
            .bind(&blob)
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

fn synthetic_vector(seed: u64, dims: usize) -> Vec<f32> {
    (0..dims)
        .map(|i| {
            let mixed = seed
                .wrapping_mul(1_664_525)
                .wrapping_add((i as u64).wrapping_mul(1_013_904_223))
                .wrapping_add(17);
            ((mixed % 2_000) as f32 / 1_000.0) - 1.0
        })
        .collect()
}

fn chunk_seed(doc_idx: usize, chunk_idx: usize) -> u64 {
    ((doc_idx as u64) << 32) ^ chunk_idx as u64
}

struct Counts {
    documents: i64,
    chunks: i64,
    vectors: i64,
}

async fn corpus_counts(pool: &SqlitePool) -> anyhow::Result<Counts> {
    let row = sqlx::query(
        r#"
        SELECT
            (SELECT COUNT(*) FROM documents) AS documents,
            (SELECT COUNT(*) FROM chunks) AS chunks,
            (SELECT COUNT(*) FROM chunk_vectors) AS vectors
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(Counts {
        documents: row.get("documents"),
        chunks: row.get("chunks"),
        vectors: row.get("vectors"),
    })
}

struct Measurement {
    result_count: usize,
    durations: Vec<Duration>,
}

async fn measure_repeat<F, Fut>(repeat: usize, mut f: F) -> Measurement
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = usize>,
{
    let mut durations = Vec::with_capacity(repeat);
    let mut result_count = 0;

    for _ in 0..repeat {
        let start = Instant::now();
        result_count = f().await;
        durations.push(start.elapsed());
    }

    Measurement {
        result_count,
        durations,
    }
}

async fn timed<F, Fut>(f: F) -> Duration
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let start = Instant::now();
    f().await;
    start.elapsed()
}

fn print_measurement(label: &str, measurement: &Measurement) {
    let mut samples = measurement.durations.clone();
    samples.sort();
    let min = samples.first().copied().unwrap_or_default();
    let max = samples.last().copied().unwrap_or_default();
    let median = samples[samples.len() / 2];
    let total: Duration = samples.iter().copied().sum();
    let avg = total / samples.len() as u32;

    println!(
        "  {label:<18} results={:<4} min_ms={:>8.2} median_ms={:>8.2} avg_ms={:>8.2} max_ms={:>8.2}",
        measurement.result_count,
        ms(min),
        ms(median),
        ms(avg),
        ms(max)
    );
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
