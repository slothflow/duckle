//! Duckle DuckDB engine adapter.
//!
//! Holds a single in-process DuckDB connection and lends it out to
//! callers that need to inspect a source's schema, sample its rows, or
//! eventually execute a full pipeline plan.
//!
//! Most heavy lifting (CSV inference, Parquet schema, JSON inference,
//! SQLite scanning) is delegated to DuckDB's own readers via SQL like
//! `DESCRIBE SELECT * FROM read_csv_auto('...')`. This means we get
//! DuckDB's mature dialect inference for free instead of re-implementing
//! it in Rust.

use async_trait::async_trait;
use duckdb::{Connection, InterruptHandle};
use duckle_metadata::{Column, DataType};
use duckle_plugin_sdk::{Inspection, InspectError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use thiserror::Error;

pub mod history;
pub mod plan;
pub use history::{append_run_record, load_run_history, RunRecord};
pub use plan::{CompiledPipeline, PipelineDoc, Stage, StageKind};

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("duckdb: {0}")]
    Duck(#[from] duckdb::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
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

/// Sample rows fetched alongside the schema for the Preview tab.
const PREVIEW_LIMIT: usize = 8;

/// Source-format / target-system dispatcher used by the autodetect Tauri
/// command. New formats only need a new arm here.
#[derive(Clone)]
pub struct DuckdbEngine {
    conn: Arc<Mutex<Connection>>,
    cancel: Arc<AtomicBool>,
    interrupt: Arc<InterruptHandle>,
}

impl std::fmt::Debug for DuckdbEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DuckdbEngine").finish_non_exhaustive()
    }
}

