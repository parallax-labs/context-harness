//! Ignored zvec bake-off probe.
//!
//! This test is intentionally feature-gated and ignored. It compares the
//! current exact SQLite vector scan against a zvec sidecar index using the same
//! synthetic corpus shape as the SQLite performance probe.

#[cfg(feature = "zvec-bundled")]
mod zvec_bench {
    use std::collections::HashSet;
    use std::env;
    use std::fmt;
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use context_harness::config::Config;
    use context_harness::db;
    use context_harness::migrate;
    use context_harness::sqlite_store::SqliteStore;
    use context_harness_core::embedding::{blob_to_vec, vec_to_blob};
    use context_harness_core::store::{ChunkCandidate, Store};
    use serde::Serialize;
    use sqlx::{Row, SqlitePool};
    use tempfile::TempDir;
    use zvec::{Collection, CollectionSchema, Doc, FieldSchema, MetricType, VectorQuery};

    const DEFAULT_DOCS: usize = 1_000;
    const DEFAULT_CHUNKS_PER_DOC: usize = 10;
    const DEFAULT_DIMS: usize = 384;
    const DEFAULT_REPEAT: usize = 5;
    const DEFAULT_CANDIDATE_K: i64 = 80;
    const ZVEC_BATCH_SIZE: usize = 512;

    #[tokio::test]
    #[ignore = "zvec bake-off; run with `cargo test -p context-harness --features zvec-bundled --test perf_zvec_vector_index -- --ignored --nocapture`"]
    async fn perf_zvec_vector_index_bakeoff() {
        let scenario = Scenario {
            docs: env_usize("CTX_PERF_DOCS", DEFAULT_DOCS),
            chunks_per_doc: env_usize("CTX_PERF_CHUNKS_PER_DOC", DEFAULT_CHUNKS_PER_DOC),
            dims: env_usize("CTX_PERF_DIMS", DEFAULT_DIMS),
            repeat: env_usize("CTX_PERF_REPEAT", DEFAULT_REPEAT),
            candidate_k: env_i64("CTX_PERF_CANDIDATE_K", DEFAULT_CANDIDATE_K),
        };

        let report = run_zvec_bakeoff(scenario).await;
        print_report(&report);
    }

    async fn run_zvec_bakeoff(scenario: Scenario) -> ZvecBakeoffReport {
        let tmp = TempDir::new().unwrap();
        let sqlite_path = tmp.path().join("ctx.sqlite");
        let zvec_path = tmp.path().join("vector-index");
        let mut cfg = Config::minimal();
        cfg.db.path = sqlite_path;

        migrate::run_migrations(&cfg).await.unwrap();
        let pool = db::connect(&cfg).await.unwrap();

        let populate_sqlite_ms = ms(timed(|| async {
            populate_sqlite(&pool, scenario.docs, scenario.chunks_per_doc, scenario.dims)
                .await
                .unwrap();
        })
        .await);

        let collection = create_collection(&zvec_path, scenario.dims);
        let build_ms = ms(timed(|| async {
            populate_zvec_from_sqlite(&pool, &collection).await.unwrap();
        })
        .await);
        let optimize_ms = ms(timed(|| async {
            collection.optimize().unwrap();
            collection.flush().unwrap();
        })
        .await);

        let store = SqliteStore::new(pool.clone());
        let query_vec = synthetic_vector(
            chunk_seed(scenario.docs / 2, scenario.chunks_per_doc / 2),
            scenario.dims,
        );

        let _ = store
            .vector_search(&query_vec, scenario.candidate_k, None, None)
            .await
            .unwrap();
        let _ = zvec_search(&collection, &query_vec, scenario.candidate_k).unwrap();

        let sqlite = measure_repeat(scenario.repeat, || async {
            store
                .vector_search(&query_vec, scenario.candidate_k, None, None)
                .await
                .unwrap()
        })
        .await;

        let zvec = measure_repeat(scenario.repeat, || async {
            zvec_search(&collection, &query_vec, scenario.candidate_k).unwrap()
        })
        .await;

        let sqlite_top = store
            .vector_search(&query_vec, scenario.candidate_k, None, None)
            .await
            .unwrap();
        let zvec_top = zvec_search(&collection, &query_vec, scenario.candidate_k).unwrap();
        let topk_overlap = topk_overlap(&sqlite_top, &zvec_top);

        let report = ZvecBakeoffReport {
            scenario,
            chunks: scenario.docs * scenario.chunks_per_doc,
            populate_sqlite_ms,
            zvec_build_ms: build_ms,
            zvec_optimize_ms: optimize_ms,
            zvec_storage_bytes: dir_size(&zvec_path),
            sqlite_vector_search: sqlite.summary(),
            zvec_vector_search: zvec.summary(),
            topk_overlap,
        };

        pool.close().await;
        report
    }

    fn create_collection(path: &Path, dims: usize) -> Collection {
        let schema = CollectionSchema::builder("context_chunks")
            .field(FieldSchema::string("document_id").invert_index(true, false))
            .field(FieldSchema::string("snippet"))
            .field(
                FieldSchema::vector_fp32("embedding", dims as u32)
                    .hnsw(16, 200)
                    .metric(MetricType::Cosine),
            )
            .build()
            .unwrap();

        Collection::create_and_open(path.to_str().unwrap(), &schema, None).unwrap()
    }

