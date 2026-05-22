//! CSV (and TSV) source connector.
//!
//! Reads the header row, scans up to `sample_rows` records, infers
//! per-column types, and returns a [`duckle_plugin_sdk::Inspection`]
//! with the schema plus the sampled rows as JSON values for preview.

use async_trait::async_trait;
use duckle_metadata::{Column, DataType};
use duckle_plugin_sdk::{Connector, ConnectorKind, Inspection, InspectError, SchemaInspector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as JsonValue};
use std::fs::File;
use std::io::{BufReader, Read};

const DEFAULT_SAMPLE_ROWS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvOptions {
    pub path: String,
    #[serde(default = "default_has_header", alias = "hasHeader")]
    pub has_header: bool,
    #[serde(default = "default_delimiter")]
    pub delimiter: String,
    #[serde(default = "default_quote_char", alias = "quoteChar")]
    pub quote_char: String,
    #[serde(default = "default_encoding")]
    pub encoding: String,
    #[serde(default, alias = "skipLines")]
    pub skip_lines: usize,
    #[serde(default = "default_sample_rows", alias = "sampleRows")]
    pub sample_rows: usize,
    #[serde(default, alias = "nullValue")]
    pub null_value: Option<String>,
}

fn default_has_header() -> bool {
    true
}
fn default_delimiter() -> String {
    ",".into()
}
fn default_quote_char() -> String {
    "\"".into()
}
fn default_encoding() -> String {
    "utf-8".into()
}
fn default_sample_rows() -> usize {
    DEFAULT_SAMPLE_ROWS
}

/// CSV source connector. Stateless - one instance handles all CSV
/// schema inspections.
pub struct CsvConnector;

impl CsvConnector {
    pub const COMPONENT_ID: &'static str = "src.csv";
}

#[async_trait]
impl SchemaInspector for CsvConnector {
    fn component_id(&self) -> &str {
        Self::COMPONENT_ID
    }

    async fn inspect(&self, config: JsonValue) -> Result<Inspection, InspectError> {
        let opts: CsvOptions = serde_json::from_value(config)
            .map_err(|e| InspectError::Config(e.to_string()))?;
        // Run the synchronous CSV parsing on a blocking task so we don't
        // stall the Tokio runtime if the file is large.
        let inspection = tokio::task::spawn_blocking(move || inspect_csv(opts))
            .await
            .map_err(|e| InspectError::Other(e.to_string()))??;
        Ok(inspection)
    }
}

#[async_trait]
impl Connector for CsvConnector {
    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Source
    }
}

