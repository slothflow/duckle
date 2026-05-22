//! Run history persistence.
//!
//! Every pipeline execution (manual, partial, or scheduled) appends a
//! [`RunRecord`] to `<workspace>/runs/<pipeline_id>.json`. We keep the
//! most recent [`MAX_RECORDS`] entries so the file stays small and
//! git-diffs stay readable.

use crate::RunResult;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_RECORDS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    /// RFC3339 timestamp of when the run started.
    pub at: String,
    pub status: String,
    pub duration_ms: u64,
    /// Total rows written across all sinks.
    pub rows: u64,
    pub node_count: usize,
    /// What kicked off the run: "manual" / "partial" / "scheduled".
    pub trigger: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RunRecord {
    pub fn from_result(result: &RunResult, trigger: &str) -> Self {
        let rows: u64 = result
            .nodes
            .values()
            .filter_map(|n| n.rows)
            .sum();
        RunRecord {
            at: Utc::now().to_rfc3339(),
            status: result.status.clone(),
            duration_ms: result.duration_ms,
            rows,
            node_count: result.nodes.len(),
            trigger: trigger.to_string(),
            error: result.error.clone(),
        }
    }
}

fn history_file(workspace: &Path, pipeline_id: &str) -> std::path::PathBuf {
    workspace.join("runs").join(format!("{}.json", pipeline_id))
}

/// Append a record, trimming to the most recent MAX_RECORDS. Best
/// effort - IO failures are logged by the caller, not propagated.
pub fn append_run_record(
    workspace: &Path,
    pipeline_id: &str,
    record: RunRecord,
) -> std::io::Result<()> {
    let path = history_file(workspace, pipeline_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut records = load_run_history(workspace, pipeline_id);
    records.push(record);
    let start = records.len().saturating_sub(MAX_RECORDS);
    let trimmed = &records[start..];
    let json = serde_json::to_string_pretty(trimmed)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)
}

/// Load the run history for a pipeline (oldest first). Returns an empty
/// vec if there's no history yet or it can't be parsed.
pub fn load_run_history(workspace: &Path, pipeline_id: &str) -> Vec<RunRecord> {
    let path = history_file(workspace, pipeline_id);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}
