//! Ignored performance probes for the current SQLite-backed store.
//!
//! These tests are intentionally not part of normal CI. They generate a
//! synthetic corpus, exercise the real SQLite schema and `SqliteStore`, and
//! print timing data that can be compared against future storage backends.

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use context_harness::config::Config;
use context_harness::db;
use context_harness::migrate;
use context_harness::sqlite_store::SqliteStore;
use context_harness_core::embedding::vec_to_blob;
use context_harness_core::search::{self, SearchParams, SearchRequest};
use context_harness_core::store::Store;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;

const DEFAULT_DOCS: usize = 1_000;
const DEFAULT_CHUNKS_PER_DOC: usize = 10;
const DEFAULT_DIMS: usize = 384;
const DEFAULT_REPEAT: usize = 5;
const DEFAULT_CANDIDATE_K: i64 = 80;

#[tokio::test]
#[ignore = "performance probe; run with `cargo test -p context-harness --test perf_sqlite_store -- --ignored --nocapture`"]
async fn perf_sqlite_keyword_vector_and_hybrid_search() {
    let scenario = Scenario {
        docs: env_usize("CTX_PERF_DOCS", DEFAULT_DOCS),
        chunks_per_doc: env_usize("CTX_PERF_CHUNKS_PER_DOC", DEFAULT_CHUNKS_PER_DOC),
        dims: env_usize("CTX_PERF_DIMS", DEFAULT_DIMS),
        repeat: env_usize("CTX_PERF_REPEAT", DEFAULT_REPEAT),
        candidate_k: env_i64("CTX_PERF_CANDIDATE_K", DEFAULT_CANDIDATE_K),
    };

    let report = run_sqlite_benchmark(scenario).await;
    print_report(&report);
}

#[tokio::test]
#[ignore = "scaling performance probe; set CTX_PERF_SCENARIOS, or use the small defaults"]
async fn perf_sqlite_scaling_profile() {
    let repeat = env_usize("CTX_PERF_REPEAT", DEFAULT_REPEAT);
    let candidate_k = env_i64("CTX_PERF_CANDIDATE_K", DEFAULT_CANDIDATE_K);
    let scenarios = env::var("CTX_PERF_SCENARIOS")
        .ok()
        .map(|value| parse_scenarios(&value, repeat, candidate_k))
        .unwrap_or_else(|| {
            vec![
                Scenario {
                    docs: 100,
                    chunks_per_doc: 5,
                    dims: 64,
                    repeat,
                    candidate_k,
                },
                Scenario {
                    docs: DEFAULT_DOCS,
                    chunks_per_doc: DEFAULT_CHUNKS_PER_DOC,
                    dims: DEFAULT_DIMS,
                    repeat,
                    candidate_k,
                },
            ]
        });

    for scenario in scenarios {
        let report = run_sqlite_benchmark(scenario).await;
        print_report(&report);
    }
}

async fn run_sqlite_benchmark(scenario: Scenario) -> BenchmarkReport {
    let total_chunks = scenario.docs * scenario.chunks_per_doc;
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::minimal();
    cfg.db.path = tmp.path().join("ctx.sqlite");
    cfg.retrieval.candidate_k_keyword = scenario.candidate_k;
    cfg.retrieval.candidate_k_vector = scenario.candidate_k;
    cfg.retrieval.final_limit = 12;

    migrate::run_migrations(&cfg).await.unwrap();
    let pool = db::connect(&cfg).await.unwrap();

    let populate_elapsed = timed(|| async {
        populate_synthetic_corpus(&pool, scenario.docs, scenario.chunks_per_doc, scenario.dims)
            .await
            .unwrap();
    })
    .await;

    let store = SqliteStore::new(pool.clone());
    let query_vec = synthetic_vector(
        chunk_seed(scenario.docs / 2, scenario.chunks_per_doc / 2),
        scenario.dims,
    );
    let params = SearchParams {
        hybrid_alpha: 0.6,
        candidate_k_keyword: scenario.candidate_k,
        candidate_k_vector: scenario.candidate_k,
        final_limit: 12,
    };

    // Warm the OS and SQLite page cache before collecting timings.
    let _ = store
        .keyword_search("needle", scenario.candidate_k, None, None)
        .await
        .unwrap();
    let _ = store
        .vector_search(&query_vec, scenario.candidate_k, None, None)
        .await
        .unwrap();

    let keyword = measure_repeat(scenario.repeat, || async {
        store
            .keyword_search("needle", scenario.candidate_k, None, None)
            .await
            .unwrap()
            .len()
    })
    .await;

    let vector = measure_repeat(scenario.repeat, || async {
        store
            .vector_search(&query_vec, scenario.candidate_k, None, None)
            .await
            .unwrap()
            .len()
    })
    .await;

    let hybrid = measure_repeat(scenario.repeat, || async {
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

    let storage_size = sqlite_storage_size(&cfg.db.path);
    let vector_bytes = total_chunks * scenario.dims * std::mem::size_of::<f32>();
    let counts = corpus_counts(&pool).await.unwrap();

    let report = BenchmarkReport {
        scenario,
        chunks: total_chunks,
        sqlite_file_bytes: storage_size.sqlite_file_bytes,
        sqlite_wal_bytes: storage_size.sqlite_wal_bytes,
        sqlite_shm_bytes: storage_size.sqlite_shm_bytes,
        sqlite_storage_bytes: storage_size.total_bytes(),
        vector_bytes_raw: vector_bytes,
        populate_ms: ms(populate_elapsed),
        documents_count: counts.documents,
        chunks_count: counts.chunks,
        vectors_count: counts.vectors,
        keyword_search: keyword.summary(),
        vector_search: vector.summary(),
        hybrid_search: hybrid.summary(),
    };

    pool.close().await;
    report
}

#[derive(Debug, Clone, Copy, Serialize)]
struct Scenario {
    docs: usize,
    chunks_per_doc: usize,
    dims: usize,
    repeat: usize,
    candidate_k: i64,
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{}x{} repeat={} k={}",
            self.docs, self.chunks_per_doc, self.dims, self.repeat, self.candidate_k
        )
    }
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    scenario: Scenario,
    chunks: usize,
    sqlite_file_bytes: u64,
    sqlite_wal_bytes: u64,
    sqlite_shm_bytes: u64,
    sqlite_storage_bytes: u64,
    vector_bytes_raw: usize,
    populate_ms: f64,
    documents_count: i64,
    chunks_count: i64,
    vectors_count: i64,
    keyword_search: MeasurementSummary,
    vector_search: MeasurementSummary,
    hybrid_search: MeasurementSummary,
}

