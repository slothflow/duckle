//! End-to-end execution tests for the DuckDB engine.
//!
//! Unlike the unit tests in `src/`, which check SQL *generation*, these
//! exercise the real read → transform → write path against temp files
//! and then read the output back to prove the data actually landed.

use duckle_duckdb_engine::{DuckdbEngine, PipelineDoc};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

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

/// Read a scalar count from any DuckDB FROM-clause using a fresh
/// connection, so we're verifying the output file independently of the
/// engine that wrote it.
fn count(from: &str) -> i64 {
    let conn = duckdb::Connection::open_in_memory().unwrap();
    conn.query_row(&format!("SELECT COUNT(*) FROM {}", from), [], |r| r.get(0))
        .unwrap()
}

fn scalar_string(sql: &str) -> String {
    let conn = duckdb::Connection::open_in_memory().unwrap();
    conn.query_row(sql, [], |r| r.get::<usize, String>(0)).unwrap()
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

    let engine = DuckdbEngine::new().unwrap();
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

    let engine = DuckdbEngine::new().unwrap();
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

    let engine = DuckdbEngine::new().unwrap();
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

    let engine = DuckdbEngine::new().unwrap();
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
fn missing_source_file_errors_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let out = out_path(tmp.path(), "never.parquet");

    let engine = DuckdbEngine::new().unwrap();
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

    let engine = DuckdbEngine::new().unwrap();
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