fn inspect_csv(opts: CsvOptions) -> Result<Inspection, InspectError> {
    let path = std::path::PathBuf::from(&opts.path);
    if !path.exists() {
        return Err(InspectError::Config(format!(
            "File does not exist: {}",
            opts.path
        )));
    }

    // Decode the file body up front via encoding_rs so we can support
    // utf-16 / latin-1 / windows-1252 in addition to utf-8.
    let mut raw = Vec::new();
    {
        let mut file = BufReader::new(File::open(&path)?);
        file.read_to_end(&mut raw)?;
    }
    let encoding = encoding_rs::Encoding::for_label(opts.encoding.as_bytes())
        .ok_or_else(|| InspectError::Config(format!("Unknown encoding {}", opts.encoding)))?;
    let (decoded, _, had_errors) = encoding.decode(&raw);
    if had_errors {
        // Don't fail - most files have stray bytes. Surface as a parse warning later.
    }
    let text = decoded.into_owned();

    // Skip leading lines per the user's preference. We just drop the
    // first N lines of the decoded text.
    let body = if opts.skip_lines > 0 {
        let mut lines = text.lines();
        for _ in 0..opts.skip_lines {
            if lines.next().is_none() {
                break;
            }
        }
        lines.collect::<Vec<_>>().join("\n")
    } else {
        text
    };

    let delim = opts.delimiter.as_bytes().first().copied().unwrap_or(b',');
    let quote = opts
        .quote_char
        .as_bytes()
        .first()
        .copied()
        .unwrap_or(b'"');

    let mut builder = csv::ReaderBuilder::new();
    builder
        .has_headers(opts.has_header)
        .delimiter(delim)
        .quote(quote)
        .flexible(true);
    let mut reader = builder.from_reader(body.as_bytes());

    let headers: Vec<String> = if opts.has_header {
        reader
            .headers()
            .map_err(|e| InspectError::Parse(format!("Header parse: {}", e)))?
            .iter()
            .map(String::from)
            .collect()
    } else {
        // For headerless files, peek the first row to determine width.
        let mut iter = reader.records();
        let first = iter
            .next()
            .ok_or_else(|| InspectError::Parse("Empty file".into()))?
            .map_err(|e| InspectError::Parse(format!("Row parse: {}", e)))?;
        let width = first.len();
        // We still need to use that first row as a sample below, so
        // rebuild the reader.
        let mut headers = (0..width).map(|i| format!("col_{}", i + 1)).collect::<Vec<_>>();
        // Rewind by rebuilding the reader.
        let mut rebuilder = csv::ReaderBuilder::new();
        rebuilder
            .has_headers(false)
            .delimiter(delim)
            .quote(quote)
            .flexible(true);
        reader = rebuilder.from_reader(body.as_bytes());
        // Pop the dummy reference to silence unused warnings while still
        // emitting the names.
        headers.shrink_to_fit();
        headers
    };

    let null_sentinel = opts.null_value.clone();
    let mut samples: Vec<csv::StringRecord> = Vec::with_capacity(opts.sample_rows);
    for (i, result) in reader.records().enumerate() {
        if i >= opts.sample_rows {
            break;
        }
        let record =
            result.map_err(|e| InspectError::Parse(format!("Row {} parse: {}", i + 1, e)))?;
        samples.push(record);
    }

    let columns: Vec<Column> = headers
        .iter()
        .enumerate()
        .map(|(idx, name)| {
            let inferred = infer_column_type(&samples, idx, null_sentinel.as_deref());
            Column {
                name: name.clone(),
                data_type: inferred,
                nullable: column_has_nulls(&samples, idx, null_sentinel.as_deref()),
                primary_key: None,
            }
        })
        .collect();

    let preview_rows = samples
        .iter()
        .take(8)
        .map(|record| build_preview_row(&headers, record, null_sentinel.as_deref(), &columns))
        .collect();

    Ok(Inspection {
        schema: columns,
        sample_rows: preview_rows,
    })
}

fn cell_value<'a>(record: &'a csv::StringRecord, idx: usize) -> Option<&'a str> {
    record.get(idx)
}

fn is_null(cell: &str, sentinel: Option<&str>) -> bool {
    let trimmed = cell.trim();
    if trimmed.is_empty() {
        return true;
    }
    if let Some(s) = sentinel {
        if trimmed == s {
            return true;
        }
    }
    matches!(trimmed.to_ascii_lowercase().as_str(), "null" | "na" | "n/a")
}

fn column_has_nulls(
    samples: &[csv::StringRecord],
    idx: usize,
    sentinel: Option<&str>,
) -> bool {
    samples
        .iter()
        .any(|r| cell_value(r, idx).map_or(true, |c| is_null(c, sentinel)))
}

fn infer_column_type(
    samples: &[csv::StringRecord],
    idx: usize,
    sentinel: Option<&str>,
) -> DataType {
    let mut has_value = false;
    let mut all_int = true;
    let mut all_float = true;
    let mut all_bool = true;
    let mut all_date = true;
    let mut all_timestamp = true;

    for record in samples {
        let Some(raw) = cell_value(record, idx) else {
            continue;
        };
        if is_null(raw, sentinel) {
            continue;
        }
        let v = raw.trim();
        has_value = true;

        if all_int && v.parse::<i64>().is_err() {
            all_int = false;
        }
        if all_float
            && (v.parse::<f64>().is_err() || v.is_empty())
        {
            all_float = false;
        }
        if all_bool
            && !matches!(
                v.to_ascii_lowercase().as_str(),
                "true" | "false" | "0" | "1" | "yes" | "no"
            )
        {
            all_bool = false;
        }
        if all_date && !is_date_like(v) {
            all_date = false;
        }
        if all_timestamp && !is_timestamp_like(v) {
            all_timestamp = false;
        }
    }

    if !has_value {
        return DataType::String;
    }
    // Order matters: timestamp is more specific than date; bool is more
    // specific than int (since "0" / "1" parse as both).
    if all_timestamp {
        return DataType::Timestamp;
    }
    if all_date {
        return DataType::Date;
    }
    if all_int {
        return DataType::Int64;
    }
    if all_float {
        return DataType::Float64;
    }
    if all_bool {
        return DataType::Bool;
    }
    DataType::String
}

