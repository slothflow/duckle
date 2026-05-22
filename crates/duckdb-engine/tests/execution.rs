//! End-to-end execution tests for the DuckDB engine.
//!
//! Unlike the unit tests in `src/`, which check SQL *generation*, these
//! exercise the real read → transform → write path against temp files
//! and then read the output back to prove the data actually landed.

use duckle_duckdb_engine::{DuckdbEngine, PipelineDoc};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// These tests drive the real DuckDB CLI. Point DUCKLE_DUCKDB_BIN at a
/// `duckdb` binary to run them; otherwise they soft-skip so `cargo test`
/// stays green in environments without it.
fn engine() -> Option<DuckdbEngine> {
    let bin = std::env::var("DUCKLE_DUCKDB_BIN").ok()?;
    let p = std::path::PathBuf::from(bin);
    p.exists().then(|| DuckdbEngine::new(p))
}

macro_rules! engine_or_skip {
    () => {
        match engine() {
            Some(e) => e,
            None => {
                eprintln!("skipping: set DUCKLE_DUCKDB_BIN to a duckdb CLI to run");
                return;
            }
        }
    };
}

fn write_file(dir: &Path, name: &str, content: &str) -> String {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    norm(&path.to_string_lossy())
}

fn out_path(dir: &Path, name: &str) -> String {
    norm(&dir.join(name).to_string_lossy())
}

/// DuckDB is happiest with forward slashes even on Windows.
fn norm(p: &str) -> String {
    p.replace('\\', "/")
}

fn doc(nodes: Value, edges: Value) -> PipelineDoc {
    serde_json::from_value(json!({ "nodes": nodes, "edges": edges })).unwrap()
}

fn node(id: &str, component: &str, props: Value) -> Value {
    json!({
        "id": id,
        "position": { "x": 0, "y": 0 },
        "data": { "label": id, "componentId": component, "properties": props }
    })
}

fn main_edge(id: &str, source: &str, target: &str) -> Value {
    json!({ "id": id, "source": source, "target": target, "data": { "connectionType": "main" } })
}

/// Edge that leaves a specific output handle of the source (e.g. the
/// "reject" port of a validator).
fn port_edge(id: &str, source: &str, source_handle: &str, target: &str) -> Value {
    json!({
        "id": id,
        "source": source,
        "sourceHandle": source_handle,
        "target": target,
        "data": { "connectionType": if source_handle == "reject" { "reject" } else { "main" } }
    })
}

/// Edge into a node's `lookup` input port (used for join/CDC second
/// inputs, e.g. the "previous" snapshot of a Diff Detect).
fn lookup_edge(id: &str, source: &str, target: &str) -> Value {
    json!({
        "id": id,
        "source": source,
        "target": target,
        "targetHandle": "lookup",
        "data": { "connectionType": "lookup" }
    })
}

/// Read back output files independently of the engine, by shelling out
/// to the same DuckDB CLI (only called after engine_or_skip!, so the
/// binary is present).
fn duckdb_json(sql: &str) -> Vec<Value> {
    let bin = std::env::var("DUCKLE_DUCKDB_BIN").expect("DUCKLE_DUCKDB_BIN set");
    let out = std::process::Command::new(bin)
        .arg(":memory:")
        .arg("-json")
        .arg("-c")
        .arg(sql)
        .output()
        .expect("run duckdb");
    let s = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(s.trim()).unwrap_or_default()
}

