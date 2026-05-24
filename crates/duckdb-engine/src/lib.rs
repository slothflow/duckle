//! Duckle DuckDB engine adapter - CLI-driven.
//!
//! Rather than statically linking libduckdb (which bloats the binary to
//! tens of MB and makes builds glacial), this drives the official DuckDB
//! **CLI** that Duckle downloads into the app-data dir on first launch.
//! The engine shells out to `duckdb -json -c "<sql>"` and parses the
//! JSON it prints. SQL generation lives in `plan.rs` and is unchanged;
//! only execution + inspection talk to the CLI here.
//!
//! Execution model: a temp on-disk `.duckdb` file. Each non-sink stage
//! materializes a `CREATE OR REPLACE TABLE` (so it persists across the
//! separate CLI invocations); sinks `COPY` from the upstream table.
//! Cancellation kills the in-flight child process.

use duckle_metadata::{Column, DataType};
use duckle_plugin_sdk::{Inspection, InspectError};
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;

pub mod history;
pub mod plan;
pub use history::{append_run_record, load_run_history, RunRecord};
pub use plan::{CompiledPipeline, PipelineDoc, Stage, StageKind};
use plan::WebhookSpec;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("config: {0}")]
    Config(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("query: {0}")]
    Query(String),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl From<EngineError> for InspectError {
    fn from(err: EngineError) -> Self {
        match err {
            EngineError::Config(m) => InspectError::Config(m),
            EngineError::Unsupported(m) => InspectError::Unsupported(m),
            other => InspectError::Other(other.to_string()),
        }
    }
}

/// Rows sampled alongside the schema for the Preview tab.
const PREVIEW_LIMIT: usize = 8;
/// Rows captured per stage during a run (shown in the node Preview tab).
const PREVIEW_ROW_LIMIT: usize = 100;

/// Drives the downloaded DuckDB CLI. Cheap to clone; holds only the
/// binary path and a shared cancel flag.
#[derive(Clone)]
pub struct DuckdbEngine {
    bin: PathBuf,
    cancel: Arc<AtomicBool>,
}

impl std::fmt::Debug for DuckdbEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DuckdbEngine")
            .field("bin", &self.bin)
            .finish()
    }
}