fn print_report(report: &BenchmarkReport) {
    if env::var("CTX_PERF_OUTPUT").as_deref() == Ok("jsonl") {
        println!("{}", serde_json::to_string(report).unwrap());
        return;
    }

    println!();
    println!("sqlite_store_perf");
    println!("  scenario:          {}", report.scenario);
    println!("  docs:              {}", report.scenario.docs);
    println!("  chunks_per_doc:    {}", report.scenario.chunks_per_doc);
    println!("  chunks:            {}", report.chunks);
    println!("  dims:              {}", report.scenario.dims);
    println!("  repeat:            {}", report.scenario.repeat);
    println!("  candidate_k:       {}", report.scenario.candidate_k);
    println!("  sqlite_file_bytes: {}", report.sqlite_file_bytes);
    println!("  sqlite_wal_bytes:  {}", report.sqlite_wal_bytes);
    println!("  sqlite_shm_bytes:  {}", report.sqlite_shm_bytes);
    println!("  sqlite_total_bytes:{}", report.sqlite_storage_bytes);
    println!("  vector_bytes_raw:  {}", report.vector_bytes_raw);
    println!("  populate_ms:       {:.2}", report.populate_ms);
    println!(
        "  counts:            documents={} chunks={} vectors={}",
        report.documents_count, report.chunks_count, report.vectors_count
    );
    print_measurement("keyword_search", &report.keyword_search);
    print_measurement("vector_search", &report.vector_search);
    print_measurement("hybrid_search", &report.hybrid_search);
    println!();
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

#[derive(Debug, Serialize)]
struct MeasurementSummary {
    result_count: usize,
    min_ms: f64,
    median_ms: f64,
    avg_ms: f64,
    max_ms: f64,
}

impl Measurement {
    fn summary(&self) -> MeasurementSummary {
        let mut samples = self.durations.clone();
        samples.sort();
        let min = samples.first().copied().unwrap_or_default();
        let max = samples.last().copied().unwrap_or_default();
        let median = samples
            .get(samples.len().saturating_sub(1) / 2)
            .copied()
            .unwrap_or_default();
        let total: Duration = samples.iter().copied().sum();
        let avg = if samples.is_empty() {
            Duration::default()
        } else {
            total / samples.len() as u32
        };

        MeasurementSummary {
            result_count: self.result_count,
            min_ms: ms(min),
            median_ms: ms(median),
            avg_ms: ms(avg),
            max_ms: ms(max),
        }
    }
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

fn print_measurement(label: &str, measurement: &MeasurementSummary) {
    println!(
        "  {label:<18} results={:<4} min_ms={:>8.2} median_ms={:>8.2} avg_ms={:>8.2} max_ms={:>8.2}",
        measurement.result_count,
        measurement.min_ms,
        measurement.median_ms,
        measurement.avg_ms,
        measurement.max_ms
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

fn env_i64(name: &str, default: i64) -> i64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

struct SqliteStorageSize {
    sqlite_file_bytes: u64,
    sqlite_wal_bytes: u64,
    sqlite_shm_bytes: u64,
}

impl SqliteStorageSize {
    fn total_bytes(&self) -> u64 {
        self.sqlite_file_bytes + self.sqlite_wal_bytes + self.sqlite_shm_bytes
    }
}

fn sqlite_storage_size(path: &Path) -> SqliteStorageSize {
    SqliteStorageSize {
        sqlite_file_bytes: file_size(path),
        sqlite_wal_bytes: file_size(&sqlite_sidecar_path(path, "wal")),
        sqlite_shm_bytes: file_size(&sqlite_sidecar_path(path, "shm")),
    }
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!("-{suffix}"));
    PathBuf::from(name)
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn parse_scenarios(value: &str, repeat: usize, candidate_k: i64) -> Vec<Scenario> {
    value
        .split(',')
        .filter_map(|part| {
            let mut fields = part.split('x');
            let docs = fields.next()?.trim().parse().ok()?;
            let chunks_per_doc = fields.next()?.trim().parse().ok()?;
            let dims = fields.next()?.trim().parse().ok()?;
            Some(Scenario {
                docs,
                chunks_per_doc,
                dims,
                repeat,
                candidate_k,
            })
        })
        .collect()
}