/// Run setup SQL against a specific database file (used to seed a
/// source DB file for the duckdb-source test).
fn duckdb_exec(db: &str, sql: &str) {
    let bin = std::env::var("DUCKLE_DUCKDB_BIN").expect("DUCKLE_DUCKDB_BIN set");
    let out = std::process::Command::new(bin)
        .arg(db)
        .arg("-c")
        .arg(sql)
        .output()
        .expect("run duckdb");
    assert!(
        out.status.success(),
        "setup sql failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn count(from: &str) -> i64 {
    let rows = duckdb_json(&format!("SELECT COUNT(*) AS n FROM {}", from));
    rows.first()
        .and_then(|r| r.get("n"))
        .and_then(|v| v.as_i64())
        .unwrap_or(-1)
}

fn scalar_string(sql: &str) -> String {
    let rows = duckdb_json(sql);
    rows.first()
        .and_then(|r| r.as_object())
        .and_then(|o| o.values().next())
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

#[test]
fn csv_filter_parquet_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(
        tmp.path(),
        "orders.csv",
        "order_id,status,amount\n1,paid,10\n2,pending,20\n3,paid,30\n4,refunded,5\n",
    );
    let out = out_path(tmp.path(), "paid.parquet");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("f1", "xf.filter", json!({ "predicate": "status = 'paid'" })),
            node("k1", "snk.parquet", json!({ "path": out })),
        ]),
        json!([main_edge("e1", "s1", "f1"), main_edge("e2", "f1", "k1")]),
    );

    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);

    // Sink reports the 2 paid rows written.
    let sink = result.nodes.get("k1").expect("sink status present");
    assert_eq!(sink.rows, Some(2), "sink should report 2 rows");

    // The Parquet file exists and, read back independently, has exactly
    // the 2 paid rows.
    assert!(Path::new(&out).exists(), "parquet file should exist");
    assert_eq!(count(&format!("read_parquet('{}')", out)), 2);

    // And both rows really are 'paid'.
    let bad = count(&format!(
        "read_parquet('{}') WHERE status != 'paid'",
        out
    ));
    assert_eq!(bad, 0, "every output row must be paid");
}

#[test]
fn csv_to_csv_roundtrip_preserves_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(
        tmp.path(),
        "in.csv",
        "id,name\n1,alice\n2,bob\n3,carol\n",
    );
    let out = out_path(tmp.path(), "out.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    assert!(Path::new(&out).exists());
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 3);
}

#[test]
fn aggregate_groups_and_sums() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(
        tmp.path(),
        "sales.csv",
        "region,amount\nwest,10\nwest,20\neast,5\neast,15\neast,5\n",
    );
    let out = out_path(tmp.path(), "agg.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "a1",
                "xf.agg",
                json!({
                    "groupBy": ["region"],
                    "aggregations": [
                        { "column": "amount", "function": "sum", "alias": "total" }
                    ]
                }),
            ),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "a1"), main_edge("e2", "a1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);

    // Two groups out.
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 2);
    // west total = 30.
    let west = scalar_string(&format!(
        "SELECT CAST(total AS VARCHAR) FROM read_csv_auto('{}') WHERE region = 'west'",
        out
    ));
    assert_eq!(west, "30");
}

#[test]
fn preview_returned_for_leaf_without_sink() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "p.csv", "a,b\n1,x\n2,y\n");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("f1", "xf.filter", json!({ "predicate": "a >= 1" })),
        ]),
        json!([main_edge("e1", "s1", "f1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);

    // The leaf (filter) has no downstream sink, so it returns a preview.
    let preview = result
        .preview
        .iter()
        .find(|p| p.node_id == "f1")
        .expect("filter leaf preview present");
    assert_eq!(preview.rows.len(), 2);
    assert_eq!(preview.columns.len(), 2);

    // The filter's view row-count is reported on the node status.
    let f = result.nodes.get("f1").unwrap();
    assert_eq!(f.rows, Some(2));
}

#[test]
fn structured_filter_predicate_actually_filters() {
    // The visual filter builder stores a structured object carrying its
    // compiled SQL - the executor must honor it, not fall back to TRUE.
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(
        tmp.path(),
        "orders.csv",
        "id,status\n1,paid\n2,pending\n3,paid\n",
    );
    let out = out_path(tmp.path(), "filtered.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "f1",
                "xf.filter",
                json!({
                    "predicate": {
                        "mode": "builder",
                        "match": "all",
                        "conditions": [
                            { "id": "c1", "column": "status", "op": "eq", "value": "paid" }
                        ],
                        "sql": "status = 'paid'"
                    }
                }),
            ),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "f1"), main_edge("e2", "f1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // Header + 2 paid rows - NOT all 3 (which is what the WHERE TRUE bug did).
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 2);
}

#[test]
fn aggregate_accepts_func_output_keys() {
    // The UI stores aggregations as { column, func, output }; the
    // executor must accept those spellings (not only function/alias).
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(
        tmp.path(),
        "sales.csv",
        "region,amount\nwest,10\nwest,20\neast,5\n",
    );
    let out = out_path(tmp.path(), "agg.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "a1",
                "xf.agg",
                json!({
                    "groupBy": ["region"],
                    "aggregations": [
                        { "column": "amount", "func": "sum", "output": "total" }
                    ]
                }),
            ),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "a1"), main_edge("e2", "a1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 2);
    let west = scalar_string(&format!(
        "SELECT CAST(total AS VARCHAR) FROM read_csv_auto('{}') WHERE region = 'west'",
        out
    ));
    assert_eq!(west, "30");
}

#[test]
fn custom_sql_runs_with_input_alias() {
    // A Custom-SQL node runs its SELECT as a real stage, with the
    // upstream exposed as `input`.
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "id,amount\n1,10\n2,20\n3,5\n");
    let out = out_path(tmp.path(), "out.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "q1",
                "code.sql",
                json!({ "sql": "SELECT id, amount * 2 AS dbl FROM input WHERE amount >= 10" }),
            ),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "q1"), main_edge("e2", "q1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // Rows with amount >= 10 → ids 1 and 2.
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 2);
    let dbl = scalar_string(&format!(
        "SELECT CAST(dbl AS VARCHAR) FROM read_csv_auto('{}') WHERE id = 1",
        out
    ));
    assert_eq!(dbl, "20");
}