fn is_date_like(s: &str) -> bool {
    // YYYY-MM-DD
    if s.len() != 10 {
        return false;
    }
    let bytes = s.as_bytes();
    bytes[4] == b'-'
        && bytes[7] == b'-'
        && s[0..4].chars().all(|c| c.is_ascii_digit())
        && s[5..7].chars().all(|c| c.is_ascii_digit())
        && s[8..10].chars().all(|c| c.is_ascii_digit())
}

fn is_timestamp_like(s: &str) -> bool {
    // YYYY-MM-DD HH:MM[:SS][.fff][Z|+HH:MM]
    if s.len() < 16 {
        return false;
    }
    if !is_date_like(&s[..10]) {
        return false;
    }
    let sep = s.as_bytes()[10];
    if sep != b'T' && sep != b' ' {
        return false;
    }
    let time = &s[11..];
    // very forgiving: just check H:M structure
    let mut parts = time.split(|c: char| c == ':' || c == '.' || c == '+' || c == '-' || c == 'Z');
    matches!(parts.next(), Some(p) if p.chars().all(|c| c.is_ascii_digit()) && (p.len() == 1 || p.len() == 2))
        && matches!(parts.next(), Some(p) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn build_preview_row(
    headers: &[String],
    record: &csv::StringRecord,
    sentinel: Option<&str>,
    columns: &[Column],
) -> JsonValue {
    let mut map = Map::with_capacity(headers.len());
    for (i, name) in headers.iter().enumerate() {
        let raw = cell_value(record, i).unwrap_or("");
        if is_null(raw, sentinel) {
            map.insert(name.clone(), JsonValue::Null);
            continue;
        }
        let trimmed = raw.trim();
        let parsed = match columns.get(i).map(|c| c.data_type) {
            Some(DataType::Int64) => trimmed.parse::<i64>().map(JsonValue::from).ok(),
            Some(DataType::Float64) => trimmed.parse::<f64>().map(JsonValue::from).ok(),
            Some(DataType::Bool) => match trimmed.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => Some(json!(true)),
                "false" | "0" | "no" => Some(json!(false)),
                _ => None,
            },
            _ => None,
        };
        map.insert(name.clone(), parsed.unwrap_or_else(|| JsonValue::String(trimmed.to_string())));
    }
    JsonValue::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_csv(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn infers_basic_schema() {
        let f = write_csv(
            "order_id,status,amount,created_at\n\
             1001,paid,129.95,2026-05-18\n\
             1002,pending,49.00,2026-05-18\n\
             1003,paid,12.50,2026-05-19\n",
        );
        let cfg = serde_json::json!({ "path": f.path().to_str().unwrap() });
        let inspection = CsvConnector.inspect(cfg).await.unwrap();
        let schema = &inspection.schema;
        assert_eq!(schema.len(), 4);
        assert_eq!(schema[0].name, "order_id");
        assert_eq!(schema[0].data_type, DataType::Int64);
        assert_eq!(schema[1].data_type, DataType::String);
        assert_eq!(schema[2].data_type, DataType::Float64);
        assert_eq!(schema[3].data_type, DataType::Date);
        assert_eq!(inspection.sample_rows.len(), 3);
    }

    #[tokio::test]
    async fn handles_null_sentinel() {
        let f = write_csv(
            "id,amount\n\
             1,100\n\
             2,NA\n\
             3,200\n",
        );
        let cfg = serde_json::json!({
            "path": f.path().to_str().unwrap(),
            "nullValue": "NA",
        });
        let inspection = CsvConnector.inspect(cfg).await.unwrap();
        assert_eq!(inspection.schema[1].data_type, DataType::Int64);
        assert!(inspection.schema[1].nullable);
    }

    #[tokio::test]
    async fn missing_file_errors() {
        let cfg = serde_json::json!({ "path": "/nonexistent/path/orders.csv" });
        let err = CsvConnector.inspect(cfg).await.unwrap_err();
        assert!(matches!(err, InspectError::Config(_)));
    }
}