impl DuckdbEngine {
    /// Construct an engine pointing at a DuckDB CLI binary. The binary
    /// need not exist yet - calls fail with a clear error if it's
    /// missing, and the first-run setup installs it.
    pub fn new(bin: PathBuf) -> Self {
        Self {
            bin,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn binary(&self) -> &Path {
        &self.bin
    }

    pub fn is_available(&self) -> bool {
        self.bin.exists()
    }

    /// Signal any in-flight run to stop. The polling loop in `run` sees
    /// the flag and kills the active CLI child, so even a long query
    /// returns promptly.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn clear_cancel(&self) {
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// Run SQL through the CLI against an optional db file. Returns raw
    /// stdout. Cancellation-aware: polls the child and kills it if a
    /// cancel was requested.
    fn run(&self, db: Option<&Path>, sql: &str, json: bool) -> Result<String, EngineError> {
        if !self.bin.exists() {
            return Err(EngineError::Config(format!(
                "DuckDB engine isn't installed (expected at {}). Open Setup to install it.",
                self.bin.display()
            )));
        }
        let mut cmd = std::process::Command::new(&self.bin);
        match db {
            Some(p) => {
                cmd.arg(p);
            }
            None => {
                cmd.arg(":memory:");
            }
        }
        if json {
            cmd.arg("-json");
        }
        cmd.arg("-bail").arg("-c").arg(sql);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // No console flash on Windows for the per-stage spawns.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| EngineError::Other(format!("could not start duckdb: {}", e)))?;

        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if self.cancel.load(Ordering::Relaxed) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(EngineError::Cancelled);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(40));
                }
                Err(e) => return Err(EngineError::Other(e.to_string())),
            }
        }

        let out = child
            .wait_with_output()
            .map_err(|e| EngineError::Other(e.to_string()))?;
        if !out.status.success() {
            let mut msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if msg.is_empty() {
                msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
            if msg.is_empty() {
                msg = "DuckDB CLI exited with an error".into();
            }
            return Err(EngineError::Query(msg));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run SQL and return the first JSON array of rows it printed
    /// (DESCRIBE / SELECT produce one array; preludes produce none).
    fn run_rows(&self, db: Option<&Path>, sql: &str) -> Result<Vec<JsonValue>, EngineError> {
        let out = self.run(db, sql, true)?;
        Ok(parse_json_arrays(&out).into_iter().next().unwrap_or_default())
    }

    // ---- Inspection ----------------------------------------------------

    /// Inspect a source for its schema and a small preview. `format` is
    /// the string the frontend ships (`"csv"`, `"parquet"`, `"s3"`, ...).
    pub fn inspect(&self, format: &str, options: JsonValue) -> Result<Inspection, EngineError> {
        let select = plan::source_select_for_format(format, &options).ok_or_else(|| {
            EngineError::Unsupported(format!("Format '{}' is not supported", format))
        })?;
        let prelude = self.source_prelude(format, &options);

        let describe_sql = format!("{}DESCRIBE {};", prelude, select);
        let cols = self.run_rows(None, &describe_sql)?;
        let schema: Vec<Column> = cols.iter().filter_map(parse_describe_row).collect();

        let sample_sql = format!("{}{} LIMIT {};", prelude, select, PREVIEW_LIMIT);
        let rows = self.run_rows(None, &sample_sql).unwrap_or_default();

        Ok(Inspection {
            schema,
            sample_rows: rows,
        })
    }

    /// Statements that must run before a source query: cloud credentials,
    /// the azure extension, or ATTACH for a DuckDB file.
    fn source_prelude(&self, format: &str, options: &JsonValue) -> String {
        let mut p = String::new();
        if let Some(secret) = secret_statement(format, "duckle_inspect", options) {
            p.push_str(&secret);
            p.push(' ');
        }
        if format == "azureblob" {
            p.push_str("INSTALL azure; LOAD azure; ");
        }
        if format == "duckdb" {
            if let Some(db) = options.get("database").and_then(JsonValue::as_str) {
                p.push_str(&format!(
                    "ATTACH '{}' AS duckle_src (READ_ONLY); ",
                    sql_escape(db)
                ));
            }
        }
        p
    }

    // ---- Execution -----------------------------------------------------

    pub fn execute_pipeline(&self, doc: &PipelineDoc) -> RunResult {
        self.execute_pipeline_with_events(doc, None::<&str>, |_| {})
    }

    /// Execute a pipeline, optionally only the subgraph upstream of
    /// `target`, streaming [`PipelineEvent`]s through `on_event`.
    pub fn execute_pipeline_with_events<F>(
        &self,
        doc: &PipelineDoc,
        target: Option<&str>,
        mut on_event: F,
    ) -> RunResult
    where
        F: FnMut(PipelineEvent),
    {
        let total_start = Instant::now();
        self.clear_cancel();

        if !self.bin.exists() {
            return RunResult::failed(
                total_start,
                "DuckDB engine isn't installed yet. Open Setup to install it.".into(),
            );
        }

        let compiled = match target {
            Some(t) => plan::compile_partial(doc, t),
            None => plan::compile(doc),
        };
        let compiled = match compiled {
            Ok(c) => c,
            Err(e) => return RunResult::failed(total_start, e.to_string()),
        };

        on_event(PipelineEvent::Started {
            total_stages: compiled.stages.len() as u32,
        });

        // Temp on-disk DB for this run. The atomic counter guarantees a
        // unique path even when several runs start in the same process at
        // the same clock tick (parallel tests, or concurrent scheduled
        // runs), which would otherwise collide and fight over the file.
        let db_path = std::env::temp_dir().join(format!(
            "duckle_run_{}_{}_{}.duckdb",
            std::process::id(),
            now_nanos(),
            RUN_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _guard = TempDbGuard(db_path.clone());

        // Cloud credentials, prefixed to every stage invocation (each is
        // a fresh CLI session).
        let secrets = collect_pipeline_secrets(doc);
        let secret_prefix = if secrets.is_empty() {
            String::new()
        } else {
            format!("{} ", secrets.join(" "))
        };

        let mut nodes: std::collections::BTreeMap<String, NodeRunStatus> = Default::default();
        let mut overall_error: Option<String> = None;
        let mut was_cancelled = false;
        let mut preview: Vec<NodePreview> = Vec::new();

        for stage in &compiled.stages {
            if self.cancel.load(Ordering::Relaxed) {
                was_cancelled = true;
                on_event(PipelineEvent::Cancelled);
                break;
            }
            let kind_label = match stage.kind {
                StageKind::Sink => "sink",
                StageKind::View => "view",
            };
            on_event(PipelineEvent::StageStarted {
                node_id: stage.node_id.clone(),
                label: stage.label.clone(),
                kind: kind_label.into(),
            });

            // ctl.wait / ctl.throttle inject an inter-stage delay
            // before running the SQL. Done in the executor so the
            // planner stays declarative.
            if let Some(ms) = stage.wait_ms {
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
            let started = Instant::now();
            // Advanced settings: memoryLimitMb prepends a PRAGMA so heavy
            // aggregations can be capped per stage. The PRAGMA only lives
            // for the duration of this CLI invocation.
            let memory_pragma = match stage.memory_limit_mb {
                Some(mb) => format!("PRAGMA memory_limit='{}MB'; ", mb),
                None => String::new(),
            };
            // Enforce "error if exists" before writing a local file sink.
            let sql = format!("{}{}{}", secret_prefix, memory_pragma, stage.sql);
            // Retry loop: retry_attempts >= 1; with the default of 1 we
            // call run() exactly once. Retries sleep retry_backoff_ms
            // (linearly scaled by attempt index) between attempts.
            // Cancellation is caught at the start of the *next* stage,
            // so the retry loop can complete its backoff naturally.
            let mut result = Err(EngineError::Query("stage did not run".into()));
            for attempt in 0..stage.retry_attempts {
                if attempt > 0 && stage.retry_backoff_ms > 0 {
                    let delay = stage.retry_backoff_ms.saturating_mul(attempt as u64);
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                }
                result = if let Some(spec) = stage.webhook.as_ref() {
                    // HTTP sink (snk.webhook / snk.rest): materialize the
                    // upstream as JSON via DuckDB, then dispatch one
                    // request per row or one batched request via ureq.
                    self.run_webhook(&db_path, &secret_prefix, spec)
                } else if let Some(spec) = stage.upsert.as_ref() {
                    // Relational-DB upsert: DESCRIBE the upstream first to
                    // get the column list, then assemble INSERT ... ON
                    // CONFLICT (Postgres) or ON DUPLICATE KEY UPDATE (MySQL).
                    self.run_upsert(&db_path, &secret_prefix, spec)
                } else if let Some(spec) = stage.text_search.as_ref() {
                    // FTS in DuckDB v1.5+ can't see tables created in the
                    // same -c invocation, so we stage in one CLI call then
                    // index + query in a second.
                    self.run_text_search(&db_path, &secret_prefix, &stage.node_id, spec)
                } else if stage.sink_mode.as_deref() == Some("error")
                    && stage
                        .sink_path
                        .as_deref()
                        .map(is_local_path)
                        .unwrap_or(false)
                    && std::path::Path::new(stage.sink_path.as_deref().unwrap()).exists()
                {
                    Err(EngineError::Query(format!(
                        "Output file already exists: {} (write mode is 'Error if exists')",
                        stage.sink_path.as_deref().unwrap()
                    )))
                } else {
                    self.run(Some(&db_path), &sql, false)
                };
                if result.is_ok() {
                    break;
                }
            }
            let elapsed_ms = started.elapsed().as_millis() as u64;

            match result {
                Ok(_) => {
                    let rows_opt = match stage.kind {
                        StageKind::Sink => stage
                            .from
                            .as_ref()
                            .and_then(|f| self.count_rows(&db_path, f).ok()),
                        StageKind::View => self.count_rows(&db_path, &stage.node_id).ok(),
                    };
                    nodes.insert(
                        stage.node_id.clone(),
                        NodeRunStatus {
                            status: "ok".into(),
                            kind: Some(kind_label.into()),
                            rows: rows_opt,
                            duration_ms: Some(elapsed_ms),
                            error: None,
                        },
                    );
                    on_event(PipelineEvent::StageFinished {
                        node_id: stage.node_id.clone(),
                        kind: kind_label.into(),
                        status: "ok".into(),
                        rows: rows_opt,
                        duration_ms: elapsed_ms,
                        error: None,
                    });
                    if stage.kind == StageKind::View {
                        if let Ok(p) = self.preview_table(&db_path, &stage.node_id) {
                            preview.push(p);
                        }
                    }
                }
                Err(EngineError::Cancelled) => {
                    was_cancelled = true;
                    on_event(PipelineEvent::Cancelled);
                    break;
                }
                Err(err) => {
                    let msg = err.to_string();
                    nodes.insert(
                        stage.node_id.clone(),
                        NodeRunStatus {
                            status: "error".into(),
                            kind: Some(kind_label.into()),
                            rows: None,
                            duration_ms: Some(elapsed_ms),
                            error: Some(msg.clone()),
                        },
                    );
                    on_event(PipelineEvent::StageFinished {
                        node_id: stage.node_id.clone(),
                        kind: kind_label.into(),
                        status: "error".into(),
                        rows: None,
                        duration_ms: elapsed_ms,
                        error: Some(msg.clone()),
                    });
                    overall_error.get_or_insert(format!("{}: {}", stage.label, msg));
                    break;
                }
            }
        }

        let final_status = if was_cancelled {
            "cancelled"
        } else if overall_error.is_some() {
            "error"
        } else {
            "ok"
        };
        on_event(PipelineEvent::Finished {
            status: final_status.into(),
            duration_ms: total_start.elapsed().as_millis() as u64,
        });

        RunResult {
            status: final_status.into(),
            duration_ms: total_start.elapsed().as_millis() as u64,
            nodes,
            preview,
            error: overall_error,
        }
    }

    /// Relational-DB upsert. DuckDB's ATTACH doesn't propagate the
    /// target's UNIQUE / PRIMARY KEY constraints, so a native DuckDB
    /// INSERT ... ON CONFLICT fails to bind. Instead we stage the
    /// upstream into the target DB via ATTACH and then run the real
    /// ON CONFLICT (Postgres) / ON DUPLICATE KEY UPDATE (MySQL) INSERT
    /// directly on the underlying connection through the extension's
    /// passthrough function (postgres_execute / mysql_execute).
    fn run_upsert(
        &self,
        db: &Path,
        secret_prefix: &str,
        spec: &plan::UpsertSpec,
    ) -> Result<String, EngineError> {
        let desc_sql = format!("DESCRIBE {};", plan::quote_ident(&spec.from_view));
        let rows = self.run_rows(Some(db), &desc_sql)?;
        let all_cols: Vec<String> = rows
            .iter()
            .filter_map(|r| {
                r.get("column_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        if all_cols.is_empty() {
            return Err(EngineError::Query(format!(
                "Upsert: couldn't read columns from '{}'",
                spec.from_view
            )));
        }
        let key_set: std::collections::HashSet<&str> =
            spec.conflict_cols.iter().map(|s| s.as_str()).collect();
        let set_cols: Vec<&String> = all_cols
            .iter()
            .filter(|c| !key_set.contains(c.as_str()))
            .collect();

        // Sanitized staging table name (suffix from upstream node id).
        let suffix: String = spec
            .from_view
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let target_native = spec
            .target
            .strip_prefix("duckle_dst.")
            .unwrap_or(&spec.target)
            .to_string();
        let staging_unqualified = format!("duckle_upsert_staging_{}", suffix);

        // Step 1: stage the rows in the target DB (via ATTACH).
        // Default schema differs per family (public for PG/Cockroach;
        // for MySQL the database is selected at ATTACH, no schema layer).
        let staging_native = match spec.family {
            plan::UpsertFamily::Postgres => format!("public.{}", staging_unqualified),
            plan::UpsertFamily::MySql => staging_unqualified.clone(),
        };
        let staging_duckle = format!("duckle_dst.{}", staging_native);
        let stage_sql = format!(
            "{secret}{attach}DROP TABLE IF EXISTS {sd}; \
             CREATE TABLE {sd} AS SELECT * FROM {from} WHERE 1=0; \
             INSERT INTO {sd} SELECT * FROM {from};",
            secret = secret_prefix,
            attach = spec.attach,
            sd = staging_duckle,
            from = plan::quote_ident(&spec.from_view)
        );
        self.run(Some(db), &stage_sql, false)?;

        // Step 2: assemble the real upsert SQL, run it on the native
        // connection so the constraint check sees the real schema.
        let native_sql = build_native_upsert_sql(spec, &set_cols, &target_native, &staging_native);
        let exec_fn = match spec.family {
            plan::UpsertFamily::Postgres => "postgres_execute",
            plan::UpsertFamily::MySql => "mysql_execute",
        };
        let exec_sql = format!(
            "{secret}{attach}CALL {fn_name}('duckle_dst', '{sql}');",
            secret = secret_prefix,
            attach = spec.attach,
            fn_name = exec_fn,
            sql = native_sql.replace('\'', "''")
        );
        self.run(Some(db), &exec_sql, false)
    }

    /// HTTP sink (snk.webhook / snk.rest). Materializes the upstream
    /// view via DuckDB's -json output, then either
    ///   - row mode: one ureq request per row, body = row JSON
    ///   - batch mode: a single request with body = entire array JSON
    /// Returns a synthetic 'sent N rows' report on success; aggregates
    /// per-row HTTP errors into a single Err for the run feedback layer.
    fn run_webhook(
        &self,
        db: &Path,
        secret_prefix: &str,
        spec: &WebhookSpec,
    ) -> Result<String, EngineError> {
        let select = format!(
            "{}SELECT * FROM {}",
            secret_prefix,
            plan::quote_ident(&spec.from_view)
        );
        let rows = self.run_rows(Some(db), &select)?;
        let method = if spec.method.is_empty() {
            "POST".to_string()
        } else {
            spec.method.to_uppercase()
        };
        let dispatch = |body: String, default_ct: &str| -> Result<(), EngineError> {
            let mut req = ureq::request(&method, &spec.url);
            let has_ct = spec
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
            for (k, v) in &spec.headers {
                req = req.set(k, v);
            }
            if !has_ct {
                req = req.set("content-type", default_ct);
            }
            match req.send_string(&body) {
                Ok(_) => Ok(()),
                Err(ureq::Error::Status(code, response)) => {
                    let body = response.into_string().unwrap_or_default();
                    Err(EngineError::Query(format!(
                        "HTTP {} from {}: {}",
                        code,
                        spec.url,
                        body.chars().take(200).collect::<String>()
                    )))
                }
                Err(e) => Err(EngineError::Query(format!(
                    "HTTP transport error to {}: {}",
                    spec.url, e
                ))),
            }
        };
        match spec.body_shape.as_str() {
            "batch" => {
                // Wrap the rows array in {body_wrap: [...]} when set,
                // and merge any body_extras (e.g. Milvus's collectionName).
                let body = if spec.body_wrap.is_some() || !spec.body_extras.is_empty() {
                    let mut obj = serde_json::Map::new();
                    if let Some(wrap_key) = &spec.body_wrap {
                        obj.insert(
                            wrap_key.clone(),
                            serde_json::Value::Array(rows.clone()),
                        );
                    }
                    for (k, v) in &spec.body_extras {
                        obj.insert(k.clone(), v.clone());
                    }
                    serde_json::to_string(&serde_json::Value::Object(obj))
                        .unwrap_or_else(|_| "{}".into())
                } else {
                    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
                };
                dispatch(body, "application/json")?;
                Ok(format!("sent 1 batch ({} rows) to {}", rows.len(), spec.url))
            }
            "ndjson_bulk" => {
                // Each row produces TWO lines: an action then the doc.
                // The action template lives in spec.bulk_action (set by
                // snk.elastic / snk.opensearch with the index name baked in).
                let action = spec
                    .bulk_action
                    .as_deref()
                    .unwrap_or("{\"index\":{}}");
                let mut body = String::new();
                for row in &rows {
                    body.push_str(action);
                    body.push('\n');
                    let doc = serde_json::to_string(row).unwrap_or_else(|_| "{}".into());
                    body.push_str(&doc);
                    body.push('\n');
                }
                dispatch(body, "application/x-ndjson")?;
                Ok(format!("bulk-indexed {} docs to {}", rows.len(), spec.url))
            }
            _ => {
                let mut sent = 0_usize;
                for row in &rows {
                    let body = serde_json::to_string(row).unwrap_or_else(|_| "{}".into());
                    dispatch(body, "application/json")?;
                    sent += 1;
                }
                Ok(format!("sent {} rows to {}", sent, spec.url))
            }
        }
    }

    /// Full-Text Search runs in two CLI invocations sharing the same
    /// temp DB file. The first stages the upstream into a permanent
    /// table; the second builds the BM25 index and the final node
    /// table. The split is needed for DuckDB v1.5+ where the fts
    /// PRAGMA can't see tables created in the same -c invocation; on
    /// v1.4 it just costs one extra CLI spawn.
    fn run_text_search(
        &self,
        db: &Path,
        secret_prefix: &str,
        node_id: &str,
        spec: &plan::TextSearchSpec,
    ) -> Result<String, EngineError> {
        let staging = plan::quote_ident(&spec.staging_table);
        let upstream = plan::quote_ident(&spec.from_view);
        let node_q = plan::quote_ident(node_id);
        let id_col_q = plan::quote_ident(&spec.id_col);
        let output_q = plan::quote_ident(&spec.output_col);

        // Phase 1: stage upstream into a named table that the next CLI
        // invocation will see.
        let stage_sql = format!(
            "{secret}INSTALL fts; LOAD fts; \
             DROP TABLE IF EXISTS {staging}; \
             CREATE TABLE {staging} AS SELECT * FROM {upstream};",
            secret = secret_prefix,
            staging = staging,
            upstream = upstream,
        );
        self.run(Some(db), &stage_sql, false)?;

        // Phase 2: PRAGMA create_fts_index sees the staged table from
        // disk; the same invocation then runs the BM25 SELECT.
        let text_args = spec
            .text_cols
            .iter()
            .map(|c| format!("'{}'", c.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        let index_schema = format!("fts_main_{}", spec.staging_table);
        let match_expr = format!(
            "{}.match_bm25({}, '{}')",
            index_schema,
            id_col_q,
            spec.query.replace('\'', "''")
        );
        let order_limit = match spec.top_k {
            Some(k) => format!(" ORDER BY {} DESC LIMIT {}", output_q, k),
            None => String::new(),
        };
        let index_sql = format!(
            "{secret}INSTALL fts; LOAD fts; \
             PRAGMA create_fts_index('{staging_raw}', '{id_col}', {text_args}); \
             CREATE OR REPLACE TABLE {node} AS \
               SELECT *, {match_expr} AS {output_q} FROM {staging} \
               WHERE {match_expr} IS NOT NULL{order_limit};",
            secret = secret_prefix,
            staging_raw = spec.staging_table.replace('\'', "''"),
            id_col = spec.id_col.replace('\'', "''"),
            text_args = text_args,
            node = node_q,
            match_expr = match_expr,
            output_q = output_q,
            staging = staging,
            order_limit = order_limit,
        );
        self.run(Some(db), &index_sql, false)
    }

    fn count_rows(&self, db: &Path, name: &str) -> Result<u64, EngineError> {
        let sql = format!("SELECT COUNT(*) AS n FROM {};", plan::quote_ident(name));
        let rows = self.run_rows(Some(db), &sql)?;
        let n = rows
            .first()
            .and_then(|r| r.get("n"))
            .and_then(|v| v.as_u64().or_else(|| v.as_i64().map(|x| x.max(0) as u64)))
            .unwrap_or(0);
        Ok(n)
    }

    fn preview_table(&self, db: &Path, name: &str) -> Result<NodePreview, EngineError> {
        let q = plan::quote_ident(name);
        let cols = self.run_rows(Some(db), &format!("DESCRIBE {};", q))?;
        let schema: Vec<Column> = cols.iter().filter_map(parse_describe_row).collect();
        let rows = self
            .run_rows(Some(db), &format!("SELECT * FROM {} LIMIT {};", q, PREVIEW_ROW_LIMIT))
            .unwrap_or_default();
        Ok(NodePreview {
            node_id: name.to_string(),
            columns: schema,
            rows,
        })
    }
}

/// Removes the temp run database (and its WAL) when dropped.
struct TempDbGuard(PathBuf);
impl Drop for TempDbGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let mut wal = self.0.clone().into_os_string();
        wal.push(".wal");
        let _ = std::fs::remove_file(PathBuf::from(wal));
    }
}

/// Per-process counter making each run's temp DB path unique even when
/// the wall clock does not advance between runs.
static RUN_SEQ: AtomicU64 = AtomicU64::new(0);

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// True for a local filesystem path (not a cloud / http URI).
fn is_local_path(p: &str) -> bool {
    let lower = p.to_ascii_lowercase();
    !["s3://", "gs://", "gcs://", "az://", "azure://", "http://", "https://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

/// Parse the (possibly multiple) top-level JSON arrays the DuckDB CLI
/// prints in `-json` mode.
fn parse_json_arrays(s: &str) -> Vec<Vec<JsonValue>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let stream = serde_json::Deserializer::from_str(trimmed).into_iter::<JsonValue>();
    for value in stream {
        match value {
            Ok(JsonValue::Array(a)) => out.push(a),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    out
}

/// Turn one DuckDB `DESCRIBE` row into a Column.
fn parse_describe_row(v: &JsonValue) -> Option<Column> {
    let name = v.get("column_name")?.as_str()?.to_string();
    let type_name = v
        .get("column_type")
        .and_then(JsonValue::as_str)
        .unwrap_or("VARCHAR");
    let nullable = v
        .get("null")
        .and_then(JsonValue::as_str)
        .map(|s| !s.eq_ignore_ascii_case("NO"))
        .unwrap_or(true);
    Some(Column {
        name,
        data_type: map_duckdb_type(type_name),
        nullable,
        primary_key: None,
    })
}

fn map_duckdb_type(t: &str) -> DataType {
    let upper = t.to_uppercase();
    let base = upper.split('(').next().unwrap_or(&upper).trim();
    match base {
        "BOOLEAN" | "BOOL" => DataType::Bool,
        "TINYINT" | "SMALLINT" | "INTEGER" | "INT" | "INT4" | "INT2" | "UTINYINT" | "USMALLINT"
        | "UINTEGER" => DataType::Int32,
        "BIGINT" | "INT8" | "HUGEINT" | "UBIGINT" => DataType::Int64,
        "REAL" | "FLOAT" | "FLOAT4" => DataType::Float32,
        "DOUBLE" | "FLOAT8" => DataType::Float64,
        "DECIMAL" | "NUMERIC" => DataType::Decimal,
        "DATE" => DataType::Date,
        "TIME" => DataType::Time,
        "TIMESTAMP" | "TIMESTAMP_S" | "TIMESTAMP_MS" | "TIMESTAMP_NS" | "TIMESTAMP_US"
        | "TIMESTAMPTZ" | "TIMESTAMP WITH TIME ZONE" => DataType::Timestamp,
        "JSON" | "MAP" | "STRUCT" | "LIST" | "ARRAY" => DataType::Json,
        "BLOB" | "VARBINARY" => DataType::Binary,
        _ => DataType::String,
    }
}

pub(crate) fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Build a `CREATE OR REPLACE SECRET` statement for a cloud format if
/// the options carry credentials. `secret_name` keeps per-source
/// secrets distinct so connections don't trample each other.
/// Compose the upsert + cleanup SQL that runs natively on the target
/// DB (through postgres_execute / mysql_execute), reading from the
/// staging table we just populated via ATTACH. Identifiers are native
/// to each family: double-quoted for Postgres, backticks for MySQL.
fn build_native_upsert_sql(
    spec: &plan::UpsertSpec,
    set_cols: &[&String],
    target_native: &str,
    staging_native: &str,
) -> String {
    match spec.family {
        plan::UpsertFamily::Postgres => {
            let key_list = spec
                .conflict_cols
                .iter()
                .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(", ");
            let conflict = if set_cols.is_empty() {
                format!("ON CONFLICT ({}) DO NOTHING", key_list)
            } else {
                let set_clause = set_cols
                    .iter()
                    .map(|c| {
                        let q = format!("\"{}\"", c.replace('"', "\"\""));
                        format!("{q} = EXCLUDED.{q}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("ON CONFLICT ({}) DO UPDATE SET {}", key_list, set_clause)
            };
            format!(
                "INSERT INTO {target} SELECT * FROM {staging} {conflict}; DROP TABLE {staging};",
                target = target_native,
                staging = staging_native,
                conflict = conflict
            )
        }
        plan::UpsertFamily::MySql => {
            // MySQL relies on the target's existing UNIQUE/PRIMARY KEY.
            // INSERT IGNORE is the fallback when there are no non-key
            // columns to update.
            if set_cols.is_empty() {
                format!(
                    "INSERT IGNORE INTO {target} SELECT * FROM {staging}; DROP TABLE {staging};",
                    target = target_native,
                    staging = staging_native
                )
            } else {
                let set_clause = set_cols
                    .iter()
                    .map(|c| format!("`{c}` = VALUES(`{c}`)"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "INSERT INTO {target} SELECT * FROM {staging} ON DUPLICATE KEY UPDATE {set}; DROP TABLE {staging};",
                    target = target_native,
                    staging = staging_native,
                    set = set_clause
                )
            }
        }
    }
}

pub(crate) fn secret_statement(
    format: &str,
    secret_name: &str,
    options: &JsonValue,
) -> Option<String> {
    let get = |k: &str| options.get(k).and_then(JsonValue::as_str);
    let sane = secret_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();
    match format {
        "s3" => {
            let key = get("accessKey")?;
            let sec = get("secretKey")?;
            let region = get("region").unwrap_or("us-east-1");
            let session = get("sessionToken");
            // S3-compatible (MinIO / R2 / B2) sets endpoint + url_style +
            // use_ssl. Empty / missing values are skipped so plain AWS S3
            // keeps its defaults.
            let endpoint = get("endpoint").filter(|s| !s.is_empty());
            let url_style = get("urlStyle").filter(|s| !s.is_empty());
            let use_ssl = get("useSsl").filter(|s| !s.is_empty());
            let mut parts = vec![
                "TYPE S3".to_string(),
                format!("KEY_ID '{}'", sql_escape(key)),
                format!("SECRET '{}'", sql_escape(sec)),
                format!("REGION '{}'", sql_escape(region)),
            ];
            if let Some(s) = session {
                parts.push(format!("SESSION_TOKEN '{}'", sql_escape(s)));
            }
            if let Some(e) = endpoint {
                parts.push(format!("ENDPOINT '{}'", sql_escape(e)));
            }
            if let Some(u) = url_style {
                parts.push(format!("URL_STYLE '{}'", sql_escape(u)));
            }
            if let Some(s) = use_ssl {
                // DuckDB takes USE_SSL as a bool literal, not a string.
                parts.push(format!("USE_SSL {}", s));
            }
            Some(format!(
                "CREATE OR REPLACE SECRET secret_{} ({});",
                sane,
                parts.join(", ")
            ))
        }
        "gcs" => {
            let key = get("accessKey")?;
            let sec = get("secretKey")?;
            Some(format!(
                "CREATE OR REPLACE SECRET secret_{} (TYPE GCS, KEY_ID '{}', SECRET '{}');",
                sane,
                sql_escape(key),
                sql_escape(sec)
            ))
        }
        "azureblob" => {
            let account = get("accountName")?;
            let key = get("accountKey")?;
            Some(format!(
                "CREATE OR REPLACE SECRET secret_{} (TYPE AZURE, CONNECTION_STRING 'DefaultEndpointsProtocol=https;AccountName={};AccountKey={};EndpointSuffix=core.windows.net');",
                sane,
                sql_escape(account),
                sql_escape(key)
            ))
        }
        _ => None,
    }
}

/// CREATE SECRET statements for every cloud source/sink with creds.
pub(crate) fn collect_pipeline_secrets(doc: &PipelineDoc) -> Vec<String> {
    let mut out = Vec::new();
    for node in &doc.nodes {
        let id = match node.data.component_id.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let format = match id {
            // S3-compatible (plain S3 + MinIO / R2 / B2) all use the same
            // CREATE SECRET (TYPE S3) machinery; the MinIO / R2 / B2
            // variants add ENDPOINT + URL_STYLE in the form.
            "src.s3" | "snk.s3" | "src.minio" | "src.r2" | "src.b2" => "s3",
            "src.gcs" | "snk.gcs" => "gcs",
            "src.azureblob" | "snk.azureblob" => "azureblob",
            _ => continue,
        };
        if let Some(props) = node.data.properties.as_ref() {
            if let Some(stmt) = secret_statement(format, &node.id, props) {
                out.push(stmt);
            }
        }
    }
    out
}

// ---- Streaming events + run result -------------------------------------

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
    Started {
        total_stages: u32,
    },
    StageStarted {
        node_id: String,
        label: String,
        kind: String,
    },
    StageFinished {
        node_id: String,
        kind: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        rows: Option<u64>,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Cancelled,
    Finished {
        status: String,
        duration_ms: u64,
    },
}

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub status: String,
    pub duration_ms: u64,
    pub nodes: std::collections::BTreeMap<String, NodeRunStatus>,
    pub preview: Vec<NodePreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RunResult {
    fn failed(start: Instant, error: String) -> Self {
        RunResult {
            status: "error".into(),
            duration_ms: start.elapsed().as_millis() as u64,
            nodes: Default::default(),
            preview: Vec::new(),
            error: Some(error),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct NodeRunStatus {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NodePreview {
    pub node_id: String,
    pub columns: Vec<Column>,
    pub rows: Vec<JsonValue>,
}

/// SQL for a single stage - returned by the `compile_pipeline` command
/// so the frontend can show / copy the generated SQL without running.
#[derive(Debug, Serialize)]
pub struct StageSql {
    pub node_id: String,
    pub label: String,
    pub kind: String,
    pub sql: String,
}

pub fn compile_pipeline_sql(doc: &PipelineDoc) -> Result<Vec<StageSql>, EngineError> {
    let compiled = plan::compile(doc)?;
    Ok(compiled
        .stages
        .into_iter()
        .map(|s| StageSql {
            node_id: s.node_id,
            label: s.label,
            kind: match s.kind {
                StageKind::Sink => "sink".into(),
                StageKind::View => "view".into(),
            },
            sql: s.sql,
        })
        .collect())
}