#[test]
fn quality_range_splits_pass_and_reject() {
    // A Range validator must route in-range rows to its main output and
    // out-of-range rows to its reject port (two materialized tables).
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "id,amount\n1,5\n2,50\n3,500\n");
    let pass = out_path(tmp.path(), "pass.csv");
    let rej = out_path(tmp.path(), "reject.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "v1",
                "qa.range",
                json!({ "column": "amount", "min": 10, "max": 100, "inclusive": true }),
            ),
            node("kp", "snk.csv", json!({ "path": pass, "hasHeader": true })),
            node("kr", "snk.csv", json!({ "path": rej, "hasHeader": true })),
        ]),
        json!([
            main_edge("e1", "s1", "v1"),
            port_edge("e2", "v1", "main", "kp"),
            port_edge("e3", "v1", "reject", "kr"),
        ]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // 50 is in [10,100] -> pass; 5 and 500 -> reject.
    assert_eq!(count(&format!("read_csv_auto('{}')", pass)), 1);
    assert_eq!(count(&format!("read_csv_auto('{}')", rej)), 2);
}

#[test]
fn window_row_number_partitions() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "g,v\na,1\na,2\nb,9\n");
    let out = out_path(tmp.path(), "win.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "w1",
                "xf.rownum",
                json!({ "partitionBy": ["g"], "orderBy": ["v"], "outputName": "rn" }),
            ),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "w1"), main_edge("e2", "w1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // Partition 'a' has two rows ranked 1 and 2 by v.
    let max_rn = scalar_string(&format!(
        "SELECT CAST(MAX(rn) AS VARCHAR) FROM read_csv_auto('{}') WHERE g = 'a'",
        out
    ));
    assert_eq!(max_rn, "2");
    let b_rn = scalar_string(&format!(
        "SELECT CAST(rn AS VARCHAR) FROM read_csv_auto('{}') WHERE g = 'b'",
        out
    ));
    assert_eq!(b_rn, "1");
}

#[test]
fn string_case_transforms_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "name\nalice\nbob\n");
    let out = out_path(tmp.path(), "out.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("c1", "xf.case", json!({ "column": "name", "pattern": "upper" })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "c1"), main_edge("e2", "c1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    let first = scalar_string(&format!(
        "SELECT name FROM read_csv_auto('{}') ORDER BY name LIMIT 1",
        out
    ));
    assert_eq!(first, "ALICE");
}

