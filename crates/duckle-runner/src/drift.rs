//! `duckle-runner drift`: detect schema drift in a pipeline's sources. For each
//! source node that declares a schema, it reads the source's live schema from
//! the real data and reports columns the source no longer provides, columns it
//! added, and columns whose type changed. The shared comparison lives in
//! `duckle_duckdb_engine::drift`; this command resolves placeholders the same
//! way a real run does, then prints the report (text or JSON).

use std::path::PathBuf;

use duckle_duckdb_engine::{context, drift, DuckdbEngine, PipelineDoc};

pub const DRIFT_USAGE: &str = "\
duckle-runner drift - detect schema drift in a pipeline's sources

USAGE:
    duckle-runner drift --pipeline <file.json> [options]

OPTIONS:
    --json                 Emit the full report as JSON.
    --workspace <dir>      Workspace root for placeholder/secret resolution
                           (default: the pipeline file's directory).
    --duckdb <path>        DuckDB CLI (else DUCKLE_DUCKDB_BIN / PATH).

For each source node that declares a schema, the live schema is read from the
real data (the same path the Autodetect button uses) and compared to the
declared one. Reports missing columns (the source no longer provides a declared
column), added columns (present in the source but not declared), and type
changes. Database / REST / streaming sources whose schema cannot be introspected
are reported but do not affect the verdict.

Exit code: 0 no breaking drift, 1 a declared column is missing or changed type,
2 usage/IO error.";

/// `duckle-runner drift`: load a pipeline, resolve placeholders, read each
/// source's live schema, and report drift against the declared schema.
pub fn run() -> Result<i32, String> {
    let mut pipeline: Option<PathBuf> = None;
    let mut workspace_arg: Option<PathBuf> = None;
    let mut duckdb_arg: Option<PathBuf> = None;
    let mut as_json = false;
    let mut it = std::env::args().skip(2); // skip the exe and the "drift" verb
    while let Some(a) = it.next() {
        match a.as_str() {
            "--pipeline" => {
                pipeline = Some(PathBuf::from(it.next().ok_or("--pipeline needs a value")?))
            }
            "--workspace" => {
                workspace_arg = Some(PathBuf::from(it.next().ok_or("--workspace needs a value")?))
            }
            "--duckdb" => {
                duckdb_arg = Some(PathBuf::from(it.next().ok_or("--duckdb needs a value")?))
            }
            "--json" => as_json = true,
            "-h" | "--help" => {
                println!("{DRIFT_USAGE}");
                return Ok(0);
            }
            other if pipeline.is_none() && !other.starts_with('-') => {
                pipeline = Some(PathBuf::from(other))
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let pipeline = pipeline.ok_or("--pipeline <file.json> is required")?;
    let text = std::fs::read_to_string(&pipeline)
        .map_err(|e| format!("read {}: {e}", pipeline.display()))?;
    let mut doc: PipelineDoc =
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", pipeline.display()))?;

    // Resolve placeholders the same way a headless run does so source paths
    // (${ENV:...}, ${workspace}, ${date}) point at the real data.
    let workspace = workspace_arg
        .clone()
        .or_else(|| pipeline.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let env_file = workspace.join("secrets.env");
    crate::apply_env_pass(&mut doc, &workspace, &env_file)?;
    context::apply_time_builtins(&mut doc);
    context::apply_workspace_context(&mut doc, &workspace);
    std::env::set_var("DUCKLE_WORKSPACE", &workspace);

    let duckdb = crate::resolve_duckdb(duckdb_arg)?;
    std::env::set_var("DUCKLE_DUCKDB_BIN", &duckdb);
    let engine = DuckdbEngine::new(duckdb);
    let report = drift::schema_drift(&engine, &doc);

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    } else {
        println!("drift: {}", pipeline.display());
        let sources = report["sources"].as_array().cloned().unwrap_or_default();
        for s in &sources {
            let node = s["nodeId"].as_str().unwrap_or("");
            let label = s["label"].as_str().unwrap_or("");
            let cid = s["componentId"].as_str().unwrap_or("");
            let status = s["status"].as_str().unwrap_or("");
            println!("  {node} ({label}) [{cid}]: {status}");
            let list = |k: &str| {
                s[k].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            };
            let missing = list("missingColumns");
            let added = list("addedColumns");
            if !missing.is_empty() {
                println!("    - missing: {missing}");
            }
            if !added.is_empty() {
                println!("    + added:   {added}");
            }
            for c in s["typeChanges"].as_array().cloned().unwrap_or_default() {
                println!(
                    "    ~ type:    {} {} -> {}",
                    c["column"].as_str().unwrap_or(""),
                    c["declared"].as_str().unwrap_or(""),
                    c["live"].as_str().unwrap_or("")
                );
            }
            if let Some(note) = s["note"].as_str() {
                println!("    ({note})");
            }
        }
        let sm = &report["summary"];
        let n = |k: &str| sm[k].as_u64().unwrap_or(0);
        println!(
            "  summary: {} checked, {} with drift, {} breaking, {} not-introspectable, {} unreadable, {} no-schema",
            n("sourcesChecked"),
            n("sourcesWithDrift"),
            n("breakingSources"),
            n("notIntrospectable"),
            n("unreadable"),
            n("noDeclaredSchema")
        );
    }

    Ok(if report["hasBreaking"] == serde_json::json!(true) { 1 } else { 0 })
}