impl DuckdbEngine {
    /// Create a fresh in-memory DuckDB instance. The engine is process-
    /// global, but each `inspect_*` call uses isolated SQL so multiple
    /// runs don't trample each other.
    pub fn new() -> Result<Self, EngineError> {
        let conn = Connection::open_in_memory()?;
        // Install the extensions we use, then LOAD them into this
        // connection. INSTALL writes to a process/user-global extension
        // directory, so we serialize it with a `Once` — otherwise two
        // engines starting at the same time race on the install file
        // move ("Could not move file: Access is denied"). Subsequent
        // engines skip INSTALL (the files are already on disk) and only
        // LOAD into their own connection.
        //
        // parquet is preloaded so the first `COPY ... (FORMAT PARQUET)`
        // doesn't trigger a lazy auto-install (which needs network +
        // a writable dir and is the racy path the integration tests
        // caught).
        static EXT_INSTALL: std::sync::Once = std::sync::Once::new();
        EXT_INSTALL.call_once(|| {
            let _ = conn.execute_batch(
                "INSTALL sqlite; INSTALL json; INSTALL httpfs; INSTALL parquet;",
            );
        });
        let _ = conn.execute_batch("LOAD sqlite; LOAD json; LOAD httpfs; LOAD parquet;");
        let interrupt = conn.interrupt_handle();
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            cancel: Arc::new(AtomicBool::new(false)),
            interrupt,
        })
    }

    /// Request that any in-flight pipeline run halts. Two layers:
    ///   1. Sets the cancel flag so the run loop bails before starting
    ///      another stage.
    ///   2. Calls `InterruptHandle::interrupt()` — this is the
    ///      thread-safe, lock-free interrupt path. It signals DuckDB
    ///      to abort whatever the connection is currently executing,
    ///      so even a long-running query returns ~immediately.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.interrupt.interrupt();
    }

    /// Clear a pending cancel — done at the start of every new run.
    pub fn clear_cancel(&self) {
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// Run an arbitrary closure with the underlying connection. Locked
    /// across the closure so we can hold prepared statements safely.
    pub fn with_connection<R>(&self, f: impl FnOnce(&Connection) -> R) -> R {
        let guard = self.conn.lock().expect("duckdb connection poisoned");
        f(&guard)
    }

    /// Inspect a source for its schema and a small preview. `format` is
    /// the same string the frontend ships (`"csv"`, `"parquet"`, ...).
    pub fn inspect(&self, format: &str, options: JsonValue) -> Result<Inspection, EngineError> {
        match format {
            "csv" | "tsv" => self.inspect_csv(options),
            "parquet" => self.inspect_parquet(options),
            "json" | "jsonl" | "ndjson" => self.inspect_json(options),
            "sqlite" => self.inspect_sqlite(options),
            "duckdb" => self.inspect_duckdb(options),
            "s3" | "gcs" | "azureblob" | "http" | "https" => {
                self.inspect_cloud_url(format, options)
            }
            other => Err(EngineError::Unsupported(format!(
                "Format '{}' is not supported by the DuckDB engine yet",
                other
            ))),
        }
    }

    /// Cloud / HTTP URL inspector. Routes by the URL's file extension
    /// to the matching DuckDB reader. Installs the right extensions
    /// lazily (azure needs the azure extension; gcs/s3 work through
    /// httpfs). If the request options include credentials (placed
    /// there by frontend connection auto-fill), they are wired into a
    /// DuckDB SECRET before the inspect runs; otherwise the engine
    /// falls back to environment variables.
    fn inspect_cloud_url(
        &self,
        format: &str,
        options: JsonValue,
    ) -> Result<Inspection, EngineError> {
        // Apply credentials if present.
        if let Some(stmt) = secret_statement(format, "duckle_inspect", &options) {
            self.with_connection(|conn| {
                if let Err(e) = conn.execute_batch(&stmt) {
                    tracing::warn!("CREATE SECRET failed (inspect): {}", e);
                }
            });
        }
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Opts {
            #[serde(default)]
            path: Option<String>,
            #[serde(default)]
            url: Option<String>,
            #[serde(default)]
            bucket: Option<String>,
            #[serde(default)]
            key: Option<String>,
            #[serde(default)]
            format: Option<String>,
        }
        let opts: Opts =
            serde_json::from_value(options.clone()).map_err(|e| EngineError::Config(e.to_string()))?;
        let url = opts
            .path
            .or(opts.url)
            .or_else(|| match (opts.bucket, opts.key) {
                (Some(b), Some(k)) => {
                    let scheme = match format {
                        "s3" => "s3://",
                        "gcs" => "gs://",
                        "azureblob" => "az://",
                        _ => "https://",
                    };
                    Some(format!("{}{}/{}", scheme, b, k.trim_start_matches('/')))
                }
                _ => None,
            })
            .ok_or_else(|| {
                EngineError::Config("Cloud source needs a path / URL".into())
            })?;

        // Lazily install Azure extension if needed.
        if format == "azureblob" {
            self.with_connection(|conn| {
                let _ = conn.execute_batch("INSTALL azure; LOAD azure;");
            });
        }

        // Infer file format from URL extension if user didn't specify.
        let lower = url.to_ascii_lowercase();
        let chosen = opts.format.as_deref().map(str::to_string).unwrap_or_else(|| {
            if lower.ends_with(".parquet") || lower.ends_with(".pq") {
                "parquet".into()
            } else if lower.ends_with(".json") || lower.ends_with(".jsonl") || lower.ends_with(".ndjson") {
                "json".into()
            } else if lower.ends_with(".tsv") {
                "tsv".into()
            } else {
                "csv".into()
            }
        });

        let mut new_options = options;
        if let Some(obj) = new_options.as_object_mut() {
            obj.insert("path".into(), JsonValue::String(url));
        }
        self.inspect(&chosen, new_options)
    }

    fn inspect_csv(&self, options: JsonValue) -> Result<Inspection, EngineError> {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Opts {
            path: String,
            #[serde(default = "yes")]
            has_header: bool,
            #[serde(default)]
            delimiter: Option<String>,
            #[serde(default)]
            quote_char: Option<String>,
            #[serde(default)]
            null_value: Option<String>,
            #[serde(default)]
            skip_lines: Option<u32>,
            #[serde(default)]
            encoding: Option<String>,
        }
        fn yes() -> bool {
            true
        }

        let opts: Opts =
            serde_json::from_value(options).map_err(|e| EngineError::Config(e.to_string()))?;
        let mut args = vec![format!("'{}'", sql_escape(&opts.path))];
        args.push(format!("header={}", opts.has_header));
        if let Some(d) = &opts.delimiter {
            args.push(format!("delim='{}'", sql_escape(d)));
        }
        if let Some(q) = &opts.quote_char {
            if !q.is_empty() {
                args.push(format!("quote='{}'", sql_escape(q)));
            }
        }
        if let Some(n) = &opts.null_value {
            if !n.is_empty() {
                args.push(format!("nullstr='{}'", sql_escape(n)));
            }
        }
        if let Some(s) = opts.skip_lines {
            if s > 0 {
                args.push(format!("skip={}", s));
            }
        }
        if let Some(e) = opts.encoding {
            if !e.is_empty() {
                args.push(format!("encoding='{}'", sql_escape(&e)));
            }
        }
        let from = format!("read_csv_auto({})", args.join(", "));
        self.describe_and_preview(&from)
    }

    fn inspect_parquet(&self, options: JsonValue) -> Result<Inspection, EngineError> {
        #[derive(Debug, Deserialize)]
        struct Opts {
            path: String,
        }
        let opts: Opts =
            serde_json::from_value(options).map_err(|e| EngineError::Config(e.to_string()))?;
        let from = format!("read_parquet('{}')", sql_escape(&opts.path));
        self.describe_and_preview(&from)
    }

    fn inspect_json(&self, options: JsonValue) -> Result<Inspection, EngineError> {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Opts {
            path: String,
            #[serde(default)]
            format: Option<String>,
        }
        let opts: Opts =
            serde_json::from_value(options).map_err(|e| EngineError::Config(e.to_string()))?;
        let mut args = vec![format!("'{}'", sql_escape(&opts.path))];
        if let Some(fmt) = opts.format.as_deref() {
            let mapped = match fmt {
                "array" => "array",
                "jsonl" | "ndjson" => "newline_delimited",
                "object" => "unstructured",
                _ => "auto",
            };
            args.push(format!("format='{}'", mapped));
        }
        let from = format!("read_json_auto({})", args.join(", "));
        self.describe_and_preview(&from)
    }

    fn inspect_sqlite(&self, options: JsonValue) -> Result<Inspection, EngineError> {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Opts {
            database: String,
            mode: Option<String>,
            schema_name: Option<String>,
            table_name: Option<String>,
            sql: Option<String>,
        }
        let opts: Opts =
            serde_json::from_value(options).map_err(|e| EngineError::Config(e.to_string()))?;
        let from = match opts.mode.as_deref().unwrap_or("table") {
            "sql" => {
                let sql = opts
                    .sql
                    .ok_or_else(|| EngineError::Config("SQL query is required".into()))?;
                format!("sqlite_scan('{}', '{}')", sql_escape(&opts.database), sql_escape(&sql))
            }
            _ => {
                let table = opts
                    .table_name
                    .ok_or_else(|| EngineError::Config("Table name is required".into()))?;
                let qualified = match opts.schema_name {
                    Some(s) if !s.is_empty() => format!("{}.{}", s, table),
                    _ => table,
                };
                format!(
                    "sqlite_scan('{}', '{}')",
                    sql_escape(&opts.database),
                    sql_escape(&qualified)
                )
            }
        };
        self.describe_and_preview(&from)
    }

    fn inspect_duckdb(&self, options: JsonValue) -> Result<Inspection, EngineError> {
        #[derive(Debug, Deserialize)]
        struct Opts {
            database: String,
            sql: Option<String>,
        }
        let opts: Opts =
            serde_json::from_value(options).map_err(|e| EngineError::Config(e.to_string()))?;
        // Attach the other DB file and run user SQL (or SELECT *) against it.
        self.with_connection(|conn| -> Result<Inspection, EngineError> {
            conn.execute_batch(&format!(
                "ATTACH '{}' AS source_db (READ_ONLY);",
                sql_escape(&opts.database)
            ))?;
            let result = (|| -> Result<Inspection, EngineError> {
                let sql = opts
                    .sql
                    .as_deref()
                    .unwrap_or("SELECT 1 AS placeholder LIMIT 0");
                let from = format!("({})", sql);
                self.describe_and_preview(&from)
            })();
            let _ = conn.execute_batch("DETACH source_db;");
            result
        })
    }

    fn describe_and_preview(&self, from_clause: &str) -> Result<Inspection, EngineError> {
        self.with_connection(|conn| -> Result<Inspection, EngineError> {
            let schema = read_schema(conn, from_clause)?;
            let sample_rows = read_preview(conn, from_clause, &schema, PREVIEW_LIMIT)?;
            Ok(Inspection {
                schema,
                sample_rows,
            })
        })
    }
}