#[test]
fn numeric_round_adds_column() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "v\n3.14159\n");
    let out = out_path(tmp.path(), "out.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "r1",
                "xf.num.round",
                json!({ "column": "v", "argument": 2, "outputColumn": "rounded" }),
            ),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "r1"), main_edge("e2", "r1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    let rounded = scalar_string(&format!(
        "SELECT CAST(rounded AS VARCHAR) FROM read_csv_auto('{}')",
        out
    ));
    assert_eq!(rounded, "3.14");
}

#[test]
fn unimplemented_component_fails_loudly_not_silently() {
    // A not-yet-executable transform must error, not silently pass data
    // through (which would look like success while doing nothing).
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "a\n1\n");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("x1", "xf.transpose", json!({})),
        ]),
        json!([main_edge("e1", "s1", "x1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "error", "unimplemented op should fail, not pass through");
}

#[test]
fn date_diff_computes_days() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "start,end\n2024-01-01,2024-01-11\n");
    let out = out_path(tmp.path(), "out.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "d1",
                "xf.dt.diff",
                json!({ "startColumn": "start", "endColumn": "end", "unit": "day", "outputColumn": "days" }),
            ),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "d1"), main_edge("e2", "d1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    let days = scalar_string(&format!("SELECT CAST(days AS VARCHAR) FROM read_csv_auto('{}')", out));
    assert_eq!(days, "10");
}

#[test]
fn rollup_adds_grand_total() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "region,amount\nwest,10\neast,20\n");
    let out = out_path(tmp.path(), "out.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "a1",
                "xf.rollup",
                json!({
                    "groupBy": ["region"],
                    "aggregations": [{ "column": "amount", "func": "sum", "output": "total" }]
                }),
            ),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "a1"), main_edge("e2", "a1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // 2 region rows + 1 grand-total row (region NULL).
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 3);
    let total = scalar_string(&format!(
        "SELECT CAST(total AS VARCHAR) FROM read_csv_auto('{}') WHERE region IS NULL",
        out
    ));
    assert_eq!(total, "30");
}

#[test]
fn array_collect_groups_into_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "g,v\na,1\na,2\nb,9\n");
    let out = out_path(tmp.path(), "out.csv");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node(
                "c1",
                "xf.arr.collect",
                json!({ "valueColumn": "v", "groupBy": ["g"], "outputColumn": "items" }),
            ),
            node("k1", "snk.json", json!({ "path": out })),
        ]),
        json!([main_edge("e1", "s1", "c1"), main_edge("e2", "c1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    let len_a = scalar_string(&format!(
        "SELECT CAST(len(items) AS VARCHAR) FROM read_json_auto('{}') WHERE g = 'a'",
        out
    ));
    assert_eq!(len_a, "2");
}

// These use the EXACT property keys the UI forms write - the bug was
// the executor reading different keys, so config was silently dropped.

#[test]
fn groupby_form_keys_actually_group() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "region,amount\nwest,10\nwest,20\neast,5\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("g1", "xf.groupby", json!({
                "groupKeys": ["region"],
                "aggregations": [{ "column": "amount", "func": "sum", "output": "total" }]
            })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "g1"), main_edge("e2", "g1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 2);
    let west = scalar_string(&format!(
        "SELECT CAST(total AS VARCHAR) FROM read_csv_auto('{}') WHERE region='west'", out));
    assert_eq!(west, "30");
}

#[test]
fn sort_form_keys_actually_sort() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "n\n3\n1\n2\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("o1", "xf.sort", json!({ "sortColumn": "n", "direction": "asc" })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "o1"), main_edge("e2", "o1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // First row after ascending sort is 1 (read back preserving order).
    let first = scalar_string(&format!(
        "SELECT CAST(n AS VARCHAR) FROM read_csv_auto('{}') LIMIT 1", out));
    assert_eq!(first, "1");
}

#[test]
fn distinct_columns_form_dedups() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "g,v\na,1\na,2\nb,3\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("d1", "xf.distinct", json!({ "columns": ["g"] })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "d1"), main_edge("e2", "d1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 2);
}

#[test]
fn map_expressions_form_computes() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "amount\n100\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("m1", "xf.map", json!({
                "expressions": [{ "key": "doubled", "value": "amount * 2" }]
            })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "m1"), main_edge("e2", "m1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    let v = scalar_string(&format!("SELECT CAST(doubled AS VARCHAR) FROM read_csv_auto('{}')", out));
    assert_eq!(v, "200");
}