    async fn populate_zvec_from_sqlite(
        pool: &SqlitePool,
        collection: &Collection,
    ) -> anyhow::Result<()> {
        let rows = sqlx::query(
            r#"
            SELECT cv.chunk_id, cv.document_id, cv.embedding,
                   COALESCE(substr(c.text, 1, 240), '') AS snippet
            FROM chunk_vectors cv
            JOIN chunks c ON c.id = cv.chunk_id
            ORDER BY cv.document_id, c.chunk_index
            "#,
        )
        .fetch_all(pool)
        .await?;

        let mut batch = Vec::with_capacity(ZVEC_BATCH_SIZE);
        for row in rows {
            let chunk_id: String = row.get("chunk_id");
            let document_id: String = row.get("document_id");
            let blob: Vec<u8> = row.get("embedding");
            let snippet: String = row.get("snippet");
            let vector = blob_to_vec(&blob);

            let mut doc = Doc::new().unwrap();
            doc.set_pk(&chunk_id).unwrap();
            doc.add_string("document_id", &document_id).unwrap();
            doc.add_string("snippet", &snippet).unwrap();
            doc.add_vector_fp32("embedding", &vector).unwrap();
            batch.push(doc);

            if batch.len() == ZVEC_BATCH_SIZE {
                insert_batch(collection, &batch);
                batch.clear();
            }
        }

        if !batch.is_empty() {
            insert_batch(collection, &batch);
        }

        collection.flush().unwrap();
        Ok(())
    }

    fn insert_batch(collection: &Collection, docs: &[Doc]) {
        let refs: Vec<&Doc> = docs.iter().collect();
        collection.upsert(&refs).unwrap();
    }

    fn zvec_search(
        collection: &Collection,
        query_vec: &[f32],
        limit: i64,
    ) -> anyhow::Result<Vec<ChunkCandidate>> {
        let query = VectorQuery::builder()
            .field("embedding")
            .vector_fp32(query_vec)
            .topk(limit as i32)
            .build()?;

        let rows = collection.query(&query)?;
        let mut candidates = Vec::new();
        for row in rows.iter() {
            let Some(chunk_id) = row.pk_copy() else {
                continue;
            };
            candidates.push(ChunkCandidate {
                chunk_id,
                document_id: row.get_string("document_id")?.unwrap_or_default(),
                raw_score: row.score() as f64,
                snippet: row.get_string("snippet")?.unwrap_or_default(),
            });
        }
        Ok(candidates)
    }

    async fn populate_sqlite(
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

                sqlx::query(
                    "INSERT INTO chunks_fts (chunk_id, document_id, text) VALUES (?, ?, ?)",
                )
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
    struct ZvecBakeoffReport {
        scenario: Scenario,
        chunks: usize,
        populate_sqlite_ms: f64,
        zvec_build_ms: f64,
        zvec_optimize_ms: f64,
        zvec_storage_bytes: u64,
        sqlite_vector_search: MeasurementSummary,
        zvec_vector_search: MeasurementSummary,
        topk_overlap: f64,
    }

    fn print_report(report: &ZvecBakeoffReport) {
        if env::var("CTX_PERF_OUTPUT").as_deref() == Ok("jsonl") {
            println!("{}", serde_json::to_string(report).unwrap());
            return;
        }

        println!();
        println!("zvec_vector_index_bakeoff");
        println!("  scenario:          {}", report.scenario);
        println!("  chunks:            {}", report.chunks);
        println!("  populate_sqlite_ms:{:.2}", report.populate_sqlite_ms);
        println!("  zvec_build_ms:     {:.2}", report.zvec_build_ms);
        println!("  zvec_optimize_ms:  {:.2}", report.zvec_optimize_ms);
        println!("  zvec_storage_bytes:{}", report.zvec_storage_bytes);
        println!("  topk_overlap:      {:.3}", report.topk_overlap);
        print_measurement("sqlite_vector", &report.sqlite_vector_search);
        print_measurement("zvec_vector", &report.zvec_vector_search);
        println!();
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
        Fut: std::future::Future<Output = Vec<ChunkCandidate>>,
    {
        let mut durations = Vec::with_capacity(repeat);
        let mut result_count = 0;

        for _ in 0..repeat {
            let start = Instant::now();
            let candidates = f().await;
            durations.push(start.elapsed());
            result_count = candidates.len();
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

    fn topk_overlap(left: &[ChunkCandidate], right: &[ChunkCandidate]) -> f64 {
        if left.is_empty() {
            return 1.0;
        }

        let right_ids: HashSet<&str> = right
            .iter()
            .map(|candidate| candidate.chunk_id.as_str())
            .collect();
        let overlap = left
            .iter()
            .filter(|candidate| right_ids.contains(candidate.chunk_id.as_str()))
            .count();
        overlap as f64 / left.len() as f64
    }

    fn dir_size(path: &Path) -> u64 {
        let Ok(entries) = fs::read_dir(path) else {
            return 0;
        };

        entries
            .filter_map(Result::ok)
            .map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    dir_size(&path)
                } else {
                    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
                }
            })
            .sum()
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
}

#[cfg(not(feature = "zvec-bundled"))]
#[test]
#[ignore = "enable `--features zvec-bundled` to compile and run the zvec bake-off"]
fn perf_zvec_vector_index_bakeoff_requires_feature() {
    eprintln!(
        "Run with: cargo test -p context-harness --features zvec-bundled --test perf_zvec_vector_index -- --ignored --nocapture"
    );
}
