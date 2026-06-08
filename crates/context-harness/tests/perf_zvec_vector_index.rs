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
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use context_harness::chunk::chunk_text;
    use context_harness::config::Config;
    use context_harness::db;
    use context_harness::migrate;
    use context_harness::sqlite_store::SqliteStore;
    use context_harness_core::embedding::{blob_to_vec, vec_to_blob};
    use context_harness_core::store::{ChunkCandidate, Store};
    use serde::Serialize;
    use sha2::{Digest, Sha256};
    use sqlx::{Row, SqlitePool};
    use tempfile::TempDir;
    use walkdir::{DirEntry, WalkDir};
    use zvec::{Collection, CollectionSchema, Doc, FieldSchema, VectorQuery, VectorSchema};

    const DEFAULT_DOCS: usize = 1_000;
    const DEFAULT_CHUNKS_PER_DOC: usize = 10;
    const DEFAULT_DIMS: usize = 384;
    const DEFAULT_REPEAT: usize = 5;
    const DEFAULT_CANDIDATE_K: i64 = 80;
    const DEFAULT_MAX_TOKENS: usize = 700;
    const DEFAULT_CORPUS_ROOT: &str = ".";
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

    #[tokio::test]
    #[ignore = "real-corpus zvec bake-off; set CTX_PERF_CORPUS_ROOT to the corpus path"]
    async fn perf_zvec_vector_index_real_corpus() {
        let scenario = CorpusScenario {
            root: PathBuf::from(
                env::var("CTX_PERF_CORPUS_ROOT")
                    .unwrap_or_else(|_| DEFAULT_CORPUS_ROOT.to_string()),
            ),
            dims: env_usize("CTX_PERF_DIMS", DEFAULT_DIMS),
            repeat: env_usize("CTX_PERF_REPEAT", DEFAULT_REPEAT),
            candidate_k: env_i64("CTX_PERF_CANDIDATE_K", DEFAULT_CANDIDATE_K),
            max_tokens: env_usize("CTX_PERF_MAX_TOKENS", DEFAULT_MAX_TOKENS),
            max_files: env::var("CTX_PERF_MAX_FILES")
                .ok()
                .and_then(|value| value.parse().ok()),
        };

        let report = run_real_corpus_bakeoff(scenario).await;
        print_corpus_report(&report);
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
        let mut zvec_metadata = Vec::new();
        let build_ms = ms(timed(|| async {
            zvec_metadata = populate_zvec_from_sqlite(&pool, &collection).await.unwrap();
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
        let _ = zvec_search(
            &collection,
            &zvec_metadata,
            &query_vec,
            scenario.candidate_k,
        )
        .unwrap();

        let sqlite = measure_repeat(scenario.repeat, || async {
            store
                .vector_search(&query_vec, scenario.candidate_k, None, None)
                .await
                .unwrap()
        })
        .await;

        let zvec = measure_repeat(scenario.repeat, || async {
            zvec_search(
                &collection,
                &zvec_metadata,
                &query_vec,
                scenario.candidate_k,
            )
            .unwrap()
        })
        .await;

        let sqlite_top = store
            .vector_search(&query_vec, scenario.candidate_k, None, None)
            .await
            .unwrap();
        let zvec_top = zvec_search(
            &collection,
            &zvec_metadata,
            &query_vec,
            scenario.candidate_k,
        )
        .unwrap();
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

    async fn run_real_corpus_bakeoff(scenario: CorpusScenario) -> CorpusBakeoffReport {
        let tmp = TempDir::new().unwrap();
        let sqlite_path = tmp.path().join("ctx.sqlite");
        let zvec_path = tmp.path().join("vector-index");
        let mut cfg = Config::minimal();
        cfg.db.path = sqlite_path;
        cfg.chunking.max_tokens = scenario.max_tokens;

        migrate::run_migrations(&cfg).await.unwrap();
        let pool = db::connect(&cfg).await.unwrap();

        let mut corpus_shape = CorpusShape::default();
        let populate_sqlite_ms = ms(timed(|| async {
            corpus_shape = populate_real_corpus_sqlite(&pool, &scenario).await.unwrap();
        })
        .await);

        let collection = create_collection(&zvec_path, scenario.dims);
        let mut zvec_metadata = Vec::new();
        let build_ms = ms(timed(|| async {
            zvec_metadata = populate_zvec_from_sqlite(&pool, &collection).await.unwrap();
        })
        .await);
        let optimize_ms = ms(timed(|| async {
            collection.optimize().unwrap();
            collection.flush().unwrap();
        })
        .await);

        let store = SqliteStore::new(pool.clone());
        let query_vec = middle_query_vector(&pool).await.unwrap();

        let _ = store
            .vector_search(&query_vec, scenario.candidate_k, None, None)
            .await
            .unwrap();
        let _ = zvec_search(
            &collection,
            &zvec_metadata,
            &query_vec,
            scenario.candidate_k,
        )
        .unwrap();

        let sqlite = measure_repeat(scenario.repeat, || async {
            store
                .vector_search(&query_vec, scenario.candidate_k, None, None)
                .await
                .unwrap()
        })
        .await;

        let zvec = measure_repeat(scenario.repeat, || async {
            zvec_search(
                &collection,
                &zvec_metadata,
                &query_vec,
                scenario.candidate_k,
            )
            .unwrap()
        })
        .await;

        let sqlite_top = store
            .vector_search(&query_vec, scenario.candidate_k, None, None)
            .await
            .unwrap();
        let zvec_top = zvec_search(
            &collection,
            &zvec_metadata,
            &query_vec,
            scenario.candidate_k,
        )
        .unwrap();
        let topk_overlap = topk_overlap(&sqlite_top, &zvec_top);

        let report = CorpusBakeoffReport {
            scenario,
            corpus_shape,
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
        let mut schema = CollectionSchema::new("context_chunks");
        schema.add_field(FieldSchema::string("chunk_id")).unwrap();
        schema
            .add_field(FieldSchema::string("document_id"))
            .unwrap();
        schema.add_field(FieldSchema::string("snippet")).unwrap();
        schema
            .add_field(VectorSchema::fp32("embedding", dims as u32))
            .unwrap();

        Collection::create_and_open(path, schema).unwrap()
    }

    async fn populate_zvec_from_sqlite(
        pool: &SqlitePool,
        collection: &Collection,
    ) -> anyhow::Result<Vec<ChunkCandidate>> {
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
        let mut metadata = Vec::with_capacity(rows.len());
        for row in rows {
            let chunk_id: String = row.get("chunk_id");
            let document_id: String = row.get("document_id");
            let blob: Vec<u8> = row.get("embedding");
            let snippet: String = row.get("snippet");
            let vector = blob_to_vec(&blob);

            metadata.push(ChunkCandidate {
                chunk_id: chunk_id.clone(),
                document_id: document_id.clone(),
                raw_score: 0.0,
                snippet: snippet.clone(),
            });

            let mut doc = Doc::new();
            doc.set_pk(&chunk_id).unwrap();
            doc.set_string("chunk_id", &chunk_id).unwrap();
            doc.set_string("document_id", &document_id).unwrap();
            doc.set_string("snippet", &snippet).unwrap();
            doc.set_vector("embedding", &vector).unwrap();
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
        Ok(metadata)
    }

    fn insert_batch(collection: &Collection, docs: &[Doc]) {
        collection.insert(docs).unwrap();
    }

    fn zvec_search(
        collection: &Collection,
        metadata: &[ChunkCandidate],
        query_vec: &[f32],
        limit: i64,
    ) -> anyhow::Result<Vec<ChunkCandidate>> {
        let query = VectorQuery::new("embedding")
            .topk(limit.max(0) as usize)
            .include_doc_id(true)
            .vector(query_vec)?;

        let rows = collection.query(query)?;
        let mut candidates = Vec::new();
        for row in rows.iter() {
            let Some(record) = metadata.get(row.doc_id() as usize) else {
                continue;
            };
            candidates.push(ChunkCandidate {
                chunk_id: record.chunk_id.clone(),
                document_id: record.document_id.clone(),
                raw_score: row.score() as f64,
                snippet: record.snippet.clone(),
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

    async fn populate_real_corpus_sqlite(
        pool: &SqlitePool,
        scenario: &CorpusScenario,
    ) -> anyhow::Result<CorpusShape> {
        let mut tx = pool.begin().await?;
        let now = chrono::Utc::now().timestamp();
        let mut shape = CorpusShape::default();

        for entry in corpus_entries(&scenario.root) {
            if scenario
                .max_files
                .is_some_and(|max_files| shape.documents >= max_files)
            {
                break;
            }

            let path = entry.path();
            if !is_text_like(path) {
                shape.skipped_files += 1;
                continue;
            }

            let Ok(body) = fs::read_to_string(path) else {
                shape.skipped_files += 1;
                continue;
            };

            if body.trim().is_empty() {
                shape.skipped_files += 1;
                continue;
            }

            let rel_path = path
                .strip_prefix(&scenario.root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            let doc_idx = shape.documents;
            let doc_id = format!("doc-{doc_idx:06}");
            let title = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&rel_path)
                .to_string();
            let dedup_hash = hash_hex(body.as_bytes());

            sqlx::query(
                r#"
                INSERT INTO documents (
                    id, source, source_id, source_url, title, author, created_at,
                    updated_at, content_type, body, metadata_json, raw_json, dedup_hash
                )
                VALUES (?, 'perf:real-corpus', ?, NULL, ?, 'perf', ?, ?, 'text/plain', ?, '{}', NULL, ?)
                "#,
            )
            .bind(&doc_id)
            .bind(&rel_path)
            .bind(&title)
            .bind(now)
            .bind(now + doc_idx as i64)
            .bind(&body)
            .bind(&dedup_hash)
            .execute(&mut *tx)
            .await?;

            let chunks = chunk_text(&doc_id, &body, scenario.max_tokens);
            for chunk in chunks {
                sqlx::query(
                    "INSERT INTO chunks (id, document_id, chunk_index, text, hash) VALUES (?, ?, ?, ?, ?)",
                )
                .bind(&chunk.id)
                .bind(&chunk.document_id)
                .bind(chunk.chunk_index)
                .bind(&chunk.text)
                .bind(&chunk.hash)
                .execute(&mut *tx)
                .await?;

                sqlx::query(
                    "INSERT INTO chunks_fts (chunk_id, document_id, text) VALUES (?, ?, ?)",
                )
                .bind(&chunk.id)
                .bind(&chunk.document_id)
                .bind(&chunk.text)
                .execute(&mut *tx)
                .await?;

                let vector = synthetic_vector(text_seed(&chunk.text), scenario.dims);
                let blob = vec_to_blob(&vector);
                sqlx::query(
                    r#"
                    INSERT INTO embeddings (chunk_id, model, dims, created_at, hash)
                    VALUES (?, 'perf-model', ?, ?, ?)
                    "#,
                )
                .bind(&chunk.id)
                .bind(scenario.dims as i64)
                .bind(now)
                .bind(&chunk.hash)
                .execute(&mut *tx)
                .await?;

                sqlx::query(
                    "INSERT INTO chunk_vectors (chunk_id, document_id, embedding) VALUES (?, ?, ?)",
                )
                .bind(&chunk.id)
                .bind(&chunk.document_id)
                .bind(&blob)
                .execute(&mut *tx)
                .await?;

                shape.chunks += 1;
            }

            shape.documents += 1;
            shape.bytes += body.len() as u64;
        }

        tx.commit().await?;
        Ok(shape)
    }

    async fn middle_query_vector(pool: &SqlitePool) -> anyhow::Result<Vec<f32>> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_vectors")
            .fetch_one(pool)
            .await?;
        anyhow::ensure!(count > 0, "corpus produced no vectors");

        let blob: Vec<u8> = sqlx::query_scalar(
            "SELECT embedding FROM chunk_vectors ORDER BY chunk_id LIMIT 1 OFFSET ?",
        )
        .bind(count / 2)
        .fetch_one(pool)
        .await?;

        Ok(blob_to_vec(&blob))
    }

    fn corpus_entries(root: &Path) -> Vec<DirEntry> {
        let mut entries: Vec<DirEntry> = WalkDir::new(root)
            .into_iter()
            .filter_entry(|entry| !is_excluded_entry(entry))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .collect();
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        entries
    }

    fn is_excluded_entry(entry: &DirEntry) -> bool {
        let name = entry.file_name().to_string_lossy();
        matches!(
            name.as_ref(),
            ".git"
                | ".hg"
                | ".svn"
                | "target"
                | "node_modules"
                | ".next"
                | ".turbo"
                | "dist"
                | "build"
                | "vendor"
                | ".venv"
                | "__pycache__"
        )
    }

    fn is_text_like(path: &Path) -> bool {
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            return false;
        };
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "adoc"
                | "c"
                | "cfg"
                | "clj"
                | "conf"
                | "cpp"
                | "cs"
                | "css"
                | "go"
                | "h"
                | "hpp"
                | "html"
                | "java"
                | "js"
                | "json"
                | "jsx"
                | "kt"
                | "lua"
                | "md"
                | "py"
                | "rb"
                | "rs"
                | "scala"
                | "sh"
                | "sql"
                | "swift"
                | "toml"
                | "ts"
                | "tsx"
                | "txt"
                | "xml"
                | "yaml"
                | "yml"
        )
    }

    fn text_seed(text: &str) -> u64 {
        let digest = Sha256::digest(text.as_bytes());
        u64::from_le_bytes(digest[0..8].try_into().unwrap())
    }

    fn hash_hex(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        format!("{digest:x}")
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

    #[derive(Debug, Clone, Serialize)]
    struct CorpusScenario {
        root: PathBuf,
        dims: usize,
        repeat: usize,
        candidate_k: i64,
        max_tokens: usize,
        max_files: Option<usize>,
    }

    #[derive(Debug, Clone, Default, Serialize)]
    struct CorpusShape {
        documents: usize,
        chunks: usize,
        bytes: u64,
        skipped_files: usize,
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

    #[derive(Debug, Serialize)]
    struct CorpusBakeoffReport {
        scenario: CorpusScenario,
        corpus_shape: CorpusShape,
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

    fn print_corpus_report(report: &CorpusBakeoffReport) {
        if env::var("CTX_PERF_OUTPUT").as_deref() == Ok("jsonl") {
            println!("{}", serde_json::to_string(report).unwrap());
            return;
        }

        println!();
        println!("zvec_vector_index_real_corpus");
        println!("  root:              {}", report.scenario.root.display());
        println!("  docs:              {}", report.corpus_shape.documents);
        println!("  chunks:            {}", report.corpus_shape.chunks);
        println!("  bytes:             {}", report.corpus_shape.bytes);
        println!("  skipped_files:     {}", report.corpus_shape.skipped_files);
        println!("  dims:              {}", report.scenario.dims);
        println!("  repeat:            {}", report.scenario.repeat);
        println!("  candidate_k:       {}", report.scenario.candidate_k);
        println!("  max_tokens:        {}", report.scenario.max_tokens);
        println!("  max_files:         {:?}", report.scenario.max_files);
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