#[test]
fn sink_error_mode_refuses_to_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "a\n1\n");
    // Pre-create the output so 'error if exists' should refuse.
    let out = write_file(tmp.path(), "out.csv", "old\n1\n");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true, "mode": "error" })),
        ]),
        json!([main_edge("e1", "s1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "error", "should refuse to overwrite existing file");
}

#[test]
fn addcol_form_adds_computed_column() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "amount\n100\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("a1", "xf.addcol", json!({ "name": "tax", "expression": "amount + 5" })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "a1"), main_edge("e2", "a1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    let tax = scalar_string(&format!("SELECT CAST(tax AS VARCHAR) FROM read_csv_auto('{}')", out));
    assert_eq!(tax, "105", "got tax={}", tax);
}

#[test]
fn rename_mapping_form_renames() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "a,b\n1,2\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("r1", "xf.rename", json!({ "mapping": [{ "key": "a", "value": "x" }] })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "r1"), main_edge("e2", "r1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // Column 'a' is now 'x'; reading 'x' must work and equal 1.
    let x = scalar_string(&format!("SELECT CAST(x AS VARCHAR) FROM read_csv_auto('{}')", out));
    assert_eq!(x, "1", "got x={}", x);
}

#[test]
fn cast_single_column_form_changes_type() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "v\n10.9\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("c1", "xf.cast", json!({ "column": "v", "targetType": "int32" })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "c1"), main_edge("e2", "c1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // 10.9 cast to int -> 11; if the cast were ignored it'd stay 10.9.
    let v = scalar_string(&format!("SELECT CAST(v AS VARCHAR) FROM read_csv_auto('{}')", out));
    assert_eq!(v, "11", "got v={}", v);
}

#[test]
fn duckdb_sink_writes_table() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "id,name\n1,a\n2,b\n");
    let dbfile = out_path(tmp.path(), "out.duckdb");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("k1", "snk.duckdb", json!({ "database": dbfile, "tableName": "people" })),
        ]),
        json!([main_edge("e1", "s1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    let n = scalar_string(&format!(
        "ATTACH '{}' AS d (READ_ONLY); SELECT CAST(count(*) AS VARCHAR) AS n FROM d.people",
        dbfile
    ));
    assert_eq!(n, "2", "got {}", n);
}

#[test]
fn sqlite_sink_writes_table() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "id,name\n1,a\n2,b\n3,c\n");
    let dbfile = out_path(tmp.path(), "out.sqlite");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("k1", "snk.sqlite", json!({ "database": dbfile, "tableName": "people" })),
        ]),
        json!([main_edge("e1", "s1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    let n = scalar_string(&format!(
        "ATTACH '{}' AS s (TYPE SQLITE); SELECT CAST(count(*) AS VARCHAR) AS n FROM s.people",
        dbfile
    ));
    assert_eq!(n, "3", "got {}", n);
}

#[test]
fn duckdb_source_reads_table() {
    let engine = engine_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let srcdb = out_path(tmp.path(), "src.duckdb");
    duckdb_exec(
        &srcdb,
        "CREATE TABLE orders AS SELECT * FROM (VALUES (1,'paid'),(2,'pending'),(3,'paid')) t(id,status)",
    );
    let out = out_path(tmp.path(), "out.csv");
    let d = doc(
        json!([
            node("s1", "src.duckdb", json!({ "database": srcdb, "tableName": "orders" })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 3);
}

#[test]
fn window_aggregate_keeps_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "g,amt\na,10\na,20\nb,5\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("w1", "xf.aggwin", json!({
                "function": "sum", "column": "amt", "partitionBy": ["g"], "outputName": "g_total"
            })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "w1"), main_edge("e2", "w1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // All 3 rows kept; group 'a' rows carry the partition total 30.
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 3);
    let total = scalar_string(&format!(
        "SELECT CAST(g_total AS VARCHAR) FROM read_csv_auto('{}') WHERE g = 'a' LIMIT 1",
        out
    ));
    assert_eq!(total, "30", "got {}", total);
}