fn read_schema(conn: &Connection, from_clause: &str) -> Result<Vec<Column>, EngineError> {
    let mut stmt = conn.prepare(&format!("DESCRIBE SELECT * FROM {}", from_clause))?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let type_name: String = row.get(1)?;
        let null_flag: String = row.get(2).unwrap_or_else(|_| "YES".to_string());
        Ok((name, type_name, null_flag))
    })?;
    let mut columns = Vec::new();
    for row in rows {
        let (name, type_name, null_flag) = row?;
        columns.push(Column {
            name,
            data_type: map_duckdb_type(&type_name),
            nullable: !null_flag.eq_ignore_ascii_case("NO"),
            primary_key: None,
        });
    }
    Ok(columns)
}

fn read_preview(
    conn: &Connection,
    from_clause: &str,
    schema: &[Column],
    limit: usize,
) -> Result<Vec<JsonValue>, EngineError> {
    if schema.is_empty() {
        return Ok(Vec::new());
    }
    let column_list = schema
        .iter()
        .map(|c| format!("\"{}\"", c.name.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {} FROM {} LIMIT {}",
        column_list, from_clause, limit
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut map = Map::with_capacity(schema.len());
        for (idx, col) in schema.iter().enumerate() {
            let value = duckdb_value_to_json(row, idx);
            map.insert(col.name.clone(), value);
        }
        out.push(JsonValue::Object(map));
    }
    Ok(out)
}

fn map_duckdb_type(t: &str) -> DataType {
    // DuckDB type names are uppercase but may include parameters: VARCHAR,
    // INTEGER, BIGINT, DECIMAL(10,2), TIMESTAMP_NS, etc.
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

fn duckdb_value_to_json(row: &duckdb::Row<'_>, idx: usize) -> JsonValue {
    use duckdb::types::Value;
    let value: Value = match row.get::<usize, Value>(idx) {
        Ok(v) => v,
        Err(_) => return JsonValue::Null,
    };
    match value {
        Value::Null => JsonValue::Null,
        Value::Boolean(b) => JsonValue::Bool(b),
        Value::TinyInt(n) => JsonValue::from(n),
        Value::SmallInt(n) => JsonValue::from(n),
        Value::Int(n) => JsonValue::from(n),
        Value::BigInt(n) => JsonValue::from(n),
        Value::HugeInt(n) => JsonValue::String(n.to_string()),
        Value::UTinyInt(n) => JsonValue::from(n),
        Value::USmallInt(n) => JsonValue::from(n),
        Value::UInt(n) => JsonValue::from(n),
        Value::UBigInt(n) => JsonValue::String(n.to_string()),
        Value::Float(f) => serde_json::Number::from_f64(f as f64)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Double(f) => serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Decimal(d) => JsonValue::String(d.to_string()),
        Value::Timestamp(_, _) | Value::Date32(_) | Value::Time64(_, _) => {
            JsonValue::String(format!("{:?}", value))
        }
        Value::Text(s) => JsonValue::String(s),
        Value::Blob(b) => JsonValue::String(format!("<{} bytes>", b.len())),
        other => JsonValue::String(format!("{:?}", other)),
    }
}

pub(crate) fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Build a `CREATE OR REPLACE SECRET` statement for a cloud format if
/// the options carry credentials. `secret_name` is used to make
/// per-source secrets so different connections in the same pipeline
/// don't trample each other.
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
            let mut parts = vec![
                "TYPE S3".to_string(),
                format!("KEY_ID '{}'", sql_escape(key)),
                format!("SECRET '{}'", sql_escape(sec)),
                format!("REGION '{}'", sql_escape(region)),
            ];
            if let Some(s) = session {
                parts.push(format!("SESSION_TOKEN '{}'", sql_escape(s)));
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

/// Walk the pipeline doc and produce CREATE SECRET statements for
/// every cloud source/sink that has its credentials populated.
pub(crate) fn collect_pipeline_secrets(doc: &PipelineDoc) -> Vec<String> {
    let mut out = Vec::new();
    for node in &doc.nodes {
        let id = match node.data.component_id.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let format = match id {
            "src.s3" | "snk.s3" => "s3",
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

// ---- Pipeline execution ------------------------------------------------

/// Streaming events emitted while a pipeline runs. Tauri's `Channel`
/// ferries these to the frontend so the UI can light up node badges
/// stage-by-stage without waiting for the final result.
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

const PREVIEW_ROW_LIMIT: usize = 50;

/// SQL for a single stage of a compiled pipeline. Returned by the
/// `compile_pipeline` Tauri command so the frontend can show the
/// generated SQL or copy it to the clipboard without executing.
#[derive(Debug, Serialize)]
pub struct StageSql {
    pub node_id: String,
    pub label: String,
    pub kind: String,
    pub sql: String,
}

/// Free function — doesn't need an engine to compile a pipeline.
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

impl DuckdbEngine {
    /// Execute a pipeline end-to-end with no event stream.
    pub fn execute_pipeline(&self, doc: &PipelineDoc) -> RunResult {
        self.execute_pipeline_with_events(doc, None::<&str>, |_| {})
    }

    /// Execute a pipeline emitting [`PipelineEvent`]s through the given
    /// callback. If `target` is `Some`, runs only the subgraph upstream
    /// of (and including) that node — the "run from here" path.
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

        let compiled_result = if let Some(target_id) = target {
            plan::compile_partial(doc, target_id)
        } else {
            plan::compile(doc)
        };
        let compiled = match compiled_result {
            Ok(c) => c,
            Err(e) => {
                return RunResult {
                    status: "error".into(),
                    duration_ms: total_start.elapsed().as_millis() as u64,
                    nodes: Default::default(),
                    preview: Vec::new(),
                    error: Some(e.to_string()),
                };
            }
        };

        on_event(PipelineEvent::Started {
            total_stages: compiled.stages.len() as u32,
        });

        // Wire any cloud credentials before stages run. Failures here
        // are non-fatal — DuckDB will fall back to env vars / IMDS.
        let secret_stmts = collect_pipeline_secrets(doc);
        if !secret_stmts.is_empty() {
            self.with_connection(|conn| {
                for stmt in &secret_stmts {
                    if let Err(e) = conn.execute_batch(stmt) {
                        tracing::warn!("CREATE SECRET failed: {}", e);
                    }
                }
            });
        }

        let mut nodes: std::collections::BTreeMap<String, NodeRunStatus> = Default::default();
        let mut overall_error: Option<String> = None;
        let mut was_cancelled = false;
        let mut preview_collected: Vec<NodePreview> = Vec::new();

        // Drop any leftover views from a prior run so we don't read
        // stale data.
        self.with_connection(|conn| {
            for stage in &compiled.stages {
                if stage.kind == StageKind::View {
                    let _ = conn.execute(
                        &format!("DROP VIEW IF EXISTS {}", plan::quote_ident(&stage.node_id)),
                        [],
                    );
                }
            }
        });

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

            let started = Instant::now();
            let mut previews_for_stage: Vec<NodePreview> = Vec::new();
            let result = self.with_connection(|conn| {
                if stage.kind == StageKind::Sink {
                    let rows = conn.execute(&stage.sql, [])?;
                    Ok::<u64, duckdb::Error>(rows as u64)
                } else {
                    conn.execute(&stage.sql, [])?;
                    Ok(0)
                }
            });
            let elapsed_ms = started.elapsed().as_millis() as u64;

            // For view stages, after a successful creation, also count
            // rows + grab a preview so the frontend can light up the
            // node's "n rows" badge and populate its Preview tab.
            let view_row_count = if let Ok(_) = &result {
                if stage.kind == StageKind::View {
                    let stage_id = stage.node_id.clone();
                    let from_clause = plan::quote_ident(&stage_id);
                    let count_result = self.with_connection(|conn| {
                        let mut stmt = conn
                            .prepare(&format!("SELECT COUNT(*) FROM {}", from_clause))?;
                        let n: i64 = stmt.query_row([], |r| r.get::<usize, i64>(0))?;
                        Ok::<i64, duckdb::Error>(n)
                    });
                    if let Ok(p) = self.preview_view(&stage_id) {
                        previews_for_stage.push(p);
                    }
                    count_result.ok().map(|n| n.max(0) as u64)
                } else {
                    None
                }
            } else {
                None
            };

            match result {
                Ok(rows) => {
                    let rows_opt = if stage.kind == StageKind::Sink {
                        Some(rows)
                    } else {
                        view_row_count
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
                    // Collect previews for every view, not just leaves.
                    preview_collected.extend(previews_for_stage);
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

        // Previews collected per-stage during the run; nothing extra
        // to do here unless we want to fall back to leaves on partial
        // failure.
        let preview = preview_collected;

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

    fn preview_view(&self, view_id: &str) -> Result<NodePreview, EngineError> {
        let from_clause = plan::quote_ident(view_id);
        let inspection = self.with_connection(|conn| -> Result<Inspection, EngineError> {
            let schema = read_schema(conn, &from_clause)?;
            let rows = read_preview(conn, &from_clause, &schema, PREVIEW_ROW_LIMIT)?;
            Ok(Inspection {
                schema,
                sample_rows: rows,
            })
        })?;
        Ok(NodePreview {
            node_id: view_id.to_string(),
            columns: inspection.schema,
            rows: inspection.sample_rows,
        })
    }
}

/// Convenience: a [`SchemaInspector`] impl backed by [`DuckdbEngine`].
/// Used by the desktop autodetect command.
pub struct DuckdbInspector {
    engine: DuckdbEngine,
    format: String,
}

impl DuckdbInspector {
    pub fn new(engine: DuckdbEngine, format: impl Into<String>) -> Self {
        Self {
            engine,
            format: format.into(),
        }
    }
}

#[async_trait]
impl duckle_plugin_sdk::SchemaInspector for DuckdbInspector {
    fn component_id(&self) -> &str {
        &self.format
    }

    async fn inspect(
        &self,
        config: JsonValue,
    ) -> Result<Inspection, InspectError> {
        let engine = self.engine.clone();
        let format = self.format.clone();
        tokio::task::spawn_blocking(move || engine.inspect(&format, config))
            .await
            .map_err(|e| InspectError::Other(e.to_string()))?
            .map_err(Into::into)
    }
}

/// Lightweight serializable preview row — useful in tests + the
/// downstream Tauri command output.
#[derive(Debug, Serialize)]
pub struct PreviewRow(pub Map<String, JsonValue>);

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn csv_via_duckdb() {
        let mut f = NamedTempFile::new().unwrap();
        write!(
            f,
            "order_id,status,amount,created_at\n\
             1001,paid,129.95,2026-05-18\n\
             1002,pending,49.00,2026-05-18\n\
             1003,paid,12.50,2026-05-19\n"
        )
        .unwrap();
        f.flush().unwrap();

        let engine = DuckdbEngine::new().unwrap();
        let result = engine
            .inspect(
                "csv",
                serde_json::json!({ "path": f.path().to_str().unwrap() }),
            )
            .unwrap();
        assert_eq!(result.schema.len(), 4);
        assert_eq!(result.schema[0].name, "order_id");
        assert!(
            matches!(result.schema[0].data_type, DataType::Int32 | DataType::Int64),
            "expected order_id to be integer, got {:?}",
            result.schema[0].data_type
        );
        assert!(result.sample_rows.len() <= PREVIEW_LIMIT);
        assert!(result.sample_rows.len() >= 3);
    }
}