#[test]
fn unpivot_wide_to_long() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(tmp.path(), "in.csv", "id,q1,q2\n1,10,20\n");
    let out = out_path(tmp.path(), "out.csv");
    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("u1", "xf.unpivot", json!({
                "columns": ["q1", "q2"], "nameColumn": "quarter", "valueColumn": "amount"
            })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([main_edge("e1", "s1", "u1"), main_edge("e2", "u1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // One input row, two unpivoted columns -> two output rows.
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 2);
    let q1 = scalar_string(&format!(
        "SELECT CAST(amount AS VARCHAR) FROM read_csv_auto('{}') WHERE quarter = 'q1'",
        out
    ));
    assert_eq!(q1, "10", "got {}", q1);
}

#[test]
fn cdc_diff_detect_tags_changes() {
    let engine = engine_or_skip!();
    let tmp = tempfile::tempdir().unwrap();
    let cur = write_file(tmp.path(), "cur.csv", "id,v\n1,a\n2,b2\n3,c\n");
    let prev = write_file(tmp.path(), "prev.csv", "id,v\n1,a\n2,b\n4,d\n");
    let out = out_path(tmp.path(), "out.csv");
    let d = doc(
        json!([
            node("c1", "src.csv", json!({ "path": cur, "hasHeader": true })),
            node("p1", "src.csv", json!({ "path": prev, "hasHeader": true })),
            node("d1", "xf.cdc.diff", json!({
                "naturalKey": ["id"], "compareColumns": ["v"], "rejectUnchanged": true
            })),
            node("k1", "snk.csv", json!({ "path": out, "hasHeader": true })),
        ]),
        json!([
            main_edge("e1", "c1", "d1"),
            lookup_edge("e2", "p1", "d1"),
            main_edge("e3", "d1", "k1"),
        ]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);
    // id=1 unchanged is dropped -> 3 rows: updated(2), inserted(3), deleted(4).
    assert_eq!(count(&format!("read_csv_auto('{}')", out)), 3);
    let t2 = scalar_string(&format!(
        "SELECT change_type FROM read_csv_auto('{}') WHERE id = 2",
        out
    ));
    assert_eq!(t2, "updated", "got {}", t2);
    let t4 = scalar_string(&format!(
        "SELECT change_type FROM read_csv_auto('{}') WHERE id = 4",
        out
    ));
    assert_eq!(t4, "deleted", "got {}", t4);
}

#[test]
fn missing_source_file_errors_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let out = out_path(tmp.path(), "never.parquet");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node(
                "s1",
                "src.csv",
                json!({ "path": "/no/such/file/orders.csv", "hasHeader": true }),
            ),
            node("k1", "snk.parquet", json!({ "path": out })),
        ]),
        json!([main_edge("e1", "s1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "error");
    assert!(result.error.is_some(), "an error message should be present");
    // No output file should have been created.
    assert!(!Path::new(&out).exists());
}

#[test]
fn project_and_rename_reshape_columns() {
    let tmp = tempfile::tempdir().unwrap();
    let csv = write_file(
        tmp.path(),
        "wide.csv",
        "id,first,last,age\n1,ada,lovelace,36\n2,alan,turing,41\n",
    );
    let out = out_path(tmp.path(), "narrow.parquet");

    let engine = engine_or_skip!();
    let d = doc(
        json!([
            node("s1", "src.csv", json!({ "path": csv, "hasHeader": true })),
            node("p1", "xf.project", json!({ "columns": ["id", "first"] })),
            node("k1", "snk.parquet", json!({ "path": out })),
        ]),
        json!([main_edge("e1", "s1", "p1"), main_edge("e2", "p1", "k1")]),
    );
    let result = engine.execute_pipeline(&d);
    assert_eq!(result.status, "ok", "run failed: {:?}", result.error);

    // Output has 2 rows and exactly 2 columns (id, first).
    assert_eq!(count(&format!("read_parquet('{}')", out)), 2);
    // DESCRIBE returns one row per column.
    let cols = count(&format!(
        "(DESCRIBE SELECT * FROM read_parquet('{}'))",
        out
    ));
    assert_eq!(cols, 2, "should have projected to 2 columns");
}
