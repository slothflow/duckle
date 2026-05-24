//! Pipeline → DuckDB SQL compiler.
//!
//! Lowers a Duckle pipeline document (the same JSON the frontend
//! produces) into an ordered list of SQL statements. Each non-sink node
//! becomes a `CREATE OR REPLACE TEMP VIEW "<node_id>" AS (...)` so
//! downstream nodes can reference it by name. Sinks become standalone
//! `COPY (...) TO '...' (FORMAT ...)` statements.

use crate::sql_escape;
use crate::EngineError;
use duckle_metadata::{PipelineEdge, PipelineNode};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Pipeline payload sent from the frontend. Just the nodes + edges
/// directly - no wrapping metadata required for a run.
#[derive(Debug, Deserialize)]
pub struct PipelineDoc {
    pub nodes: Vec<PipelineNode>,
    #[serde(default)]
    pub edges: Vec<PipelineEdge>,
}

#[derive(Debug)]
pub struct Stage {
    pub node_id: String,
    pub component_id: String,
    pub label: String,
    pub sql: String,
    pub kind: StageKind,
    /// For sinks: the upstream object name they read from, so the
    /// executor can report a row count.
    pub from: Option<String>,
    /// For sinks: the output path + write mode, so the executor can
    /// enforce "error if exists" before writing.
    pub sink_path: Option<String>,
    pub sink_mode: Option<String>,
    /// For relational-DB sinks in upsert mode: the planner can't
    /// enumerate the upstream's non-key columns up front, so it leaves
    /// `sql` empty and the executor introspects the materialized
    /// upstream (DESCRIBE) before assembling the final INSERT ... ON
    /// CONFLICT statement.
    pub upsert: Option<UpsertSpec>,
    /// For xf.ai.text_search: in DuckDB v1.5.x the fts PRAGMA can't see
    /// tables created in the same -c invocation. The planner records
    /// the spec; the executor runs two CLI calls (stage then index +
    /// query) so the PRAGMA sees committed state. Works unchanged on
    /// v1.4 too.
    pub text_search: Option<TextSearchSpec>,
    /// HTTP per-row sink (snk.webhook / snk.rest). When set, the
    /// executor materializes the upstream view and dispatches requests
    /// via ureq; stage SQL is empty (no DuckDB write).
    pub webhook: Option<WebhookSpec>,
    /// Milliseconds the executor sleeps before running this stage.
    /// Set by ctl.wait and ctl.throttle. None = no delay.
    pub wait_ms: Option<u64>,
    /// Advanced-settings retry: total attempts (1 = no retry). The
    /// executor sleeps `retry_backoff_ms` (with linear scaling) between
    /// attempts and only retries on engine errors, not on cancellation.
    pub retry_attempts: u32,
    pub retry_backoff_ms: u64,
    /// PRAGMA memory_limit prepended to the stage SQL when set. Lets a
    /// user cap a heavy aggregation without touching the whole pipeline.
    pub memory_limit_mb: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TextSearchSpec {
    pub from_view: String,
    pub id_col: String,
    pub text_cols: Vec<String>,
    pub query: String,
    pub top_k: Option<u64>,
    pub output_col: String,
    /// Sanitized staging table name (so PRAGMA can reference a valid
    /// SQL identifier even when the node id has special characters).
    pub staging_table: String,
}

/// snk.webhook / snk.rest / vendor HTTP sinks: one HTTP POST/PUT
/// per row, or a single batched request whose body is the entire
/// result as a JSON array or NDJSON bulk doc set. ureq keeps the
/// per-stage CLI shape we already use; no tokio required.
#[derive(Debug, Clone)]
pub struct WebhookSpec {
    pub from_view: String,
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    /// Body shape:
    ///   'row'         - one POST per row, body = row JSON
    ///   'batch'       - single POST, body = entire result as JSON array
    ///   'ndjson_bulk' - single POST, NDJSON pairs (action + doc per row)
    ///                   for Elasticsearch / OpenSearch bulk APIs.
    pub body_shape: String,
    /// Optional batch-mode wrap: when set, the array body is wrapped
    /// in {body_wrap: [...]} so vendors like Pinecone ('vectors'),
    /// Qdrant ('points'), or Weaviate ('objects') get the shape they
    /// expect without the user hand-building the JSON.
    pub body_wrap: Option<String>,
    /// Extra static fields injected into the wrapped object alongside
    /// the array. Used by Milvus ({collectionName: ..., data: [...]})
    /// and other vendors whose body has metadata + the array side by
    /// side.
    pub body_extras: Vec<(String, serde_json::Value)>,
    /// NDJSON bulk only: the action line emitted before each row.
    /// E.g. `{"index":{"_index":"docs"}}` for Elasticsearch bulk.
    pub bulk_action: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpsertSpec {
    pub family: UpsertFamily,
    /// INSTALL/LOAD/ATTACH preamble; ends with a trailing space.
    pub attach: String,
    /// Fully qualified target inside the ATTACHed DB
    /// (e.g. `duckle_dst."public"."orders"`).
    pub target: String,
    /// The upstream materialized table name in the temp DB.
    pub from_view: String,
    /// Columns the user declared as the conflict key.
    pub conflict_cols: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum UpsertFamily {
    /// `ON CONFLICT (key) DO UPDATE SET col = EXCLUDED.col` (Postgres, Cockroach).
    Postgres,
    /// `ON DUPLICATE KEY UPDATE col = VALUES(col)` (MySQL, MariaDB).
    MySql,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StageKind {
    /// Non-sink node - emitted as a `CREATE OR REPLACE TEMP VIEW`.
    View,
    /// Sink - emitted as a `COPY (...) TO '...' (FORMAT ...)`.
    Sink,
}

#[derive(Debug)]
pub struct CompiledPipeline {
    pub stages: Vec<Stage>,
    /// Node IDs that have no downstream consumer - used to fetch
    /// preview rows when there's no sink.
    pub leaves: Vec<String>,
}

/// Compile only the subgraph upstream of (and including) `target_id`.
/// Sinks downstream of the target are dropped - the target becomes the
/// new "leaf" whose preview the caller can fetch. Used by the
/// "Run from here" right-click action.
pub fn compile_partial(
    pipeline: &PipelineDoc,
    target_id: &str,
) -> Result<CompiledPipeline, EngineError> {
    // Make sure the target actually exists.
    if !pipeline.nodes.iter().any(|n| n.id == target_id) {
        return Err(EngineError::Config(format!(
            "Target node '{}' not found",
            target_id
        )));
    }
    // BFS backwards from target along data edges.
    let mut keep: std::collections::HashSet<String> = std::collections::HashSet::new();
    keep.insert(target_id.to_string());
    let mut frontier = vec![target_id.to_string()];
    while let Some(id) = frontier.pop() {
        for edge in pipeline.edges.iter().filter(|e| is_data_edge(e) && e.target == id) {
            if keep.insert(edge.source.clone()) {
                frontier.push(edge.source.clone());
            }
        }
    }
    let filtered = PipelineDoc {
        nodes: pipeline
            .nodes
            .iter()
            .filter(|n| keep.contains(&n.id))
            .cloned()
            .collect(),
        edges: pipeline
            .edges
            .iter()
            .filter(|e| keep.contains(&e.source) && keep.contains(&e.target))
            .cloned()
            .collect(),
    };
    compile(&filtered)
}

pub fn compile(pipeline: &PipelineDoc) -> Result<CompiledPipeline, EngineError> {
    let node_index: HashMap<&str, &PipelineNode> = pipeline
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();

    let data_edges: Vec<&PipelineEdge> = pipeline
        .edges
        .iter()
        .filter(|e| is_data_edge(e))
        .collect();

    let order = topological_sort(&pipeline.nodes, &data_edges)?;

    // Build inputs map: node_id -> port_id -> Vec<source_node_id>
    let mut inputs: HashMap<&str, NodeInputs> = HashMap::new();
    for edge in &data_edges {
        let port = edge
            .target_handle
            .as_deref()
            .unwrap_or("main");
        let port_key = canonical_port(port);
        // Resolve which materialized table this edge actually reads, based
        // on the SOURCE node's output handle (main vs reject).
        let source_ref = output_table_ref(&edge.source, edge.source_handle.as_deref());
        inputs
            .entry(edge.target.as_str())
            .or_default()
            .ports
            .entry(port_key.to_string())
            .or_default()
            .push(source_ref);
    }

    let mut stages = Vec::with_capacity(order.len());
    for node_id in &order {
        let node = node_index
            .get(node_id.as_str())
            .ok_or_else(|| EngineError::Config(format!("Unknown node: {}", node_id)))?;
        let component_id = node
            .data
            .component_id
            .as_deref()
            .ok_or_else(|| {
                EngineError::Config(format!(
                    "Node '{}' has no componentId; can't execute",
                    node_id
                ))
            })?;
        if node.data.disabled.unwrap_or(false) {
            continue;
        }
        let empty = NodeInputs::default();
        let node_inputs = inputs.get(node_id.as_str()).unwrap_or(&empty);
        let stage = build_stage(node, component_id, node_inputs)?;
        stages.push(stage);
    }

    // Leaves = data-flow nodes that nothing else consumes from
    let has_downstream: HashSet<&str> = data_edges.iter().map(|e| e.source.as_str()).collect();
    let leaves: Vec<String> = order
        .iter()
        .filter(|id| !has_downstream.contains(id.as_str()))
        .cloned()
        .collect();

    Ok(CompiledPipeline { stages, leaves })
}

#[derive(Debug, Default)]
struct NodeInputs {
    /// canonical port -> ordered list of upstream node ids.
    ports: BTreeMap<String, Vec<String>>,
}

impl NodeInputs {
    fn main(&self) -> Option<&str> {
        self.ports.get("main").and_then(|v| v.first()).map(|s| s.as_str())
    }

    /// Inputs across the `main` and `main_N` ports (used by set ops,
    /// whose handles are main_1 / main_2 / main_3).
    fn all_main_ports(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for (key, refs) in &self.ports {
            if key == "main" || key.starts_with("main_") {
                out.extend(refs.iter().map(|s| s.as_str()));
            }
        }
        out
    }

    #[allow(dead_code)]
    fn lookup(&self, idx: usize) -> Option<&str> {
        let key = if idx == 0 {
            "lookup".to_string()
        } else {
            format!("lookup_{}", idx + 1)
        };
        self.ports.get(&key).and_then(|v| v.first()).map(|s| s.as_str())
    }

    fn first_lookup(&self) -> Option<&str> {
        for (k, v) in &self.ports {
            if k.starts_with("lookup") {
                if let Some(first) = v.first() {
                    return Some(first.as_str());
                }
            }
        }
        None
    }
}

/// Suffix for a node's secondary "reject" output table.
const REJECT_SUFFIX: &str = "__reject";

/// Which materialized table an edge reads, based on the source node's
/// OUTPUT handle. Reject/filter outputs read the node's `__reject`
/// table; everything else reads its main table.
fn output_table_ref(source_id: &str, source_handle: Option<&str>) -> String {
    match source_handle.map(canonical_port) {
        Some("reject") | Some("filter") => format!("{}{}", source_id, REJECT_SUFFIX),
        // Switch / conditional split: each case + default port reads
        // from its own `<node>__<handle>` table that build_switch
        // materializes.
        Some(h) if h.starts_with("case_") || h == "default" => {
            format!("{}__{}", source_id, h)
        }
        _ => source_id.to_string(),
    }
}

fn canonical_port(p: &str) -> &str {
    // Collapse port handle ids to canonical names. The frontend uses
    // 'main', 'lookup_1', 'lookup_2', 'lookup_3', 'reject', 'filter',
    // 'iterate'. Triggers don't carry data so we never see them here.
    if p.is_empty() {
        return "main";
    }
    p
}

fn is_data_edge(edge: &PipelineEdge) -> bool {
    match edge.data.as_ref() {
        Some(d) => matches!(
            d.connection_type.as_str(),
            "main" | "lookup" | "reject" | "filter"
        ),
        None => true,
    }
}

fn topological_sort(
    nodes: &[PipelineNode],
    edges: &[&PipelineEdge],
) -> Result<Vec<String>, EngineError> {
    let mut in_degree: HashMap<String, usize> =
        nodes.iter().map(|n| (n.id.clone(), 0_usize)).collect();
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for edge in edges {
        if !in_degree.contains_key(&edge.source) || !in_degree.contains_key(&edge.target) {
            continue;
        }
        adjacency
            .entry(edge.source.clone())
            .or_default()
            .push(edge.target.clone());
        *in_degree.entry(edge.target.clone()).or_insert(0) += 1;
    }
    let mut queue: Vec<String> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(k, _)| k.clone())
        .collect();
    // Stabilize order so generated SQL is reproducible.
    queue.sort();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(id) = queue.pop() {
        order.push(id.clone());
        if let Some(children) = adjacency.get(&id) {
            for child in children {
                let entry = in_degree.entry(child.clone()).or_insert(0);
                if *entry > 0 {
                    *entry -= 1;
                    if *entry == 0 {
                        queue.push(child.clone());
                        queue.sort();
                    }
                }
            }
        }
    }
    if order.len() != nodes.len() {
        return Err(EngineError::Config(
            "Pipeline contains a cycle in the data-flow edges".into(),
        ));
    }
    Ok(order)
}

fn build_stage(
    node: &PipelineNode,
    component_id: &str,
    inputs: &NodeInputs,
) -> Result<Stage, EngineError> {
    let props = node
        .data
        .properties
        .as_ref()
        .cloned()
        .unwrap_or(JsonValue::Null);
    let mut sink_path: Option<String> = None;
    let mut sink_mode: Option<String> = None;
    let mut upsert: Option<UpsertSpec> = None;
    let mut text_search: Option<TextSearchSpec> = None;
    let mut webhook: Option<WebhookSpec> = None;
    let mut wait_ms: Option<u64> = None;
    // Advanced settings (universal across components, written by the
    // Properties Panel's Advanced tab). Engine honours them per stage.
    let retry_attempts = props
        .get("retryAttempts")
        .and_then(|v| v.as_u64())
        .map(|n| n.max(1) as u32)
        .unwrap_or(1);
    let retry_backoff_ms = props
        .get("retryBackoffMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let memory_limit_mb = props
        .get("memoryLimitMb")
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0)
        .map(|n| n as u32);
    // ATTACH statements for external-DB nodes (DuckDB/SQLite). Each stage
    // runs in its own CLI process, so fixed aliases are collision-free.
    let attach = attach_prelude(component_id, &props);
    let (sql, kind, from) = if component_id == "snk.webhook" || component_id == "snk.rest" {
        // HTTP sink. Stage SQL stays empty; the executor materializes
        // the upstream view, then dispatches one ureq request per row
        // (body_shape='row') or one batched request (body_shape='batch').
        let from_view = inputs
            .main()
            .ok_or_else(|| missing_input(node, "main"))?;
        let url = string_prop(&props, "url")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: url required", component_id)))?;
        let method = string_prop(&props, "method")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "POST".into())
            .to_uppercase();
        // Prefer bodyShape (engine-native), fall back to batchMode
        // (form-native): 'one' -> per-row, 'array' -> batched.
        let body_shape = string_prop(&props, "bodyShape")
            .filter(|s| !s.is_empty())
            .or_else(|| {
                string_prop(&props, "batchMode").map(|m| match m.as_str() {
                    "array" => "batch".into(),
                    _ => "row".into(),
                })
            })
            .unwrap_or_else(|| if component_id == "snk.webhook" { "row".into() } else { "batch".into() });
        let mut headers = headers_from_props(&props);
        // Translate the form's authType + authToken into a header so
        // the executor doesn't need to know about auth shapes.
        let auth_type = string_prop(&props, "authType").unwrap_or_else(|| "none".into());
        let auth_token = string_prop(&props, "authToken").unwrap_or_default();
        if !auth_token.is_empty() {
            match auth_type.as_str() {
                "bearer" => headers.push((
                    "Authorization".into(),
                    format!("Bearer {}", auth_token),
                )),
                "apikey" => headers.push(("X-API-Key".into(), auth_token)),
                _ => {}
            }
        }
        let body_wrap = string_prop(&props, "bodyWrap").filter(|s| !s.is_empty());
        webhook = Some(WebhookSpec {
            from_view: from_view.to_string(),
            url,
            method,
            headers,
            body_shape,
            body_wrap,
            body_extras: Vec::new(),
            bulk_action: None,
        });
        (String::new(), StageKind::Sink, Some(from_view.to_string()))
    } else if component_id == "snk.pinecone" {
        // Pinecone vector upsert. Form fields: indexHost (e.g.
        // 'idx-abc123.svc.us-east1-gcp.pinecone.io'), apiKey, vectorColumn,
        // idColumn. The engine builds the {vectors: [...]} body that the
        // /vectors/upsert endpoint expects and sets the Api-Key header.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let host = string_prop(&props, "indexHost")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: indexHost required (e.g. 'idx-abc123.svc.us-east1-gcp.pinecone.io')", component_id)))?;
        let api_key = string_prop(&props, "apiKey").unwrap_or_default();
        let url = format!("https://{}/vectors/upsert", host.trim_start_matches("https://"));
        let mut headers = headers_from_props(&props);
        if !api_key.is_empty() {
            headers.push(("Api-Key".into(), api_key));
        }
        webhook = Some(WebhookSpec {
            from_view: from_view.to_string(),
            url,
            method: "POST".into(),
            headers,
            body_shape: "batch".into(),
            body_wrap: Some("vectors".into()),
            body_extras: Vec::new(),
            bulk_action: None,
        });
        (String::new(), StageKind::Sink, Some(from_view.to_string()))
    } else if component_id == "snk.qdrant" {
        // Qdrant points upsert. Form fields: clusterUrl (e.g.
        // 'https://xyz-east1.aws.cloud.qdrant.io:6333'), collection,
        // apiKey. Body shape: {points: [...]}; upsert is PUT to
        // /collections/{collection}/points.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let cluster = string_prop(&props, "clusterUrl")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: clusterUrl required", component_id)))?;
        let collection = string_prop(&props, "collection")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: collection required", component_id)))?;
        let api_key = string_prop(&props, "apiKey").unwrap_or_default();
        let url = format!(
            "{}/collections/{}/points",
            cluster.trim_end_matches('/'),
            collection
        );
        let mut headers = headers_from_props(&props);
        if !api_key.is_empty() {
            headers.push(("api-key".into(), api_key));
        }
        webhook = Some(WebhookSpec {
            from_view: from_view.to_string(),
            url,
            method: "PUT".into(),
            headers,
            body_shape: "batch".into(),
            body_wrap: Some("points".into()),
            body_extras: Vec::new(),
            bulk_action: None,
        });
        (String::new(), StageKind::Sink, Some(from_view.to_string()))
    } else if component_id == "snk.weaviate" {
        // Weaviate batch objects endpoint:
        //   POST {endpoint}/v1/batch/objects
        //   { "objects": [ { class, properties, vector }, ... ] }
        // Auth via Bearer token (apiKey) when supplied.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let endpoint = string_prop(&props, "endpoint")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: endpoint required (e.g. 'https://my-cluster.weaviate.network')", component_id)))?;
        let api_key = string_prop(&props, "apiKey").unwrap_or_default();
        let url = format!("{}/v1/batch/objects", endpoint.trim_end_matches('/'));
        let mut headers = headers_from_props(&props);
        if !api_key.is_empty() {
            headers.push(("Authorization".into(), format!("Bearer {}", api_key)));
        }
        webhook = Some(WebhookSpec {
            from_view: from_view.to_string(),
            url,
            method: "POST".into(),
            headers,
            body_shape: "batch".into(),
            body_wrap: Some("objects".into()),
            body_extras: Vec::new(),
            bulk_action: None,
        });
        (String::new(), StageKind::Sink, Some(from_view.to_string()))
    } else if component_id == "snk.milvus" {
        // Milvus REST insert:
        //   POST {endpoint}/v1/vector/insert
        //   { "collectionName": "...", "data": [ {id, vector, ...}, ... ] }
        // body_extras puts the collectionName next to data.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let endpoint = string_prop(&props, "endpoint")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: endpoint required", component_id)))?;
        let collection = string_prop(&props, "collection")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: collection required", component_id)))?;
        let api_key = string_prop(&props, "apiKey").unwrap_or_default();
        let url = format!("{}/v1/vector/insert", endpoint.trim_end_matches('/'));
        let mut headers = headers_from_props(&props);
        if !api_key.is_empty() {
            headers.push(("Authorization".into(), format!("Bearer {}", api_key)));
        }
        webhook = Some(WebhookSpec {
            from_view: from_view.to_string(),
            url,
            method: "POST".into(),
            headers,
            body_shape: "batch".into(),
            body_wrap: Some("data".into()),
            body_extras: vec![(
                "collectionName".into(),
                serde_json::Value::String(collection),
            )],
            bulk_action: None,
        });
        (String::new(), StageKind::Sink, Some(from_view.to_string()))
    } else if component_id == "snk.elastic" || component_id == "snk.opensearch" {
        // Elasticsearch / OpenSearch bulk API:
        //   POST {host}/{index}/_bulk
        //   action_line\n
        //   document_line\n
        //   ... (repeated, NDJSON, no trailing comma)
        // Content-Type: application/x-ndjson.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let host = string_prop(&props, "endpoint")
            .or_else(|| string_prop(&props, "host"))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: endpoint required", component_id)))?;
        let index = string_prop(&props, "index")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: index required", component_id)))?;
        let api_key = string_prop(&props, "apiKey").unwrap_or_default();
        let url = format!("{}/_bulk", host.trim_end_matches('/'));
        let mut headers = headers_from_props(&props);
        headers.push(("Content-Type".into(), "application/x-ndjson".into()));
        if !api_key.is_empty() {
            headers.push(("Authorization".into(), format!("ApiKey {}", api_key)));
        }
        // index action template: {"index": {"_index": "<index>"}}
        let action_line = format!("{{\"index\":{{\"_index\":\"{}\"}}}}", index.replace('"', "\\\""));
        webhook = Some(WebhookSpec {
            from_view: from_view.to_string(),
            url,
            method: "POST".into(),
            headers,
            body_shape: "ndjson_bulk".into(),
            body_wrap: None,
            body_extras: Vec::new(),
            bulk_action: Some(action_line),
        });
        (String::new(), StageKind::Sink, Some(from_view.to_string()))
    } else if component_id.starts_with("snk.") {
        let from_view = inputs
            .main()
            .ok_or_else(|| missing_input(node, "main"))?;
        sink_path = string_prop(&props, "path").filter(|s| !s.is_empty());
        sink_mode = string_prop(&props, "mode").filter(|s| !s.is_empty());
        // Relational DB upsert is the only sink mode whose SQL the
        // planner can't fully generate up front: the SET clause needs
        // the upstream's non-key column list, which the executor reads
        // via DESCRIBE before assembling the final INSERT.
        if sink_mode.as_deref() == Some("upsert")
            && matches!(
                component_id,
                "snk.postgres" | "snk.cockroach" | "snk.mysql" | "snk.mariadb"
            )
        {
            let conflict_cols = columns_list(&props, "conflictColumns");
            if conflict_cols.is_empty() {
                return Err(EngineError::Config(format!(
                    "{}: upsert mode needs at least one column in Conflict columns",
                    component_id
                )));
            }
            let table = string_prop(&props, "tableName")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    EngineError::Config(format!("{}: table name is required", component_id))
                })?;
            let schema = string_prop(&props, "schemaName").filter(|s| !s.is_empty());
            let target = relational_qualified(
                "duckle_dst",
                component_id,
                schema.as_deref(),
                &table,
            );
            let family = if component_id == "snk.postgres" || component_id == "snk.cockroach" {
                UpsertFamily::Postgres
            } else {
                UpsertFamily::MySql
            };
            upsert = Some(UpsertSpec {
                family,
                attach: attach.clone(),
                target,
                from_view: from_view.to_string(),
                conflict_cols,
            });
            (String::new(), StageKind::Sink, Some(from_view.to_string()))
        } else {
            (
                format!("{}{}", attach, build_sink_sql(component_id, &props, from_view)?),
                StageKind::Sink,
                Some(from_view.to_string()),
            )
        }
    } else if component_id == "ctl.wait" {
        // Pass-through view. Engine sleeps wait_ms before running the SQL.
        // Form writes { duration: int, unit: 'milliseconds'|'seconds'|'minutes'|'hours' }.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let dur = props.get("duration").and_then(|v| v.as_u64()).unwrap_or(0);
        let unit = string_prop(&props, "unit").unwrap_or_else(|| "seconds".into());
        let ms = match unit.as_str() {
            "milliseconds" | "ms" => dur,
            "minutes" => dur.saturating_mul(60_000),
            "hours" => dur.saturating_mul(3_600_000),
            _ => dur.saturating_mul(1_000),
        };
        if ms > 0 {
            wait_ms = Some(ms);
        }
        let sql = format!(
            "CREATE OR REPLACE TABLE {} AS SELECT * FROM {}",
            quote_ident(&node.id),
            quote_ident(from_view)
        );
        (sql, StageKind::View, None)
    } else if component_id == "ctl.throttle" {
        // Same shape as ctl.wait - applies an inter-stage delay derived
        // from the requested rows-per-second. Marginal for batch
        // workloads but the hook is in place for streaming.
        // Form writes { rate: int (rows/sec) }.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let rps = props
            .get("rate")
            .and_then(|v| v.as_f64())
            .or_else(|| props.get("rowsPerSecond").and_then(|v| v.as_f64()))
            .unwrap_or(0.0);
        if rps > 0.0 {
            wait_ms = Some((1000.0 / rps).max(1.0) as u64);
        }
        let sql = format!(
            "CREATE OR REPLACE TABLE {} AS SELECT * FROM {}",
            quote_ident(&node.id),
            quote_ident(from_view)
        );
        (sql, StageKind::View, None)
    } else if component_id == "ctl.checkpoint" {
        // Pass-through view + a sidecar parquet write. The temp DB the
        // executor uses goes away after the pipeline; the parquet is
        // the durable artifact a user can read back into a future run.
        // Form writes { name, storage }.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let path = string_prop(&props, "storage")
            .or_else(|| string_prop(&props, "path"))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: checkpoint storage path required", component_id)))?;
        let sql = format!(
            "CREATE OR REPLACE TABLE {} AS SELECT * FROM {}; COPY (SELECT * FROM {}) TO '{}' (FORMAT PARQUET)",
            quote_ident(&node.id),
            quote_ident(from_view),
            quote_ident(&node.id),
            sql_escape(&path)
        );
        (sql, StageKind::View, None)
    } else if component_id == "ctl.deadletter" {
        // Terminal sink for rejected rows. Same shape as snk.parquet /
        // snk.csv / snk.json - write the upstream to a file.
        // Form writes { destination: path, format: 'json'|'csv'|'parquet' }.
        let from_view = inputs.main().ok_or_else(|| missing_input(node, "main"))?;
        let path = string_prop(&props, "destination")
            .or_else(|| string_prop(&props, "path"))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EngineError::Config(format!("{}: dead letter destination required", component_id)))?;
        let format = string_prop(&props, "format").unwrap_or_else(|| "json".into());
        sink_path = Some(path.clone());
        sink_mode = string_prop(&props, "mode").filter(|s| !s.is_empty());
        let copy = match format.as_str() {
            "csv" => format!(
                "COPY (SELECT * FROM {}) TO '{}' (FORMAT CSV, HEADER true)",
                quote_ident(from_view),
                sql_escape(&path)
            ),
            "parquet" => format!(
                "COPY (SELECT * FROM {}) TO '{}' (FORMAT PARQUET, COMPRESSION 'ZSTD')",
                quote_ident(from_view),
                sql_escape(&path)
            ),
            _ => format!(
                "COPY (SELECT * FROM {}) TO '{}' (FORMAT JSON, ARRAY false)",
                quote_ident(from_view),
                sql_escape(&path)
            ),
        };
        (copy, StageKind::Sink, Some(from_view.to_string()))
    } else if component_id == "ctl.switch" {
        // Switch materializes one table per case + default; it has no
        // main output table, so the count_rows fallback in the executor
        // (which would target node.id) just returns None for it.
        let sql = build_switch(&node.id, inputs, &props).map_err(|e| {
            EngineError::Config(format!("{} ({} / {}): {}", node.data.label, component_id, node.id, e))
        })?;
        (format!("{}{}", attach, sql), StageKind::View, None)
    } else if component_id == "xf.ai.text_search" {
        // Full-Text Search runs as a two-step path in the executor (the
        // v1.5 fts PRAGMA can't see tables created in the same -c
        // invocation). The planner records the spec; sql stays empty.
        let spec = build_text_search_spec(&node.id, inputs, &props).map_err(|e| {
            EngineError::Config(format!("{} ({} / {}): {}", node.data.label, component_id, node.id, e))
        })?;
        text_search = Some(spec);
        (String::new(), StageKind::View, None)
    } else {
        let body = build_view_sql(component_id, &props, inputs).map_err(|e| {
            EngineError::Config(format!("{} ({} / {}): {}", node.data.label, component_id, node.id, e))
        })?;
        // Materialize as a real table so the result persists across the
        // separate CLI invocations the executor uses per stage.
        let mut sql = format!(
            "{}CREATE OR REPLACE TABLE {} AS {}",
            attach,
            quote_ident(&node.id),
            body
        );
        // Components that split rows (filter, quality validators) also
        // materialize a `<node>__reject` table for their reject port.
        if let Some(reject_body) = build_reject_sql(component_id, &props, inputs).map_err(|e| {
            EngineError::Config(format!("{} ({} / {}): {}", node.data.label, component_id, node.id, e))
        })? {
            let reject_table = format!("{}{}", node.id, REJECT_SUFFIX);
            sql.push_str(&format!(
                "; CREATE OR REPLACE TABLE {} AS {}",
                quote_ident(&reject_table),
                reject_body
            ));
        }
        (sql, StageKind::View, None)
    };
    Ok(Stage {
        node_id: node.id.clone(),
        component_id: component_id.to_string(),
        label: node.data.label.clone(),
        sql,
        kind,
        from,
        sink_path,
        sink_mode,
        upsert,
        text_search,
        webhook,
        wait_ms,
        retry_attempts,
        retry_backoff_ms,
        memory_limit_mb,
    })
}

/// The `SELECT * FROM <reader>` SQL for a source format - used by the
/// engine's inspect path to DESCRIBE / sample without materializing.
pub fn source_select_for_format(format: &str, props: &JsonValue) -> Option<String> {
    Some(match format {
        "csv" => build_csv_source(props),
        "tsv" => build_tsv_source(props),
        "parquet" => build_parquet_source(props),
        "json" | "jsonl" | "ndjson" => build_json_source(props),
        "sqlite" => build_sqlite_source(props),
        "duckdb" => build_duckdb_source(props),
        "s3" | "gcs" | "azureblob" | "http" | "https" => build_cloud_source(format, props),
        _ => return None,
    })
}

fn missing_input(node: &PipelineNode, port: &str) -> EngineError {
    EngineError::Config(format!(
        "{} ({}) is missing its '{}' input",
        node.data.label, node.id, port
    ))
}

// ---- View SQL (sources + transforms) ------------------------------------

fn build_view_sql(
    component_id: &str,
    props: &JsonValue,
    inputs: &NodeInputs,
) -> Result<String, String> {
    match component_id {
        // Sources
        "src.csv" => Ok(build_csv_source(props)),
        "src.tsv" => Ok(build_tsv_source(props)),
        "src.parquet" => Ok(build_parquet_source(props)),
        "src.json" | "src.jsonl" => Ok(build_json_source(props)),
        "src.sqlite" => Ok(build_sqlite_source(props)),
        "src.duckdb" => Ok(build_duckdb_source(props)),
        "src.s3" | "src.gcs" | "src.azureblob" | "src.http"
        | "src.minio" | "src.r2" | "src.b2" => {
            // MinIO / R2 / B2 are S3-compatible; the endpoint lives in
            // the SECRET created by the runtime, so the URL itself is
            // just s3://bucket/key.
            let s = component_id.strip_prefix("src.").unwrap_or(component_id);
            let scheme = if matches!(s, "minio" | "r2" | "b2") { "s3" } else { s };
            Ok(build_cloud_source(scheme, props))
        }
        "src.postgres" | "src.cockroach" | "src.mysql" | "src.mariadb"
        | "src.motherduck" | "src.ducklake" | "src.pgvector"
        | "src.redshift" | "src.bigquery" => build_relational_source(component_id, props),
        "src.avro" => Ok(build_avro_source(props)),
        "src.excel" => Ok(build_excel_source(props)),
        "src.iceberg" => Ok(build_iceberg_source(props)),
        "src.delta" => Ok(build_delta_source(props)),
        "src.spatial" => Ok(build_spatial_source(props)),
        // Pass-through transforms
        "xf.filter" => build_filter(inputs, props),
        // Log Rows - pass data through unchanged; its rows surface in the
        // Output / Preview so you can inspect mid-pipeline (like tLogRow).
        "xf.log" => build_passthrough_op(inputs, "SELECT *"),
        "xf.project" => build_project(inputs, props),
        "xf.distinct" => build_distinct(inputs, props),
        "xf.limit" => build_limit(inputs, props),
        "xf.sort" => build_sort(inputs, props),
        "xf.agg" | "xf.groupby" => build_aggregate(inputs, props, GroupMode::Plain),
        "xf.approx.quantile" => build_approx_quantile(inputs, props),
        "xf.rollup" => build_aggregate(inputs, props, GroupMode::Rollup),
        "xf.cube" => build_aggregate(inputs, props, GroupMode::Cube),
        "xf.aggwin" => build_window_aggregate(inputs, props),
        "xf.union" => build_union(inputs, true),
        "xf.unionall" => build_union(inputs, false),
        "xf.intersect" => build_setop(inputs, "INTERSECT"),
        "xf.except" => build_setop(inputs, "EXCEPT"),
        "xf.addcol" | "xf.coalesce" => build_addcol(inputs, props),
        "xf.rownum" | "xf.rank" | "xf.denserank" | "xf.lead" | "xf.lag" | "xf.first"
        | "xf.last" | "xf.ntile" => build_window(inputs, props, component_id),
        "xf.pivot" => build_pivot(inputs, props),
        "xf.unpivot" => build_unpivot(inputs, props),
        "xf.denorm" => build_denormalize(inputs, props),
        "xf.norm" => build_normalize(inputs, props),
        "xf.transpose" => build_transpose(inputs),
        "xf.cdc.diff" => build_cdc_diff(inputs, props),
        "xf.cdc.scd2" => build_scd2(inputs, props),
        "xf.cdc.scd1" => build_scd1(inputs, props),
        "xf.cdc.upsert" => build_upsert(inputs, props),
        "xf.ai.vector_search" => build_vector_search(inputs, props),
        // Data-quality validators - the PASS rows. Failures go to the
        // node's __reject table (see build_reject_sql).
        "qa.notnull" | "qa.range" | "qa.regex" | "qa.unique" | "qa.schemavalidate" => {
            build_quality(inputs, props, component_id, false)
        }
        "qa.profile" => build_profile(inputs, props),
        "qa.describe" => build_describe(inputs),
        "qa.histogram" => build_histogram(inputs, props),
        "qa.standardize" => build_standardize(inputs, props),
        "qa.dedupe" => build_fuzzy_dedupe(inputs, props),
        "qa.match" => build_record_match(inputs, props),
        "xf.reorder" => build_reorder(inputs, props),
        "xf.count" => build_count(inputs),
        "xf.join.cross" => build_cross_join(inputs),
        "xf.join.spatial" => build_spatial_join(inputs, props),
        "xf.regex" | "xf.regex.extract" | "xf.regex.match" | "xf.trim" | "xf.case"
        | "xf.length" | "xf.substring" | "xf.concat" | "xf.split" | "xf.format" => {
            build_string(inputs, props, component_id)
        }
        "xf.url.parse" => build_url_parse(inputs, props),
        "xf.assert" => build_assert(inputs, props),
        "xf.hash" => build_hash(inputs, props),
        "xf.ip.parse" => build_ip_parse(inputs, props),
        "xf.geo.distance" => build_geo_distance(inputs, props),
        "xf.geo.buffer" => build_geo_buffer(inputs, props),
        "xf.geo.intersects" => build_geo_intersects(inputs, props),
        "xf.num.round" | "xf.num.abs" | "xf.num.mod" | "xf.num.power" | "xf.num.sqrt"
        | "xf.num.log" => build_numeric(inputs, props, component_id),
        "xf.num.bucketize" => build_bucketize(inputs, props),
        "xf.num.zscore" => build_zscore(inputs, props),
        "xf.num.clamp" => build_clamp(inputs, props),
        "xf.num.sign" => build_sign(inputs, props),
        "xf.rank.filter" => build_rank_filter(inputs, props),
        "xf.fill_forward" => build_fill_forward(inputs, props),
        "xf.cumulative" => build_cumulative(inputs, props),
        "xf.dt.bin" => build_dt_bin(inputs, props),
        "xf.arr.length" => build_arr_length(inputs, props),
        "xf.uuid" => build_uuid(inputs, props),
        "xf.dt.parse" | "xf.dt.format" | "xf.dt.extract" | "xf.dt.trunc" | "xf.dt.tz" => {
            build_datetime(inputs, props, component_id)
        }
        "xf.dt.add" => build_date_add(inputs, props),
        "xf.dt.diff" => build_date_diff(inputs, props),
        "xf.dt.now" => build_dt_now(inputs, props),
        "xf.dt.epoch" => build_dt_epoch(inputs, props),
        "xf.json.parse" | "xf.json.stringify" | "xf.json.path" => {
            build_json(inputs, props, component_id)
        }
        "xf.json.flatten" => build_json_flatten(inputs, props),
        "xf.json.merge" => build_json_merge(inputs, props),
        "xf.json.array_agg" => build_json_array_agg(inputs, props),
        "xf.text.similarity" => build_text_similarity(inputs, props),
        "xf.text.base64" => build_base64(inputs, props),
        "xf.text.padding" => build_padding(inputs, props),
        "xf.text.match" => build_text_match(inputs, props),
        "xf.text.reverse" => build_text_reverse(inputs, props),
        "xf.text.repeat" => build_text_repeat(inputs, props),
        "xf.compare" => build_compare(inputs, props),
        "xf.arr.element" | "xf.arr.distinct" | "xf.arr.explode" => {
            build_array(inputs, props, component_id)
        }
        "xf.arr.collect" => build_arr_collect(inputs, props),
        "xf.arr.contains" => build_arr_contains(inputs, props),
        "xf.cast" => build_cast(inputs, props),
        "xf.rename" => build_rename(inputs, props),
        "xf.drop" | "xf.dropcol" => build_drop(inputs, props),
        "xf.map" => build_mapper(inputs, props),
        "xf.join.inner" | "xf.join" => build_join(inputs, props, "INNER"),
        "xf.join.left" => build_join(inputs, props, "LEFT"),
        "xf.join.right" => build_join(inputs, props, "RIGHT"),
        "xf.join.full" | "xf.join.outer" => build_join(inputs, props, "FULL OUTER"),
        "xf.lookup" | "xf.lookup.outer" => build_join(inputs, props, "LEFT"),
        "xf.semi" | "xf.semi.join" => build_semi(inputs, props, false),
        "xf.anti" | "xf.anti.join" => build_semi(inputs, props, true),
        "xf.topn" => build_take(inputs, props, TakeKind::Limit),
        "xf.skip" => build_take(inputs, props, TakeKind::Offset),
        "xf.sample" => build_take(inputs, props, TakeKind::Sample),
        // Custom SQL - runs the user's SELECT as a real stage, with the
        // upstream exposed as `input`. Makes SQL routines executable too.
        "code.sql" | "code.sqltemplate" => build_custom_sql(inputs, props),
        // Routing: replicate is a passthrough (the graph already lets
        // multiple downstream edges read the same materialized table);
        // merge concatenates multiple input streams with UNION ALL.
        "ctl.replicate" => {
            let upstream = inputs.main().ok_or_else(|| missing_input_msg("ctl.replicate"))?;
            Ok(format!("SELECT * FROM {}", quote_ident(upstream)))
        }
        "ctl.merge" => build_union(inputs, false),
        // Everything else isn't executable yet. Fail loudly rather than
        // silently passing data through unchanged (which would look like
        // success while doing nothing).
        other => Err(format!(
            "'{}' isn't executable on the DuckDB engine yet - it's a preview component.",
            other
        )),
    }
}

fn build_passthrough_op(inputs: &NodeInputs, op: &str) -> Result<String, String> {
    let upstream = inputs
        .main()
        .ok_or_else(|| "missing main input".to_string())?;
    Ok(format!("{} FROM {}", op, quote_ident(upstream)))
}

fn build_filter(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    // The predicate is usually a structured object carrying compiled
    // `sql`; it may also be a raw string (legacy / raw-SQL mode).
    let predicate = filter_predicate_sql(props.get("predicate"))
        .or_else(|| {
            props
                .get("filterSql")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    let predicate = predicate.trim();
    let predicate = if predicate.is_empty() { "TRUE" } else { predicate };
    Ok(format!(
        "SELECT * FROM {} WHERE {}",
        quote_ident(upstream),
        predicate
    ))
}

/// Extract the effective SQL from a filter predicate value, which may be
/// a plain string or the structured FilterPredicate object the visual
/// builder writes ({ mode, conditions, rawSql, sql }).
fn filter_predicate_sql(v: Option<&JsonValue>) -> Option<String> {
    match v {
        Some(JsonValue::String(s)) => Some(s.clone()),
        Some(JsonValue::Object(o)) => o
            .get("sql")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                if o.get("mode").and_then(JsonValue::as_str) == Some("raw") {
                    o.get("rawSql").and_then(JsonValue::as_str).map(str::to_string)
                } else {
                    None
                }
            }),
        _ => None,
    }
}

fn build_project(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let columns = columns_from_props(props, "columns").or_else(|| columns_from_props(props, "keep"));
    let cols = match columns {
        Some(cs) if !cs.is_empty() => cs
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", "),
        _ => "*".to_string(),
    };
    Ok(format!("SELECT {} FROM {}", cols, quote_ident(upstream)))
}

fn build_drop(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let columns = columns_from_props(props, "columns")
        .or_else(|| columns_from_props(props, "drop"))
        .unwrap_or_default();
    if columns.is_empty() {
        return Ok(format!("SELECT * FROM {}", quote_ident(upstream)));
    }
    let except_list = columns
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "SELECT * EXCLUDE ({}) FROM {}",
        except_list,
        quote_ident(upstream)
    ))
}

fn build_limit(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let limit = props
        .get("limit")
        .and_then(JsonValue::as_u64)
        .or_else(|| props.get("rows").and_then(JsonValue::as_u64))
        .unwrap_or(100);
    Ok(format!(
        "SELECT * FROM {} LIMIT {}",
        quote_ident(upstream),
        limit
    ))
}

enum TakeKind {
    Limit,
    Offset,
    Sample,
}

fn build_take(inputs: &NodeInputs, props: &JsonValue, kind: TakeKind) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let n = props
        .get("count")
        .and_then(JsonValue::as_u64)
        .or_else(|| props.get("limit").and_then(JsonValue::as_u64))
        .unwrap_or(100);
    let from = quote_ident(upstream);
    Ok(match kind {
        TakeKind::Limit => format!("SELECT * FROM {} LIMIT {}", from, n),
        TakeKind::Offset => format!("SELECT * FROM {} OFFSET {}", from, n),
        TakeKind::Sample => format!("SELECT * FROM {} USING SAMPLE {} ROWS", from, n),
    })
}

/// Custom SQL stage. The upstream table is exposed as a CTE named
/// `input`, so a node's SQL like `SELECT * FROM input WHERE x > 1`
/// just works. With no upstream, the SQL stands alone (e.g. a source
/// SELECT). build_stage wraps the result in CREATE OR REPLACE TABLE.
fn build_custom_sql(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let sql = string_prop(props, "sql")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Custom SQL is empty - write a SELECT or pick a SQL routine".to_string())?;
    Ok(match inputs.main() {
        Some(upstream) => {
            format!("WITH input AS (SELECT * FROM {}) {}", quote_ident(upstream), sql)
        }
        None => sql,
    })
}

fn build_distinct(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let cols = columns_list(props, "columns");
    if cols.is_empty() {
        Ok(format!("SELECT DISTINCT * FROM {}", quote_ident(upstream)))
    } else {
        let on = cols.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ");
        Ok(format!(
            "SELECT DISTINCT ON ({}) * FROM {}",
            on,
            quote_ident(upstream)
        ))
    }
}

fn build_sort(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let sort_keys: Vec<String> = props
        .get("orderBy")
        .and_then(JsonValue::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    if let Some(s) = v.as_str() {
                        Some(s.to_string())
                    } else if let Some(obj) = v.as_object() {
                        let col = obj.get("column").and_then(JsonValue::as_str)?;
                        let dir = obj
                            .get("direction")
                            .and_then(JsonValue::as_str)
                            .unwrap_or("asc");
                        Some(format!("{} {}", quote_ident(col), dir.to_uppercase()))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let mut sort_keys = sort_keys;
    // The Sort form writes a single sortColumn + direction + nullsLast.
    if sort_keys.is_empty() {
        if let Some(col) = string_prop(props, "sortColumn").filter(|s| !s.is_empty()) {
            let dir = if string_prop(props, "direction").as_deref() == Some("desc") {
                "DESC"
            } else {
                "ASC"
            };
            let nulls = if props.get("nullsLast").and_then(JsonValue::as_bool).unwrap_or(true) {
                " NULLS LAST"
            } else {
                " NULLS FIRST"
            };
            sort_keys.push(format!("{} {}{}", quote_ident(&col), dir, nulls));
        }
    }
    if sort_keys.is_empty() {
        return Ok(format!("SELECT * FROM {}", quote_ident(upstream)));
    }
    Ok(format!(
        "SELECT * FROM {} ORDER BY {}",
        quote_ident(upstream),
        sort_keys.join(", ")
    ))
}

enum GroupMode {
    Plain,
    Rollup,
    Cube,
}

fn build_aggregate(
    inputs: &NodeInputs,
    props: &JsonValue,
    mode: GroupMode,
) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    // The Group By form writes `groupKeys`; accept `groupBy` too.
    let group_by: Vec<String> = columns_from_props(props, "groupKeys")
        .or_else(|| columns_from_props(props, "groupBy"))
        .unwrap_or_default();
    let aggregations = props
        .get("aggregations")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let mut select_terms: Vec<String> = group_by.iter().map(|c| quote_ident(c)).collect();
    for agg in &aggregations {
        let column = agg.get("column").and_then(JsonValue::as_str).unwrap_or("*");
        // The UI's AggregationsField stores { column, func, output };
        // accept the function/alias spellings too for robustness.
        let func = agg
            .get("function")
            .or_else(|| agg.get("func"))
            .and_then(JsonValue::as_str)
            .unwrap_or("count")
            .to_uppercase();
        let alias = agg
            .get("alias")
            .or_else(|| agg.get("output"))
            .and_then(JsonValue::as_str)
            .map(String::from)
            .unwrap_or_else(|| format!("{}_{}", func.to_lowercase(), column.replace('*', "all")));
        let column_expr = if column == "*" {
            "*".to_string()
        } else {
            quote_ident(column)
        };
        let agg_expr = match func.as_str() {
            "COUNT_DISTINCT" => format!("COUNT(DISTINCT {})", column_expr),
            "APPROX_COUNT_DISTINCT" => format!("approx_count_distinct({})", column_expr),
            _ => format!("{}({})", func, column_expr),
        };
        select_terms.push(format!("{} AS {}", agg_expr, quote_ident(&alias)));
    }
    if select_terms.is_empty() {
        select_terms.push("COUNT(*) AS row_count".to_string());
    }
    let group_clause = if group_by.is_empty() {
        String::new()
    } else {
        let cols = group_by
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        match mode {
            GroupMode::Plain => format!(" GROUP BY {}", cols),
            GroupMode::Rollup => format!(" GROUP BY ROLLUP ({})", cols),
            GroupMode::Cube => format!(" GROUP BY CUBE ({})", cols),
        }
    };
    let having = string_prop(props, "havingClause")
        .or_else(|| string_prop(props, "having"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|h| format!(" HAVING {}", h))
        .unwrap_or_default();
    Ok(format!(
        "SELECT {} FROM {}{}{}",
        select_terms.join(", "),
        quote_ident(upstream),
        group_clause,
        having
    ))
}

fn interval_unit(unit: &str) -> &'static str {
    match unit.to_lowercase().as_str() {
        "year" | "years" => "YEAR",
        "quarter" | "quarters" => "QUARTER",
        "month" | "months" => "MONTH",
        "week" | "weeks" => "WEEK",
        "hour" | "hours" => "HOUR",
        "minute" | "minutes" => "MINUTE",
        "second" | "seconds" => "SECOND",
        _ => "DAY",
    }
}

fn build_date_add(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.dt.add"))?;
    let column = require_column(props)?;
    let amount = props.get("amount").and_then(JsonValue::as_i64).unwrap_or(1);
    let unit = string_prop(props, "unit").unwrap_or_else(|| "day".into());
    // amount * INTERVAL 1 unit handles negatives cleanly.
    let expr = format!(
        "{} + ({} * INTERVAL 1 {})",
        quote_ident(&column),
        amount,
        interval_unit(&unit)
    );
    Ok(apply_col_expr(upstream, &column, expr, string_prop(props, "outputColumn")))
}

fn build_date_diff(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.dt.diff"))?;
    let start = string_prop(props, "startColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Date diff needs a start column".to_string())?;
    let end = string_prop(props, "endColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Date diff needs an end column".to_string())?;
    let unit = string_prop(props, "unit").unwrap_or_else(|| "day".into());
    let out = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "date_diff".into());
    Ok(format!(
        "SELECT *, date_diff('{}', {}, {}) AS {} FROM {}",
        sql_escape(&unit),
        quote_ident(&start),
        quote_ident(&end),
        quote_ident(&out),
        quote_ident(upstream)
    ))
}

fn build_json_flatten(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.json.flatten"))?;
    let column = require_column(props)?;
    let col = quote_ident(&column);
    // Expand a STRUCT column's fields to top-level columns.
    Ok(format!(
        "SELECT * EXCLUDE ({}), {}.* FROM {}",
        col,
        col,
        quote_ident(upstream)
    ))
}

fn build_json_merge(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.json.merge"))?;
    let a = require_column(props)?;
    let b = string_prop(props, "secondColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Merge needs a second column".to_string())?;
    let out = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "merged".into());
    Ok(format!(
        "SELECT *, json_merge_patch(CAST({} AS JSON), CAST({} AS JSON)) AS {} FROM {}",
        quote_ident(&a),
        quote_ident(&b),
        quote_ident(&out),
        quote_ident(upstream)
    ))
}

fn build_arr_collect(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.arr.collect"))?;
    let value = string_prop(props, "valueColumn")
        .or_else(|| string_prop(props, "column"))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Collect needs a value column".to_string())?;
    let out = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "items".into());
    let group = columns_list(props, "groupBy");
    if group.is_empty() {
        Ok(format!(
            "SELECT list({}) AS {} FROM {}",
            quote_ident(&value),
            quote_ident(&out),
            quote_ident(upstream)
        ))
    } else {
        let g = group.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ");
        Ok(format!(
            "SELECT {}, list({}) AS {} FROM {} GROUP BY {}",
            g,
            quote_ident(&value),
            quote_ident(&out),
            quote_ident(upstream),
            g
        ))
    }
}

fn build_arr_contains(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.arr.contains"))?;
    let column = require_column(props)?;
    let value = string_prop(props, "value").unwrap_or_default();
    let lit = if value.trim().parse::<f64>().is_ok() {
        value.trim().to_string()
    } else {
        format!("'{}'", sql_escape(&value))
    };
    let expr = format!("list_contains({}, {})", quote_ident(&column), lit);
    let out = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_contains", column));
    Ok(format!(
        "SELECT *, {} AS {} FROM {}",
        expr,
        quote_ident(&out),
        quote_ident(upstream)
    ))
}

fn build_union(inputs: &NodeInputs, distinct: bool) -> Result<String, String> {
    let mains = inputs.all_main_ports();
    if mains.is_empty() {
        return Err("Union needs at least one input".into());
    }
    let op = if distinct { " UNION " } else { " UNION ALL " };
    Ok(mains
        .iter()
        .map(|id| format!("SELECT * FROM {}", quote_ident(id)))
        .collect::<Vec<_>>()
        .join(op))
}

fn build_setop(inputs: &NodeInputs, op: &str) -> Result<String, String> {
    let mains = inputs.all_main_ports();
    if mains.len() < 2 {
        return Err(format!("{} needs two inputs", op));
    }
    let sep = format!(" {} ", op);
    Ok(mains
        .iter()
        .map(|id| format!("SELECT * FROM {}", quote_ident(id)))
        .collect::<Vec<_>>()
        .join(&sep))
}

fn build_window(
    inputs: &NodeInputs,
    props: &JsonValue,
    component_id: &str,
) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "window: missing main input".to_string())?;
    let func = string_prop(props, "function")
        .unwrap_or_else(|| component_id.rsplit('.').next().unwrap_or("rownum").to_string());
    let target = string_prop(props, "targetColumn").filter(|s| !s.is_empty());
    let offset = props.get("offset").and_then(JsonValue::as_u64).unwrap_or(1);
    let need_target = |f: &str| -> Result<String, String> {
        target
            .clone()
            .map(|c| quote_ident(&c))
            .ok_or_else(|| format!("Window function '{}' needs a target column", f))
    };
    let call = match func.as_str() {
        "rownum" => "ROW_NUMBER()".to_string(),
        "rank" => "RANK()".to_string(),
        "denserank" => "DENSE_RANK()".to_string(),
        "lead" => format!("LEAD({}, {})", need_target("lead")?, offset),
        "lag" => format!("LAG({}, {})", need_target("lag")?, offset),
        "first" => format!("FIRST_VALUE({})", need_target("first")?),
        "last" => format!("LAST_VALUE({})", need_target("last")?),
        "ntile" => format!("NTILE({})", offset.max(1)),
        other => return Err(format!("Unknown window function '{}'", other)),
    };
    let partition = columns_list(props, "partitionBy");
    let order = columns_list(props, "orderBy");
    let mut over = String::new();
    if !partition.is_empty() {
        over.push_str(&format!(
            "PARTITION BY {}",
            partition.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
        ));
    }
    if !order.is_empty() {
        if !over.is_empty() {
            over.push(' ');
        }
        over.push_str(&format!(
            "ORDER BY {}",
            order.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
        ));
    }
    let out_name = string_prop(props, "outputName")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| func.clone());
    Ok(format!(
        "SELECT *, {} OVER ({}) AS {} FROM {}",
        call,
        over,
        quote_ident(&out_name),
        quote_ident(upstream)
    ))
}

fn build_pivot(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "pivot: missing main input".to_string())?;
    let pivot_col = string_prop(props, "pivotColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Pivot needs a pivot column".to_string())?;
    let value_col = string_prop(props, "valueColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Pivot needs a value column".to_string())?;
    let agg = string_prop(props, "aggregation").unwrap_or_else(|| "sum".into());
    let mut sql = format!(
        "PIVOT (SELECT * FROM {}) ON {} USING {}({})",
        quote_ident(upstream),
        quote_ident(&pivot_col),
        agg,
        quote_ident(&value_col)
    );
    let group = columns_list(props, "groupBy");
    if !group.is_empty() {
        sql.push_str(&format!(
            " GROUP BY {}",
            group.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(sql)
}

fn missing_input_msg(component: &str) -> String {
    format!("{} is missing its input connection", component)
}

/// Emit a per-row column expression: add it as `output` if given, else
/// replace the source column in place.
fn apply_col_expr(upstream: &str, column: &str, expr: String, output: Option<String>) -> String {
    match output.filter(|s| !s.trim().is_empty()) {
        Some(out) => format!(
            "SELECT *, {} AS {} FROM {}",
            expr,
            quote_ident(out.trim()),
            quote_ident(upstream)
        ),
        None => format!(
            "SELECT * REPLACE ({} AS {}) FROM {}",
            expr,
            quote_ident(column),
            quote_ident(upstream)
        ),
    }
}

fn require_column(props: &JsonValue) -> Result<String, String> {
    string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "This transform needs a column".to_string())
}

fn build_string(inputs: &NodeInputs, props: &JsonValue, component_id: &str) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg(component_id))?;
    let column = require_column(props)?;
    let col = quote_ident(&column);
    let pattern = string_prop(props, "pattern").unwrap_or_default();
    let replacement = string_prop(props, "replacement").unwrap_or_default();
    let expr = match component_id {
        "xf.regex" => format!(
            "regexp_replace(CAST({} AS VARCHAR), '{}', '{}', 'g')",
            col,
            sql_escape(&pattern),
            sql_escape(&replacement)
        ),
        "xf.regex.extract" => {
            let group_idx = props
                .get("groupIndex")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .max(0);
            format!(
                "regexp_extract(CAST({} AS VARCHAR), '{}', {})",
                col,
                sql_escape(&pattern),
                group_idx
            )
        }
        "xf.regex.match" => format!(
            "regexp_matches(CAST({} AS VARCHAR), '{}')",
            col,
            sql_escape(&pattern)
        ),
        "xf.trim" => format!("trim(CAST({} AS VARCHAR))", col),
        "xf.case" => match pattern.to_lowercase().as_str() {
            "lower" => format!("lower(CAST({} AS VARCHAR))", col),
            "title" | "initcap" | "proper" => format!("initcap(CAST({} AS VARCHAR))", col),
            _ => format!("upper(CAST({} AS VARCHAR))", col),
        },
        "xf.length" => format!("length(CAST({} AS VARCHAR))", col),
        "xf.substring" => {
            let start = pattern.trim().parse::<i64>().unwrap_or(1).max(1);
            match replacement.trim().parse::<i64>() {
                Ok(len) => format!("substring(CAST({} AS VARCHAR), {}, {})", col, start, len),
                Err(_) => format!("substring(CAST({} AS VARCHAR), {})", col, start),
            }
        }
        "xf.concat" => format!("concat(CAST({} AS VARCHAR), '{}')", col, sql_escape(&pattern)),
        "xf.split" => format!("string_split(CAST({} AS VARCHAR), '{}')", col, sql_escape(&pattern)),
        "xf.format" => format!("printf('{}', {})", sql_escape(&pattern), col),
        other => return Err(format!("String op '{}' is not implemented", other)),
    };
    Ok(apply_col_expr(upstream, &column, expr, string_prop(props, "outputColumn")))
}

fn build_numeric(inputs: &NodeInputs, props: &JsonValue, component_id: &str) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg(component_id))?;
    let column = require_column(props)?;
    let col = quote_ident(&column);
    let arg = num_prop(props, "argument");
    let expr = match component_id {
        "xf.num.round" => format!("round({}, {})", col, arg.unwrap_or_else(|| "0".into())),
        "xf.num.abs" => format!("abs({})", col),
        "xf.num.mod" => format!("{} % {}", col, arg.ok_or("Modulo needs a divisor argument")?),
        "xf.num.power" => format!("power({}, {})", col, arg.unwrap_or_else(|| "2".into())),
        "xf.num.sqrt" => format!("sqrt({})", col),
        "xf.num.log" => match arg {
            Some(base) => format!("log({}, {})", base, col),
            None => format!("ln({})", col),
        },
        other => return Err(format!("Numeric op '{}' is not implemented", other)),
    };
    Ok(apply_col_expr(upstream, &column, expr, string_prop(props, "outputColumn")))
}

fn build_datetime(inputs: &NodeInputs, props: &JsonValue, component_id: &str) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg(component_id))?;
    let column = require_column(props)?;
    let col = quote_ident(&column);
    let fmt = string_prop(props, "format").unwrap_or_else(|| "%Y-%m-%d".into());
    let unit = string_prop(props, "unit").unwrap_or_else(|| "day".into());
    let tz = string_prop(props, "timezone").unwrap_or_default();
    let expr = match component_id {
        "xf.dt.parse" => format!("strptime(CAST({} AS VARCHAR), '{}')", col, sql_escape(&fmt)),
        "xf.dt.format" => format!("strftime({}, '{}')", col, sql_escape(&fmt)),
        "xf.dt.extract" => format!("date_part('{}', {})", sql_escape(&unit), col),
        "xf.dt.trunc" => format!("date_trunc('{}', {})", sql_escape(&unit), col),
        "xf.dt.tz" => {
            if tz.is_empty() {
                return Err("Timezone convert needs a timezone".into());
            }
            format!("{} AT TIME ZONE '{}'", col, sql_escape(&tz))
        }
        other => return Err(format!("Date/time op '{}' is not implemented", other)),
    };
    Ok(apply_col_expr(upstream, &column, expr, string_prop(props, "outputColumn")))
}

fn build_json(inputs: &NodeInputs, props: &JsonValue, component_id: &str) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg(component_id))?;
    let column = require_column(props)?;
    let col = quote_ident(&column);
    let path = string_prop(props, "path").unwrap_or_default();
    let expr = match component_id {
        "xf.json.parse" => format!("CAST({} AS JSON)", col),
        "xf.json.stringify" => format!("CAST({} AS VARCHAR)", col),
        "xf.json.path" => {
            if path.is_empty() {
                return Err("JSONPath extract needs a path".into());
            }
            format!("json_extract({}, '{}')", col, sql_escape(&path))
        }
        other => return Err(format!("JSON op '{}' is not implemented", other)),
    };
    Ok(apply_col_expr(upstream, &column, expr, string_prop(props, "outputColumn")))
}

fn build_array(inputs: &NodeInputs, props: &JsonValue, component_id: &str) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg(component_id))?;
    let column = require_column(props)?;
    let col = quote_ident(&column);
    if component_id == "xf.arr.explode" {
        // One row per element, keeping the other columns.
        return Ok(format!(
            "SELECT unnest({}) AS {}, * EXCLUDE ({}) FROM {}",
            col,
            col,
            col,
            quote_ident(upstream)
        ));
    }
    let expr = match component_id {
        "xf.arr.element" => {
            let idx = props.get("index").and_then(JsonValue::as_i64).unwrap_or(1);
            format!("{}[{}]", col, idx)
        }
        "xf.arr.distinct" => format!("list_distinct({})", col),
        other => return Err(format!("Array op '{}' is not implemented", other)),
    };
    Ok(apply_col_expr(upstream, &column, expr, string_prop(props, "outputColumn")))
}

fn build_reorder(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.reorder"))?;
    let cols = columns_list(props, "columns");
    if cols.is_empty() {
        return Ok(format!("SELECT * FROM {}", quote_ident(upstream)));
    }
    let listed = cols.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ");
    // Listed columns first, everything else after - never drops a column.
    Ok(format!(
        "SELECT {}, * EXCLUDE ({}) FROM {}",
        listed,
        listed,
        quote_ident(upstream)
    ))
}

fn build_count(inputs: &NodeInputs) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.count"))?;
    Ok(format!("SELECT count(*) AS row_count FROM {}", quote_ident(upstream)))
}

/// Approximate Quantile via DuckDB's t-digest. Single-row aggregate
/// (or one row per group, if `groupBy` is set). Picks `quantile` from
/// 0..1 (default 0.5 = median). approx_quantile uses fixed memory
/// regardless of cardinality, so it's the right tool for "what's the
/// p95 latency over 10B rows" instead of an exact quantile() call
/// that would need to sort the whole input.
fn build_approx_quantile(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.approx.quantile"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Approx Quantile needs a column".to_string())?;
    let q = props.get("quantile").and_then(|v| v.as_f64()).unwrap_or(0.5);
    let q = if (0.0..=1.0).contains(&q) { q } else { 0.5 };
    let group_by: Vec<String> = columns_from_props(props, "groupBy").unwrap_or_default();
    let alias = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_q{}", column, (q * 100.0).round() as i64));
    let select_extra = group_by
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let select = if group_by.is_empty() {
        format!("approx_quantile({}, {}) AS {}", quote_ident(&column), q, quote_ident(&alias))
    } else {
        format!(
            "{}, approx_quantile({}, {}) AS {}",
            select_extra,
            quote_ident(&column),
            q,
            quote_ident(&alias)
        )
    };
    let group_clause = if group_by.is_empty() {
        String::new()
    } else {
        format!(" GROUP BY {}", select_extra)
    };
    Ok(format!(
        "SELECT {} FROM {}{}",
        select,
        quote_ident(upstream),
        group_clause
    ))
}

fn build_cross_join(inputs: &NodeInputs) -> Result<String, String> {
    let left = inputs.main().ok_or_else(|| "Cross join needs a main input".to_string())?;
    let right = inputs
        .first_lookup()
        .ok_or_else(|| "Cross join needs a lookup input".to_string())?;
    Ok(format!(
        "SELECT * FROM {} CROSS JOIN {}",
        quote_ident(left),
        quote_ident(right)
    ))
}

/// Window aggregate: an aggregate computed over a window, keeping every
/// row (unlike Group By, which collapses them).
fn build_window_aggregate(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.aggwin"))?;
    let func = string_prop(props, "function").unwrap_or_else(|| "sum".into()).to_uppercase();
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "*".into());
    let call = if column == "*" {
        format!("{}(*)", func)
    } else {
        format!("{}({})", func, quote_ident(&column))
    };
    let partition = columns_list(props, "partitionBy");
    let order = columns_list(props, "orderBy");
    let mut over = String::new();
    if !partition.is_empty() {
        over.push_str(&format!(
            "PARTITION BY {}",
            partition.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
        ));
    }
    if !order.is_empty() {
        if !over.is_empty() {
            over.push(' ');
        }
        over.push_str(&format!(
            "ORDER BY {}",
            order.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ")
        ));
    }
    let out = string_prop(props, "outputName")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_{}", func.to_lowercase(), column.replace('*', "all")));
    Ok(format!(
        "SELECT *, {} OVER ({}) AS {} FROM {}",
        call,
        over,
        quote_ident(&out),
        quote_ident(upstream)
    ))
}

/// CDC Diff Detect: compare a 'new' input (main) against a 'previous'
/// input (lookup) on a natural key and tag each row inserted / deleted /
/// updated / unchanged. Updates are detected from the compare columns;
/// unchanged rows are dropped unless the user keeps them.
fn build_cdc_diff(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let cur = inputs
        .main()
        .ok_or_else(|| "Diff Detect needs a 'new' input on the main port".to_string())?;
    let prev = inputs.first_lookup().ok_or_else(|| {
        "Diff Detect needs a 'previous' input (connect it to the previous port)".to_string()
    })?;
    let keys = columns_list(props, "naturalKey");
    if keys.is_empty() {
        return Err("Diff Detect needs natural key columns".to_string());
    }
    let compares = columns_list(props, "compareColumns");
    let reject_unchanged = props
        .get("rejectUnchanged")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let coalesced = keys
        .iter()
        .map(|k| {
            let q = quote_ident(k);
            format!("COALESCE(cur.{q}, prev.{q}) AS {q}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let excl = keys
        .iter()
        .map(|k| quote_ident(k))
        .collect::<Vec<_>>()
        .join(", ");
    let join_on = keys
        .iter()
        .map(|k| {
            let q = quote_ident(k);
            format!("cur.{q} = prev.{q}")
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let first_key = quote_ident(&keys[0]);
    let updated = if compares.is_empty() {
        String::new()
    } else {
        let diff = compares
            .iter()
            .map(|c| {
                let q = quote_ident(c);
                format!("cur.{q} IS DISTINCT FROM prev.{q}")
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        format!("WHEN ({diff}) THEN 'updated' ")
    };
    let inner = format!(
        "SELECT {coalesced}, cur.* EXCLUDE ({excl}), \
         CASE WHEN prev.{first_key} IS NULL THEN 'inserted' \
         WHEN cur.{first_key} IS NULL THEN 'deleted' \
         {updated}ELSE 'unchanged' END AS change_type \
         FROM {cur} cur FULL OUTER JOIN {prev} prev ON {join_on}",
        cur = quote_ident(cur),
        prev = quote_ident(prev),
    );
    if reject_unchanged {
        Ok(format!(
            "SELECT * FROM ({inner}) WHERE change_type != 'unchanged'"
        ))
    } else {
        Ok(inner)
    }
}

/// Denormalize: collapse many rows per group into one, joining the
/// chosen columns into a single delimited cell with string_agg.
fn build_denormalize(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.denorm"))?;
    let group_by = columns_list(props, "groupBy");
    if group_by.is_empty() {
        return Err("Denormalize needs group-by columns".to_string());
    }
    let agg_cols = columns_list(props, "aggregateColumns");
    if agg_cols.is_empty() {
        return Err("Denormalize needs columns to aggregate".to_string());
    }
    let sep = string_prop(props, "separator").unwrap_or_else(|| ", ".into());
    let sep_sql = sep.replace('\'', "''");
    let group_list = group_by
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let aggs = agg_cols
        .iter()
        .map(|c| {
            let q = quote_ident(c);
            format!("string_agg(CAST({q} AS VARCHAR), '{sep_sql}') AS {q}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "SELECT {group_list}, {aggs} FROM {} GROUP BY {group_list}",
        quote_ident(upstream)
    ))
}

/// Normalize: explode a delimited string (or array) column into one row
/// per element, keeping the other columns.
fn build_normalize(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.norm"))?;
    let col = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Normalize needs a column to split".to_string())?;
    let q = quote_ident(&col);
    let sep = string_prop(props, "separator").unwrap_or_else(|| ",".into());
    let value_expr = if sep.is_empty() {
        // Empty separator means the column is already an array; just unnest.
        format!("unnest({q})")
    } else {
        let sep_sql = sep.replace('\'', "''");
        format!("unnest(string_split(CAST({q} AS VARCHAR), '{sep_sql}'))")
    };
    Ok(format!(
        "SELECT * EXCLUDE ({q}), {value_expr} AS {q} FROM {}",
        quote_ident(upstream)
    ))
}

/// Transpose: swap the input's rows and columns. The output has one row
/// per original column (named `colname`) and one value column per
/// original row, named `r1`, `r2`, ... The "r" prefix keeps the column
/// names valid identifiers and parsable as a CSV header (a pure-numeric
/// header would not auto-detect). Requires the input's columns to share
/// a compatible type (UNPIVOT cannot mix unrelated types).
fn build_transpose(inputs: &NodeInputs) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.transpose"))?;
    Ok(format!(
        "SELECT * FROM (PIVOT (FROM (SELECT *, \
         'r' || CAST(ROW_NUMBER() OVER () AS VARCHAR) AS _row FROM {up}) \
         UNPIVOT (val FOR colname IN (COLUMNS(* EXCLUDE _row)))) \
         ON _row USING first(val) GROUP BY colname)",
        up = quote_ident(upstream)
    ))
}

/// Switch / Conditional Split. Routes rows to case_1 ... case_N output
/// ports based on the form's `branches` (a key-value of branch name
/// -> boolean SQL expression). First-match-wins: a row that satisfied
/// branch i is excluded from branches i+1..N and from default. Up to
/// 3 cases (matching the fixed port set) plus a default for the
/// remainder. The form's branch object preserves insertion order
/// because the workspace enables serde_json's preserve_order feature.
fn build_switch(node_id: &str, inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("ctl.switch"))?;
    let mut conds: Vec<String> = Vec::new();
    if let Some(obj) = props.get("branches").and_then(|v| v.as_object()) {
        for (_name, val) in obj {
            if let Some(c) = val.as_str().filter(|s| !s.trim().is_empty()) {
                conds.push(c.to_string());
            }
            if conds.len() >= 3 {
                break;
            }
        }
    }
    if conds.is_empty() {
        return Err("Switch needs at least one branch condition".to_string());
    }
    let up = quote_ident(upstream);
    let mut stmts: Vec<String> = Vec::new();
    let mut prior: Vec<String> = Vec::new();
    for (i, cond) in conds.iter().enumerate() {
        let case_table = format!("{}__case_{}", node_id, i + 1);
        let where_clause = if prior.is_empty() {
            format!("({})", cond)
        } else {
            let neg = prior
                .iter()
                .map(|p| format!("NOT ({})", p))
                .collect::<Vec<_>>()
                .join(" AND ");
            format!("({}) AND {}", cond, neg)
        };
        stmts.push(format!(
            "CREATE OR REPLACE TABLE {} AS SELECT * FROM {} WHERE {}",
            quote_ident(&case_table),
            up,
            where_clause
        ));
        prior.push(cond.clone());
    }
    // Default: rows that no branch matched.
    let default_table = format!("{}__default", node_id);
    let default_where = prior
        .iter()
        .map(|p| format!("NOT ({})", p))
        .collect::<Vec<_>>()
        .join(" AND ");
    stmts.push(format!(
        "CREATE OR REPLACE TABLE {} AS SELECT * FROM {} WHERE {}",
        quote_ident(&default_table),
        up,
        default_where
    ));
    Ok(stmts.join("; "))
}

/// SCD Type 1: overwrite-in-place. Output is the resolved current
/// state: every row from `current`, plus rows from `previous` whose
/// key isn't in current (so unrelated history isn't dropped). Both
/// inputs must have the same column schema.
fn build_scd1(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let cur = inputs.main().ok_or_else(|| missing_input_msg("xf.cdc.scd1"))?;
    let prev = inputs.first_lookup().ok_or_else(|| {
        "SCD1 needs a 'previous' input on the lookup port".to_string()
    })?;
    let keys = columns_list(props, "naturalKey");
    if keys.is_empty() {
        return Err("SCD1 needs natural key columns".to_string());
    }
    let key_eq = keys
        .iter()
        .map(|k| {
            let q = quote_ident(k);
            format!("p.{q} = c.{q}")
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    Ok(format!(
        "SELECT * FROM {cur} \
         UNION ALL \
         SELECT * FROM {prev} p WHERE NOT EXISTS (SELECT 1 FROM {cur} c WHERE {key_eq})",
        cur = quote_ident(cur),
        prev = quote_ident(prev),
    ))
}

/// Merge / Upsert: output the delta to write into a target -  the
/// rows in `current` that are either a new key or a changed value.
/// Unchanged rows are skipped (the target already has them). Deletes
/// are NOT emitted; use Diff Detect when you need them.
fn build_upsert(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let cur = inputs.main().ok_or_else(|| missing_input_msg("xf.cdc.upsert"))?;
    let prev = inputs.first_lookup().ok_or_else(|| {
        "Upsert needs a 'previous' input on the lookup port".to_string()
    })?;
    let keys = columns_list(props, "naturalKey");
    if keys.is_empty() {
        return Err("Upsert needs natural key columns".to_string());
    }
    let compares = columns_list(props, "compareColumns");
    let key_eq = keys
        .iter()
        .map(|k| {
            let q = quote_ident(k);
            format!("cur.{q} = p.{q}")
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let first_key = quote_ident(&keys[0]);
    let change_clause = if compares.is_empty() {
        // No compare columns means we only flag new keys; everything
        // already in previous (regardless of value) is skipped.
        String::new()
    } else {
        let cmp_diff = compares
            .iter()
            .map(|c| {
                let q = quote_ident(c);
                format!("cur.{q} IS DISTINCT FROM p.{q}")
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        format!(" OR ({cmp_diff})")
    };
    Ok(format!(
        "SELECT cur.* FROM {cur} cur LEFT JOIN {prev} p ON {key_eq} \
         WHERE p.{first_key} IS NULL{change_clause}",
        cur = quote_ident(cur),
        prev = quote_ident(prev),
    ))
}

/// SCD Type 2: maintain versioned history. Reads `current` on main and
/// `previous` on the lookup port; the previous input must already carry
/// the SCD columns (valid_from, valid_to, is_current) at the end of its
/// schema. Output is the new history table: closed records get their
/// valid_to + is_current updated, unchanged records pass through, and
/// new / changed keys land as fresh current versions. Compare columns
/// drive the change detection.
fn build_scd2(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let cur = inputs.main().ok_or_else(|| missing_input_msg("xf.cdc.scd2"))?;
    let prev = inputs.first_lookup().ok_or_else(|| {
        "SCD2 needs a 'previous' input on the lookup port (the current history table)".to_string()
    })?;
    let keys = columns_list(props, "naturalKey");
    if keys.is_empty() {
        return Err("SCD2 needs natural key columns".to_string());
    }
    let compares = columns_list(props, "compareColumns");
    if compares.is_empty() {
        return Err("SCD2 needs at least one compare column to detect changes".to_string());
    }
    let valid_from = string_prop(props, "validFromColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "valid_from".into());
    let valid_to = string_prop(props, "validToColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "valid_to".into());
    let is_current = string_prop(props, "isCurrentColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "is_current".into());

    let key_eq = keys
        .iter()
        .map(|k| {
            let q = quote_ident(k);
            format!("p.{q} = c.{q}")
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let cmp_diff = compares
        .iter()
        .map(|c| {
            let q = quote_ident(c);
            format!("p.{q} IS DISTINCT FROM c.{q}")
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let cmp_same = compares
        .iter()
        .map(|c| {
            let q = quote_ident(c);
            format!("p.{q} IS NOT DISTINCT FROM c.{q}")
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let first_key = quote_ident(&keys[0]);
    let vf = quote_ident(&valid_from);
    let vt = quote_ident(&valid_to);
    let ic = quote_ident(&is_current);
    let cur_q = quote_ident(cur);
    let prev_q = quote_ident(prev);

    Ok(format!(
        "WITH prev_current AS (SELECT * FROM {prev_q} WHERE {ic}), \
              prev_history AS (SELECT * FROM {prev_q} WHERE NOT {ic}), \
              to_close AS (SELECT p.* FROM prev_current p LEFT JOIN {cur_q} c ON {key_eq} \
                           WHERE c.{first_key} IS NULL OR ({cmp_diff})), \
              to_keep AS (SELECT p.* FROM prev_current p INNER JOIN {cur_q} c ON {key_eq} \
                          WHERE {cmp_same}), \
              to_insert AS (SELECT c.* FROM {cur_q} c LEFT JOIN prev_current p ON {key_eq} \
                            WHERE p.{first_key} IS NULL OR ({cmp_diff})) \
         SELECT * FROM prev_history \
         UNION ALL SELECT * FROM to_keep \
         UNION ALL SELECT * REPLACE (CURRENT_TIMESTAMP AS {vt}, FALSE AS {ic}) FROM to_close \
         UNION ALL SELECT *, CURRENT_TIMESTAMP AS {vf}, NULL::TIMESTAMP AS {vt}, TRUE AS {ic} FROM to_insert"
    ))
}

/// Unpivot: turn a set of columns into name/value rows (wide to long).
fn build_unpivot(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.unpivot"))?;
    let cols = columns_list(props, "columns");
    if cols.is_empty() {
        return Err("Unpivot needs the columns to unpivot".to_string());
    }
    let name_col = string_prop(props, "nameColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "name".into());
    let value_col = string_prop(props, "valueColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "value".into());
    let on = cols.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ");
    Ok(format!(
        "SELECT * FROM (UNPIVOT (SELECT * FROM {}) ON {} INTO NAME {} VALUE {})",
        quote_ident(upstream),
        on,
        quote_ident(&name_col),
        quote_ident(&value_col)
    ))
}

/// Column Profile: one summary-stats row per column, via DuckDB
/// SUMMARIZE (count, null %, approx distinct, min/max, quartiles).
fn build_profile(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("qa.profile"))?;
    let cols = columns_list(props, "columns");
    let projection = if cols.is_empty() {
        "*".to_string()
    } else {
        cols.iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ")
    };
    Ok(format!(
        "SELECT * FROM (SUMMARIZE SELECT {} FROM {})",
        projection,
        quote_ident(upstream)
    ))
}

/// Describe: the column names and types of the input.
fn build_describe(inputs: &NodeInputs) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("qa.describe"))?;
    Ok(format!(
        "SELECT * FROM (DESCRIBE SELECT * FROM {})",
        quote_ident(upstream)
    ))
}

/// Histogram: value frequencies for one column, most frequent first.
fn build_histogram(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("qa.histogram"))?;
    let col = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Histogram needs a column".to_string())?;
    let q = quote_ident(&col);
    Ok(format!(
        "SELECT {q} AS value, COUNT(*) AS frequency FROM {} GROUP BY {q} ORDER BY frequency DESC, value",
        quote_ident(upstream)
    ))
}

/// Standardize: trim, case-normalize, and collapse internal whitespace in
/// the chosen text columns, in place.
fn build_standardize(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("qa.standardize"))?;
    let cols = columns_list(props, "columns");
    if cols.is_empty() {
        return Err("Standardize needs at least one column".to_string());
    }
    let case = string_prop(props, "case").unwrap_or_else(|| "none".into());
    let trim = props.get("trim").and_then(|v| v.as_bool()).unwrap_or(true);
    let collapse = props
        .get("collapseWhitespace")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let replacements = cols
        .iter()
        .map(|c| {
            let q = quote_ident(c);
            let mut expr = format!("CAST({} AS VARCHAR)", q);
            expr = match case.as_str() {
                "upper" => format!("UPPER({})", expr),
                "lower" => format!("LOWER({})", expr),
                "title" => format!("INITCAP({})", expr),
                _ => expr,
            };
            if collapse {
                expr = format!("regexp_replace({}, '\\s+', ' ', 'g')", expr);
            }
            if trim {
                expr = format!("TRIM({})", expr);
            }
            format!("{} AS {}", expr, q)
        })
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "SELECT * REPLACE ({}) FROM {}",
        replacements,
        quote_ident(upstream)
    ))
}

/// Lowercased comparison key from the chosen columns, for fuzzy
/// matching. Errors if no columns are given.
fn match_key(props: &JsonValue) -> Result<String, String> {
    let cols = columns_list(props, "columns");
    if cols.is_empty() {
        return Err("needs at least one compare column".to_string());
    }
    Ok(format!(
        "lower(concat_ws(' ', {}))",
        cols.iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// A 0..1 similarity score expression over a._key / b._key, plus the
/// configured threshold. Unknown algorithms fall back to Jaro-Winkler.
fn similarity(props: &JsonValue) -> (String, f64) {
    let algo = string_prop(props, "algorithm").unwrap_or_else(|| "jaro-winkler".into());
    let threshold = props
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.85);
    let score = match algo.as_str() {
        "levenshtein" => "(1.0 - levenshtein(a._key, b._key)::DOUBLE \
             / GREATEST(length(a._key), length(b._key), 1))"
            .to_string(),
        _ => "jaro_winkler_similarity(a._key, b._key)".to_string(),
    };
    (score, threshold)
}

/// Fuzzy Deduplicate: keep the first row of each near-duplicate cluster,
/// where rows are duplicates when their key similarity meets the
/// threshold.
fn build_fuzzy_dedupe(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("qa.dedupe"))?;
    let key = match_key(props).map_err(|e| format!("Fuzzy Deduplicate {e}"))?;
    let (score, threshold) = similarity(props);
    Ok(format!(
        "WITH ranked AS MATERIALIZED (SELECT *, {key} AS _key, \
         ROW_NUMBER() OVER (ORDER BY {key}) AS _rn FROM {up}) \
         SELECT a.* EXCLUDE (_key, _rn) FROM ranked a \
         WHERE NOT EXISTS (SELECT 1 FROM ranked b \
         WHERE b._rn < a._rn AND {score} >= {threshold})",
        up = quote_ident(upstream)
    ))
}

/// Record Match: self-join the input and emit each pair of rows whose key
/// similarity meets the threshold, with a match score (record linkage
/// within one dataset).
fn build_record_match(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("qa.match"))?;
    let key = match_key(props).map_err(|e| format!("Record Match {e}"))?;
    let (score, threshold) = similarity(props);
    Ok(format!(
        "WITH k AS MATERIALIZED (SELECT *, {key} AS _key, ROW_NUMBER() OVER () AS _rn FROM {up}) \
         SELECT a.* EXCLUDE (_key, _rn), b._key AS matched_key, round({score}, 4) AS match_score \
         FROM k a JOIN k b ON a._rn < b._rn AND {score} >= {threshold}",
        up = quote_ident(upstream)
    ))
}

/// Data-quality validators. `reject = false` yields the passing rows;
/// `reject = true` yields the failing rows for the node's reject port.
fn build_quality(
    inputs: &NodeInputs,
    props: &JsonValue,
    component_id: &str,
    reject: bool,
) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "validator: missing main input".to_string())?;
    let from = quote_ident(upstream);
    if component_id == "qa.unique" {
        let keys = columns_list(props, "columns");
        if keys.is_empty() {
            return Err("Uniqueness check needs key columns".into());
        }
        let partition = keys.iter().map(|c| quote_ident(c)).collect::<Vec<_>>().join(", ");
        let cmp = if reject { ">" } else { "=" };
        return Ok(format!(
            "SELECT * EXCLUDE (__dq_rn) FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY {}) AS __dq_rn FROM {}) WHERE __dq_rn {} 1",
            partition, from, cmp
        ));
    }
    let predicate = quality_pass_predicate(component_id, props)?;
    Ok(if reject {
        format!("SELECT * FROM {} WHERE NOT COALESCE(({}), FALSE)", from, predicate)
    } else {
        format!("SELECT * FROM {} WHERE COALESCE(({}), FALSE)", from, predicate)
    })
}

fn quality_pass_predicate(component_id: &str, props: &JsonValue) -> Result<String, String> {
    match component_id {
        "qa.notnull" | "qa.schemavalidate" => {
            // Schema Validate reuses the not-null predicate against the
            // form's expectedColumns list (the columns the user said the
            // input must have populated). Any row missing a value in any
            // of those columns is rejected.
            let key = if component_id == "qa.schemavalidate" {
                "expectedColumns"
            } else {
                "columns"
            };
            let cols = columns_list(props, key);
            if cols.is_empty() {
                return Ok("TRUE".into());
            }
            Ok(cols
                .iter()
                .map(|c| format!("{} IS NOT NULL", quote_ident(c)))
                .collect::<Vec<_>>()
                .join(" AND "))
        }
        "qa.range" => {
            let col = string_prop(props, "column")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "Range check needs a column".to_string())?;
            let c = quote_ident(&col);
            let inclusive = props.get("inclusive").and_then(JsonValue::as_bool).unwrap_or(true);
            let (ge, le) = if inclusive { (">=", "<=") } else { (">", "<") };
            let mut parts = Vec::new();
            if let Some(min) = num_prop(props, "min") {
                parts.push(format!("{} {} {}", c, ge, min));
            }
            if let Some(max) = num_prop(props, "max") {
                parts.push(format!("{} {} {}", c, le, max));
            }
            Ok(if parts.is_empty() { "TRUE".into() } else { parts.join(" AND ") })
        }
        "qa.regex" => {
            let col = string_prop(props, "column")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "Regex check needs a column".to_string())?;
            let pat = string_prop(props, "pattern")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "Regex check needs a pattern".to_string())?;
            Ok(format!(
                "regexp_full_match(CAST({} AS VARCHAR), '{}')",
                quote_ident(&col),
                sql_escape(&pat)
            ))
        }
        other => Err(format!("Validator '{}' is not yet implemented", other)),
    }
}

/// Reject-port SQL for components that split rows. None = no reject table.
fn build_reject_sql(
    component_id: &str,
    props: &JsonValue,
    inputs: &NodeInputs,
) -> Result<Option<String>, String> {
    match component_id {
        "xf.filter" => {
            let upstream = inputs.main().ok_or_else(|| "filter: missing main input".to_string())?;
            let predicate = filter_predicate_sql(props.get("predicate")).unwrap_or_default();
            let predicate = predicate.trim();
            let predicate = if predicate.is_empty() { "TRUE" } else { predicate };
            Ok(Some(format!(
                "SELECT * FROM {} WHERE NOT COALESCE(({}), FALSE)",
                quote_ident(upstream),
                predicate
            )))
        }
        "qa.notnull" | "qa.range" | "qa.regex" | "qa.unique" | "qa.schemavalidate" => {
            Ok(Some(build_quality(inputs, props, component_id, true)?))
        }
        _ => Ok(None),
    }
}

fn columns_list(props: &JsonValue, key: &str) -> Vec<String> {
    props
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// A numeric property as a SQL literal - only if it's actually numeric,
/// so it can't smuggle arbitrary SQL into a comparison.
fn num_prop(props: &JsonValue, key: &str) -> Option<String> {
    match props.get(key) {
        Some(JsonValue::Number(n)) => Some(n.to_string()),
        Some(JsonValue::String(s)) => {
            let t = s.trim();
            t.parse::<f64>().ok().map(|_| t.to_string())
        }
        _ => None,
    }
}

fn build_addcol(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let columns = props
        .get("columns")
        .or_else(|| props.get("additions"))
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    let mut additions: Vec<String> = Vec::new();
    for col in &columns {
        let name = col.get("name").and_then(JsonValue::as_str).unwrap_or("col");
        let expr = col
            .get("expression")
            .or_else(|| col.get("expr"))
            .and_then(JsonValue::as_str)
            .unwrap_or("NULL");
        additions.push(format!("{} AS {}", expr, quote_ident(name)));
    }
    // The Add-Column / Coalesce form is single: { name, expression }.
    if additions.is_empty() {
        let name = string_prop(props, "name").filter(|s| !s.is_empty());
        let expr = string_prop(props, "expression").or_else(|| string_prop(props, "expr"));
        if let (Some(name), Some(expr)) = (name, expr) {
            if !expr.trim().is_empty() {
                additions.push(format!("{} AS {}", expr, quote_ident(&name)));
            }
        }
    }
    if additions.is_empty() {
        return Ok(format!("SELECT * FROM {}", quote_ident(upstream)));
    }
    Ok(format!(
        "SELECT *, {} FROM {}",
        additions.join(", "),
        quote_ident(upstream)
    ))
}

fn build_cast(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let casts = props
        .get("casts")
        .or_else(|| props.get("columns"))
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    // Use REPLACE so we keep other columns. e.g.
    //   SELECT * REPLACE (CAST(amount AS DECIMAL(10,2)) AS amount) FROM x
    let mut replacements: Vec<String> = Vec::new();
    for c in &casts {
        let column = c.get("column").and_then(JsonValue::as_str).unwrap_or("");
        let target = c
            .get("targetType")
            .or_else(|| c.get("type"))
            .and_then(JsonValue::as_str)
            .unwrap_or("VARCHAR");
        if column.is_empty() {
            continue;
        }
        let target_sql = duckle_type_to_duckdb(target);
        replacements.push(format!(
            "CAST({} AS {}) AS {}",
            quote_ident(column),
            target_sql,
            quote_ident(column)
        ));
    }
    // The Cast form is single-column: { column, targetType }.
    if replacements.is_empty() {
        if let Some(column) = string_prop(props, "column").filter(|s| !s.is_empty()) {
            let target = string_prop(props, "targetType")
                .or_else(|| string_prop(props, "type"))
                .unwrap_or_else(|| "string".into());
            replacements.push(format!(
                "CAST({} AS {}) AS {}",
                quote_ident(&column),
                duckle_type_to_duckdb(&target),
                quote_ident(&column)
            ));
        }
    }
    if replacements.is_empty() {
        return Ok(format!("SELECT * FROM {}", quote_ident(upstream)));
    }
    Ok(format!(
        "SELECT * REPLACE ({}) FROM {}",
        replacements.join(", "),
        quote_ident(upstream)
    ))
}

fn build_rename(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "missing main input".to_string())?;
    let renames = props
        .get("renames")
        .or_else(|| props.get("columns"))
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    // RENAME via SELECT * REPLACE - keeps unrelated columns intact.
    // DuckDB doesn't support * REPLACE for renames directly; we use
    // SELECT *, col AS new_col then DROP not possible without listing.
    // Cleanest: enumerate explicit aliases. Need to know all columns.
    // For now, emit a CTE that selects everything then renames each
    // listed pair using a fresh wrapper.
    //   SELECT x.* EXCLUDE (a,b), x.a AS new_a, x.b AS new_b FROM up x
    let mut excludes = Vec::new();
    let mut aliases = Vec::new();
    for r in &renames {
        let from = r
            .get("from")
            .or_else(|| r.get("source"))
            .and_then(JsonValue::as_str);
        let to = r
            .get("to")
            .or_else(|| r.get("target"))
            .and_then(JsonValue::as_str);
        if let (Some(from), Some(to)) = (from, to) {
            excludes.push(quote_ident(from));
            aliases.push(format!(
                "{}.{} AS {}",
                quote_ident(upstream),
                quote_ident(from),
                quote_ident(to)
            ));
        }
    }
    // The Rename form writes `mapping` as key-value pairs: old -> new.
    if aliases.is_empty() {
        if let Some(pairs) = props.get("mapping").and_then(JsonValue::as_array) {
            for kv in pairs {
                let old = kv.get("key").and_then(JsonValue::as_str);
                let new = kv.get("value").and_then(JsonValue::as_str);
                if let (Some(old), Some(new)) = (old, new) {
                    if !old.is_empty() && !new.is_empty() {
                        excludes.push(quote_ident(old));
                        aliases.push(format!(
                            "{}.{} AS {}",
                            quote_ident(upstream),
                            quote_ident(old),
                            quote_ident(new)
                        ));
                    }
                }
            }
        }
    }
    if aliases.is_empty() {
        return Ok(format!("SELECT * FROM {}", quote_ident(upstream)));
    }
    Ok(format!(
        "SELECT {}.* EXCLUDE ({}), {} FROM {}",
        quote_ident(upstream),
        excludes.join(", "),
        aliases.join(", "),
        quote_ident(upstream)
    ))
}

fn build_mapper(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| "mapper: missing main input".to_string())?;
    // The Map form writes `expressions` as key-value pairs:
    // output column name -> SQL expression.
    if let Some(pairs) = props.get("expressions").and_then(JsonValue::as_array) {
        let terms: Vec<String> = pairs
            .iter()
            .filter_map(|kv| {
                let name = kv.get("key").and_then(JsonValue::as_str)?.trim();
                let expr = kv.get("value").and_then(JsonValue::as_str)?.trim();
                if name.is_empty() || expr.is_empty() {
                    return None;
                }
                Some(format!("{} AS {}", strip_port_prefixes(expr), quote_ident(name)))
            })
            .collect();
        if !terms.is_empty() {
            return Ok(format!("SELECT {} FROM {}", terms.join(", "), quote_ident(upstream)));
        }
    }
    let mapper = props.get("mapper");
    let outputs = mapper
        .and_then(|m| m.get("outputs"))
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();
    if outputs.is_empty() {
        return Ok(format!("SELECT * FROM {}", quote_ident(upstream)));
    }
    let mut select_terms = Vec::new();
    for o in &outputs {
        let name = o.get("name").and_then(JsonValue::as_str).unwrap_or("col");
        let expr_raw = o
            .get("expression")
            .or_else(|| o.get("expr"))
            .and_then(JsonValue::as_str)
            .unwrap_or("NULL");
        // The visual mapper emits references like `main.col` or
        // `lookup_1.col`. Those don't exist as DuckDB alias prefixes
        // in our generated SQL, so we strip them to bare column refs.
        let expr = strip_port_prefixes(expr_raw);
        select_terms.push(format!("{} AS {}", expr, quote_ident(name)));
    }
    let filter = mapper
        .and_then(|m| m.get("filter"))
        .and_then(JsonValue::as_str)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let mut sql = format!(
        "SELECT {} FROM {}",
        select_terms.join(", "),
        quote_ident(upstream)
    );
    if let Some(predicate) = filter {
        sql.push_str(" WHERE ");
        sql.push_str(predicate);
    }
    Ok(sql)
}

fn strip_port_prefixes(expr: &str) -> String {
    // Replace `<word>.<word>` where the leading word is a known port
    // alias the mapper used, leaving the column reference untouched.
    let mut out = String::with_capacity(expr.len());
    for token in expr.split_inclusive(|c: char| !c.is_alphanumeric() && c != '_' && c != '.') {
        // For each token, if it looks like main.col / lookup_N.col,
        // drop the prefix.
        let (alpha, rest) = split_leading_token(token);
        if !alpha.is_empty() && (alpha == "main" || alpha.starts_with("lookup")) {
            if let Some(stripped) = rest.strip_prefix('.') {
                out.push_str(stripped);
                continue;
            }
        }
        out.push_str(token);
    }
    out
}

fn split_leading_token(s: &str) -> (&str, &str) {
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    (&s[..end], &s[end..])
}

fn build_join(inputs: &NodeInputs, props: &JsonValue, kind: &str) -> Result<String, String> {
    let left = inputs.main().ok_or_else(|| "join: missing main input".to_string())?;
    let right = inputs
        .first_lookup()
        .ok_or_else(|| "join: missing lookup input".to_string())?;
    let left_key = props
        .get("leftKey")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "join: leftKey property required".to_string())?;
    let right_key = props
        .get("rightKey")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "join: rightKey property required".to_string())?;
    // The form's joinType, if set, overrides the component-id default so
    // changing it in the UI actually takes effect.
    let kind = match string_prop(props, "joinType").as_deref() {
        Some("inner") => "INNER",
        Some("left") => "LEFT",
        Some("right") => "RIGHT",
        Some("full") | Some("outer") => "FULL OUTER",
        _ => kind,
    };
    Ok(format!(
        "SELECT m.*, r.* FROM {} m {} JOIN {} r ON m.{} = r.{}",
        quote_ident(left),
        kind,
        quote_ident(right),
        quote_ident(left_key),
        quote_ident(right_key)
    ))
}

fn build_semi(inputs: &NodeInputs, props: &JsonValue, anti: bool) -> Result<String, String> {
    let left = inputs.main().ok_or_else(|| "semi: missing main input".to_string())?;
    let right = inputs
        .first_lookup()
        .ok_or_else(|| "semi: missing lookup input".to_string())?;
    let left_key = props
        .get("leftKey")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "semi: leftKey required".to_string())?;
    let right_key = props
        .get("rightKey")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "semi: rightKey required".to_string())?;
    let op = if anti { "NOT IN" } else { "IN" };
    Ok(format!(
        "SELECT * FROM {} WHERE {} {} (SELECT {} FROM {})",
        quote_ident(left),
        quote_ident(left_key),
        op,
        quote_ident(right_key),
        quote_ident(right)
    ))
}

// ---- Sources ------------------------------------------------------------

fn build_csv_source(props: &JsonValue) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    let has_header = props
        .get("hasHeader")
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    let delim = string_prop(props, "delimiter");
    let quote = string_prop(props, "quoteChar");
    let null_val = string_prop(props, "nullValue");
    let mut args = vec![format!("'{}'", sql_escape(&path))];
    args.push(format!("header={}", has_header));
    if let Some(d) = delim.as_deref().filter(|s| !s.is_empty()) {
        args.push(format!("delim='{}'", sql_escape(d)));
    }
    if let Some(q) = quote.as_deref().filter(|s| !s.is_empty()) {
        args.push(format!("quote='{}'", sql_escape(q)));
    }
    if let Some(n) = null_val.as_deref().filter(|s| !s.is_empty()) {
        args.push(format!("nullstr='{}'", sql_escape(n)));
    }
    if let Some(skip) = props.get("skipLines").and_then(JsonValue::as_u64) {
        if skip > 0 {
            args.push(format!("skip={}", skip));
        }
    }
    if let Some(enc) = string_prop(props, "encoding").filter(|s| !s.is_empty()) {
        args.push(format!("encoding='{}'", sql_escape(&enc)));
    }
    format!("SELECT * FROM read_csv_auto({})", args.join(", "))
}

fn build_tsv_source(props: &JsonValue) -> String {
    // TSV is just CSV with delim='\t'. Force it.
    let mut p = props.clone();
    if let Some(obj) = p.as_object_mut() {
        obj.insert(
            "delimiter".into(),
            JsonValue::String("\t".into()),
        );
    }
    build_csv_source(&p)
}

fn build_parquet_source(props: &JsonValue) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    // Optional projection: comma-separated column list pushed into the read.
    let select = string_prop(props, "columns")
        .filter(|s| !s.trim().is_empty())
        .map(|c| {
            c.split(',')
                .map(|s| quote_ident(s.trim()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "*".into());
    format!("SELECT {} FROM read_parquet('{}')", select, sql_escape(&path))
}

fn build_json_source(props: &JsonValue) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    format!(
        "SELECT * FROM read_json_auto('{}')",
        sql_escape(&path)
    )
}

fn build_sqlite_source(props: &JsonValue) -> String {
    let database = string_prop(props, "database").unwrap_or_default();
    let table = string_prop(props, "tableName").unwrap_or_default();
    let sql = string_prop(props, "sql");
    let from_arg = sql
        .filter(|s| !s.is_empty())
        .unwrap_or(table);
    format!(
        "SELECT * FROM sqlite_scan('{}', '{}')",
        sql_escape(&database),
        sql_escape(&from_arg)
    )
}

fn build_duckdb_source(props: &JsonValue) -> String {
    // The DuckDB file is ATTACHed as `duckle_src` (READ_ONLY) by the
    // stage / inspect prelude; we read from it qualified by that alias.
    if let Some(table) = string_prop(props, "tableName").filter(|s| !s.is_empty()) {
        match string_prop(props, "schema").filter(|s| !s.is_empty()) {
            Some(schema) => format!(
                "SELECT * FROM duckle_src.{}.{}",
                quote_ident(&schema),
                quote_ident(&table)
            ),
            None => format!("SELECT * FROM duckle_src.{}", quote_ident(&table)),
        }
    } else if let Some(sql) = string_prop(props, "sql").filter(|s| !s.trim().is_empty()) {
        // Advanced: a custom query. Reference tables as duckle_src.<table>.
        format!("({})", sql)
    } else {
        "SELECT 1 AS placeholder LIMIT 0".into()
    }
}

/// ATTACH statements for external-database nodes. The aliases are fixed
/// (`duckle_src` / `duckle_dst`) - safe because each stage is its own
/// CLI process.
fn attach_prelude(component_id: &str, props: &JsonValue) -> String {
    // Network DBs use host/port + libpq-style fields, not the
    // file-style `database` path the file-based ATTACH connectors use.
    // Cockroach speaks PG wire so it rides the postgres extension;
    // MariaDB speaks MySQL wire so it rides the mysql extension.
    match component_id {
        "src.postgres" | "src.cockroach" | "src.pgvector" | "src.redshift" => {
            // Redshift speaks the Postgres wire protocol with a different
            // default port (5439). The DuckDB postgres extension is happy
            // pointed at any pg-compatible endpoint.
            let default_port = if component_id == "src.redshift" { 5439 } else { 5432 };
            return db_attach(props, "postgres", default_port, true);
        }
        "snk.postgres" | "snk.cockroach" | "snk.pgvector" | "snk.redshift" => {
            let default_port = if component_id == "snk.redshift" { 5439 } else { 5432 };
            return db_attach(props, "postgres", default_port, false);
        }
        "src.mysql" | "src.mariadb" => return db_attach(props, "mysql", 3306, true),
        "snk.mysql" | "snk.mariadb" => return db_attach(props, "mysql", 3306, false),
        "src.motherduck" => return md_attach(props, true),
        "snk.motherduck" => return md_attach(props, false),
        "src.ducklake" => return ducklake_attach(props, true),
        "snk.ducklake" => return ducklake_attach(props, false),
        // BigQuery via the duckdb-bigquery community extension. The
        // user's prop 'project' becomes the BigQuery project ID; the
        // ATTACH alias is the standard duckle_src / duckle_dst.
        "src.bigquery" => return bigquery_attach(props, true),
        "snk.bigquery" => return bigquery_attach(props, false),
        // snk.excel COPYs through the DuckDB excel extension; LOAD is
        // enough since the install paths pre-fetched it.
        "snk.excel" => return "LOAD excel; ".into(),
        // Extensions are pre-installed (desktop: the first-launch
        // installer; CI: a dedicated pre-install step). Each fresh
        // DuckDB process still needs LOAD. Concurrent INSTALL would
        // race on the cached extension file and intermittently fail.
        "src.avro" => return "LOAD avro; ".into(),
        "src.excel" => return "LOAD excel; ".into(),
        "src.iceberg" | "snk.iceberg" => return "LOAD iceberg; ".into(),
        "src.delta" => return "LOAD delta; ".into(),
        // Vector Similarity Search uses the vss extension's array_*
        // distance functions; LOAD before the SELECT runs.
        "xf.ai.vector_search" => return "LOAD vss; ".into(),
        // Full-Text Search uses the fts extension's match_bm25.
        "xf.ai.text_search" => return "LOAD fts; ".into(),
        // Spatial is GDAL-backed and ~50 MB; deliberately kept out of
        // the first-launch DUCKDB_EXTENSIONS pre-fetch so the install
        // stays small. INSTALL runs lazily on first use, then LOAD on
        // every subsequent run.
        "src.spatial"
        | "snk.spatial"
        | "xf.geo.distance"
        | "xf.geo.buffer"
        | "xf.geo.intersects"
        | "xf.join.spatial" => {
            return "INSTALL spatial; LOAD spatial; ".into();
        }
        // inet is a small built-in extension. INSTALL is a no-op once
        // the extension is bundled, but keeping it explicit means a
        // fresh CLI cache still works without the first-launch fetch.
        "xf.ip.parse" => return "INSTALL inet; LOAD inet; ".into(),
        _ => {}
    }
    let db = match string_prop(props, "database").filter(|s| !s.is_empty()) {
        Some(d) => d,
        None => return String::new(),
    };
    match component_id {
        "src.duckdb" => format!("ATTACH '{}' AS duckle_src (READ_ONLY); ", sql_escape(&db)),
        "snk.sqlite" => format!("ATTACH '{}' AS duckle_dst (TYPE SQLITE); ", sql_escape(&db)),
        "snk.duckdb" => format!("ATTACH '{}' AS duckle_dst; ", sql_escape(&db)),
        _ => String::new(),
    }
}

/// ATTACH a network relational database through a DuckDB extension
/// (postgres or mysql). The connection string is built libpq-style from
/// host / port / database / user / password; the extension-specific key
/// for the database name (`dbname` for libpq/Postgres, `database` for
/// the MySQL driver) is handled here. INSTALL+LOAD is prepended so a
/// fresh user without the extension cache still attaches successfully,
/// though the first-launch installer already pre-fetches both.
fn db_attach(props: &JsonValue, extension: &str, default_port: u64, read_only: bool) -> String {
    let host = string_prop(props, "host").unwrap_or_default();
    if host.is_empty() {
        return String::new();
    }
    let port = props
        .get("port")
        .and_then(|v| v.as_u64())
        .filter(|p| *p > 0)
        .unwrap_or(default_port);
    let db_key = if extension == "postgres" { "dbname" } else { "database" };
    let mut parts = vec![format!("host={}", host), format!("port={}", port)];
    if let Some(db) = string_prop(props, "database").filter(|s| !s.is_empty()) {
        parts.push(format!("{}={}", db_key, db));
    }
    if let Some(u) = string_prop(props, "user").filter(|s| !s.is_empty()) {
        parts.push(format!("user={}", u));
    }
    if let Some(p) = string_prop(props, "password").filter(|s| !s.is_empty()) {
        parts.push(format!("password={}", p));
    }
    let connstr = parts.join(" ");
    let (alias, mode) = if read_only {
        ("duckle_src", ", READ_ONLY")
    } else {
        ("duckle_dst", "")
    };
    let type_name = extension.to_uppercase();
    format!(
        "LOAD {ext}; ATTACH '{conn}' AS {alias} (TYPE {type_name}{mode}); ",
        ext = extension,
        conn = sql_escape(&connstr),
        alias = alias,
        type_name = type_name,
        mode = mode
    )
}

/// Source for a network relational DB (Postgres / Cockroach via the
/// postgres extension; MySQL / MariaDB via the mysql extension). Reads
/// from `duckle_src` qualified by the right depth: Postgres uses
/// catalog.schema.table (default schema `public`); MySQL uses
/// catalog.table (the database is selected at ATTACH time).
fn build_relational_source(component_id: &str, props: &JsonValue) -> Result<String, String> {
    let mode = string_prop(props, "mode").unwrap_or_else(|| "table".into());
    if mode == "sql" {
        let sql = string_prop(props, "sql")
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| format!("{}: SQL query is empty", component_id))?;
        return Ok(format!("({})", sql));
    }
    if mode == "incremental" {
        return Err(format!(
            "{}: incremental read mode isn't implemented yet",
            component_id
        ));
    }
    let table = string_prop(props, "tableName")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{}: table name is required", component_id))?;
    let schema = string_prop(props, "schemaName").filter(|s| !s.is_empty());
    Ok(format!(
        "SELECT * FROM {}",
        relational_qualified("duckle_src", component_id, schema.as_deref(), &table)
    ))
}

/// Sink for a network relational DB (Postgres / Cockroach / MySQL /
/// MariaDB). Only `overwrite` (DROP + CREATE) is wired today; append /
/// upsert / truncate / error-if-exists error loudly rather than
/// pretending to apply. Writes inside the ATTACHed `duckle_dst` DB.
fn build_relational_sink(
    component_id: &str,
    props: &JsonValue,
    from_view: &str,
) -> Result<String, EngineError> {
    let table = string_prop(props, "tableName")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| EngineError::Config(format!("{}: table name is required", component_id)))?;
    let schema = string_prop(props, "schemaName").filter(|s| !s.is_empty());
    let mode = string_prop(props, "mode").unwrap_or_else(|| "overwrite".into());
    let qual = relational_qualified("duckle_dst", component_id, schema.as_deref(), &table);
    match mode.as_str() {
        "overwrite" => Ok(format!(
            "DROP TABLE IF EXISTS {q}; CREATE TABLE {q} AS (SELECT * FROM {from})",
            q = qual,
            from = quote_ident(from_view)
        )),
        // Append inserts into an existing table; the table must already
        // exist (create-if-missing isn't wired yet because we don't know
        // the upstream's column types ahead of time without inspecting).
        "append" => Ok(format!(
            "INSERT INTO {q} SELECT * FROM {from}",
            q = qual,
            from = quote_ident(from_view)
        )),
        // Truncate keeps the table's existing schema (and any indexes /
        // grants on it) and replaces just the rows. Useful when the
        // table is referenced by downstream views or foreign keys.
        "truncate" => Ok(format!(
            "TRUNCATE TABLE {q}; INSERT INTO {q} SELECT * FROM {from}",
            q = qual,
            from = quote_ident(from_view)
        )),
        other => Err(EngineError::Config(format!(
            "{}: write mode '{}' isn't implemented yet (use 'overwrite', 'append', or 'truncate')",
            component_id, other
        ))),
    }
}

/// Qualify a table reference under the right naming depth for each
/// network DB family. Postgres / Cockroach use catalog.schema.table
/// (default schema `public`); MotherDuck is DuckDB-native and uses
/// catalog.schema.table with default schema `main`; MySQL / MariaDB
/// use catalog.table (the MySQL database is selected at ATTACH time,
/// though we honour an explicit schemaName as a 3-level qualifier).
fn relational_qualified(alias: &str, component_id: &str, schema: Option<&str>, table: &str) -> String {
    let default_schema: Option<&str> = if component_id.ends_with(".postgres")
        || component_id.ends_with(".cockroach")
        || component_id.ends_with(".pgvector")
        || component_id.ends_with(".redshift")
    {
        Some("public")
    } else if component_id.ends_with(".motherduck") || component_id.ends_with(".ducklake") {
        Some("main")
    } else if component_id.ends_with(".bigquery") {
        // BigQuery's first level is a "dataset" - same shape as schema.
        // Caller can supply dataset via either prop name; we leave the
        // default empty so the ATTACH-time default dataset takes over
        // when unqualified.
        None
    } else {
        None // MySQL / MariaDB: skip the schema layer unless given
    };
    match (schema, default_schema) {
        (Some(s), _) => format!("{}.{}.{}", alias, quote_ident(s), quote_ident(table)),
        (None, Some(d)) => format!("{}.{}.{}", alias, quote_ident(d), quote_ident(table)),
        (None, None) => format!("{}.{}", alias, quote_ident(table)),
    }
}

/// DuckLake ATTACH. DuckLake is DuckDB's own lakehouse format (a
/// catalog stored in a DuckDB file or Postgres pointing at parquet
/// data files). The form's `path` is the catalog path.
fn ducklake_attach(props: &JsonValue, read_only: bool) -> String {
    let path = match string_prop(props, "path").filter(|s| !s.is_empty()) {
        Some(p) => p,
        None => return String::new(),
    };
    let (alias, mode) = if read_only {
        ("duckle_src", " (READ_ONLY)")
    } else {
        ("duckle_dst", "")
    };
    format!(
        "INSTALL ducklake; LOAD ducklake; ATTACH 'ducklake:{}' AS {}{}; ",
        sql_escape(&path),
        alias,
        mode
    )
}

/// MotherDuck ATTACH. MotherDuck support is built into DuckDB itself
/// (no extension to install), so this just builds an `md:` URL with
/// an optional inline `motherduck_token` query parameter. If the token
/// isn't in the form, MotherDuck falls back to the MOTHERDUCK_TOKEN env
/// var, which lets a user keep credentials out of saved pipelines.
/// BigQuery via the duckdb-bigquery community extension. ATTACHes a
/// project by ID; auth uses the standard GCP credential discovery
/// (GOOGLE_APPLICATION_CREDENTIALS env var, gcloud default, etc).
/// User points the extension at a project via the 'project' prop;
/// optional 'dataset' fills in the default dataset for unqualified
/// table names.
fn bigquery_attach(props: &JsonValue, read_only: bool) -> String {
    let project = match string_prop(props, "project").filter(|s| !s.is_empty()) {
        Some(p) => p,
        None => return String::new(),
    };
    let dataset = string_prop(props, "dataset").filter(|s| !s.is_empty());
    let attach_target = match dataset {
        Some(d) => format!("project={} dataset={}", project, d),
        None => format!("project={}", project),
    };
    let (alias, mode) = if read_only {
        ("duckle_src", " (READ_ONLY)")
    } else {
        ("duckle_dst", "")
    };
    // INSTALL/LOAD the community extension. The community: tag tells
    // DuckDB to fetch from the community-extensions repo.
    format!(
        "INSTALL bigquery FROM community; LOAD bigquery; ATTACH '{}' AS {} (TYPE bigquery{}); ",
        attach_target, alias, mode
    )
}

fn md_attach(props: &JsonValue, read_only: bool) -> String {
    let db = match string_prop(props, "database").filter(|s| !s.is_empty()) {
        Some(d) => d,
        None => return String::new(),
    };
    let token = string_prop(props, "token").filter(|s| !s.is_empty());
    let url = match token {
        Some(t) => format!("md:{}?motherduck_token={}", db, t),
        None => format!("md:{}", db),
    };
    let (alias, mode) = if read_only {
        ("duckle_src", " (READ_ONLY)")
    } else {
        ("duckle_dst", "")
    };
    format!("ATTACH '{}' AS {}{}; ", sql_escape(&url), alias, mode)
}

/// Excel sink: COPY ... TO '<path>' (FORMAT 'xlsx'). The form's
/// `hasHeader` toggle becomes HEADER true/false. v1.2+ ships native
/// xlsx writer in the excel extension.
fn build_excel_sink(props: &JsonValue, from_view: &str) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    let header = props
        .get("hasHeader")
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    format!(
        "COPY (SELECT * FROM {}) TO '{}' (FORMAT 'xlsx', HEADER {})",
        quote_ident(from_view),
        sql_escape(&path),
        header
    )
}

/// Iceberg sink: COPY ... TO '<path>' (FORMAT 'iceberg'). DuckDB
/// v1.5+ writes a full Iceberg table (data/ + metadata/) at the
/// given path. Read-back via src.iceberg.
fn build_iceberg_sink(props: &JsonValue, from_view: &str) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    format!(
        "COPY (SELECT * FROM {}) TO '{}' (FORMAT 'iceberg')",
        quote_ident(from_view),
        sql_escape(&path)
    )
}

/// Geospatial sink via the spatial extension's GDAL writer. The form's
/// `driver` picks the OGR driver (GeoJSON / GeoPackage / Shapefile /
/// KML / GPX). Most drivers expect a geometry column called `geom`.
fn build_spatial_sink(props: &JsonValue, from_view: &str) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    let driver = string_prop(props, "driver")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "GeoJSON".into());
    format!(
        "COPY (SELECT * FROM {}) TO '{}' (FORMAT GDAL, DRIVER '{}')",
        quote_ident(from_view),
        sql_escape(&path),
        sql_escape(&driver)
    )
}

/// SQLite / DuckDB sink - write the upstream into a table inside the
/// ATTACHed `duckle_dst` database. DROP+CREATE works for both writers
/// (the SQLite writer doesn't support CREATE OR REPLACE).
fn build_db_sink(props: &JsonValue, from_view: &str) -> String {
    let table = string_prop(props, "tableName")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "output".into());
    let t = quote_ident(&table);
    format!(
        "DROP TABLE IF EXISTS duckle_dst.{}; CREATE TABLE duckle_dst.{} AS (SELECT * FROM {})",
        t,
        t,
        quote_ident(from_view)
    )
}

/// Avro source. The `avro` DuckDB community extension exposes
/// `read_avro` (read-only); the LOAD is in the stage prelude so the
/// function is available before the SELECT runs.
fn build_avro_source(props: &JsonValue) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    format!("SELECT * FROM read_avro('{}')", sql_escape(&path))
}

/// Validate the text-search form and produce the spec the executor
/// uses to run the two CLI calls (stage table -> index + final query).
fn build_text_search_spec(node_id: &str, inputs: &NodeInputs, props: &JsonValue) -> Result<TextSearchSpec, String> {
    let upstream = inputs
        .main()
        .ok_or_else(|| missing_input_msg("xf.ai.text_search"))?;
    let id_col = string_prop(props, "idColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Text Search needs an id column (unique per row)".to_string())?;
    let text_cols = columns_list(props, "textColumns");
    if text_cols.is_empty() {
        return Err("Text Search needs at least one text column to index".to_string());
    }
    let query = string_prop(props, "query")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Text Search needs a query string".to_string())?;
    let top_k = props
        .get("topK")
        .and_then(|v| v.as_u64())
        .filter(|k| *k > 0);
    let output_col = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "score".into());
    let suffix: String = node_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let staging_table = format!("_fts_{}", suffix);
    Ok(TextSearchSpec {
        from_view: upstream.to_string(),
        id_col,
        text_cols,
        query,
        top_k,
        output_col,
        staging_table,
    })
}

/// Spatial Distance: add a column with the distance from each row's
/// geometry to a fixed target point (WKT). Uses the spatial extension's
/// ST_Distance over CAST geometries. Units come from the SRS of the
/// input geometry (degrees for plain WGS84, metres for projected SRS).
fn build_geo_distance(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.geo.distance"))?;
    let column = string_prop(props, "geomColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Geo Distance needs a geometry column".to_string())?;
    let target = string_prop(props, "targetWkt")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Geo Distance needs a target geometry (WKT, e.g. 'POINT(0 0)')".to_string())?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "distance".into());
    Ok(format!(
        "SELECT *, ST_Distance(CAST({col} AS GEOMETRY), ST_GeomFromText('{target}')) AS {out} FROM {up}",
        col = quote_ident(&column),
        target = target.replace('\'', "''"),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Spatial Buffer: add a column with ST_Buffer(geom, distance) - the
/// area within `distance` of each row's geometry.
fn build_geo_buffer(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.geo.buffer"))?;
    let column = string_prop(props, "geomColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Geo Buffer needs a geometry column".to_string())?;
    let distance = props
        .get("distance")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "Geo Buffer needs a distance".to_string())?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "buffer".into());
    Ok(format!(
        "SELECT *, ST_Buffer(CAST({col} AS GEOMETRY), {distance}) AS {out} FROM {up}",
        col = quote_ident(&column),
        distance = distance,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Base64: encode a column to base64 text, or decode a base64 text
/// column back to bytes (returned as VARCHAR for downstream
/// compatibility - the actual underlying type is BLOB).
fn build_base64(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.text.base64"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Base64 needs a column".to_string())?;
    let mode = string_prop(props, "mode").unwrap_or_else(|| "encode".into());
    let qcol = quote_ident(&column);
    let expr = if mode == "decode" {
        format!("CAST(from_base64(CAST({} AS VARCHAR)) AS VARCHAR)", qcol)
    } else {
        format!("base64(CAST({} AS BLOB))", qcol)
    };
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_{}", column, mode));
    Ok(format!(
        "SELECT *, {expr} AS {out} FROM {up}",
        expr = expr,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Z-Score: per-row standardized value computed against the whole
/// input via window aggregates. (value - mean) / stddev_samp. Useful
/// for outlier detection and feature scaling. Single SQL pass; no
/// extra stage. If stddev is 0 (all values equal), the result is NULL
/// rather than divide-by-zero.
fn build_zscore(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.num.zscore"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Z-Score needs a column".to_string())?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_zscore", column));
    let qcol = quote_ident(&column);
    Ok(format!(
        "SELECT *, CASE WHEN stddev_samp(CAST({col} AS DOUBLE)) OVER () = 0 THEN NULL ELSE (CAST({col} AS DOUBLE) - avg(CAST({col} AS DOUBLE)) OVER ()) / stddev_samp(CAST({col} AS DOUBLE)) OVER () END AS {out} FROM {up}",
        col = qcol,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Text Reverse: reverse the characters in a string column.
/// DuckDB reverse() function.
fn build_text_reverse(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.text.reverse"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Reverse needs a column".to_string())?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_reversed", column));
    Ok(format!(
        "SELECT *, reverse(CAST({col} AS VARCHAR)) AS {out} FROM {up}",
        col = quote_ident(&column),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Text Repeat: repeat a string column N times via DuckDB repeat().
fn build_text_repeat(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.text.repeat"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Repeat needs a column".to_string())?;
    let count = props
        .get("count")
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .unwrap_or(2);
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_repeated", column));
    Ok(format!(
        "SELECT *, repeat(CAST({col} AS VARCHAR), {n}) AS {out} FROM {up}",
        col = quote_ident(&column),
        n = count,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Compare: produce a boolean column from a comparison of two
/// upstream columns. op = eq / neq / lt / le / gt / ge. Useful for
/// flagging mismatches between expected/actual columns.
fn build_compare(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.compare"))?;
    let left = string_prop(props, "leftColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Compare needs a left column".to_string())?;
    let right = string_prop(props, "rightColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Compare needs a right column".to_string())?;
    let op = string_prop(props, "op").unwrap_or_else(|| "eq".into());
    let sql_op = match op.as_str() {
        "neq" => "!=",
        "lt" => "<",
        "le" => "<=",
        "gt" => ">",
        "ge" => ">=",
        _ => "=",
    };
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_{}_{}", left, op, right));
    Ok(format!(
        "SELECT *, ({} {} {}) AS {} FROM {}",
        quote_ident(&left),
        sql_op,
        quote_ident(&right),
        quote_ident(&output),
        quote_ident(upstream)
    ))
}

/// Text Match: boolean substring / prefix / suffix predicate via
/// DuckDB's contains / starts_with / ends_with. Adds a boolean
/// column - pair with Filter Rows downstream to keep only matches.
fn build_text_match(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.text.match"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Text Match needs a column".to_string())?;
    let needle = string_prop(props, "needle")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Text Match needs a search term".to_string())?;
    let mode = string_prop(props, "mode").unwrap_or_else(|| "contains".into());
    let fn_name = match mode.as_str() {
        "starts_with" => "starts_with",
        "ends_with" => "ends_with",
        _ => "contains",
    };
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_{}", column, mode));
    Ok(format!(
        "SELECT *, {fn}(CAST({col} AS VARCHAR), '{n}') AS {out} FROM {up}",
        fn = fn_name,
        col = quote_ident(&column),
        n = sql_escape(&needle),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Sign: -1 for negative, 0 for zero, +1 for positive. DuckDB's
/// sign() function on a DOUBLE input.
fn build_sign(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.num.sign"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Sign needs a column".to_string())?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_sign", column));
    Ok(format!(
        "SELECT *, sign(CAST({col} AS DOUBLE)) AS {out} FROM {up}",
        col = quote_ident(&column),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Clamp: clip numeric values to a [low, high] range via LEAST +
/// GREATEST. Values below low become low; above high become high.
/// Useful for capping outliers before downstream stats.
fn build_clamp(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.num.clamp"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Clamp needs a column".to_string())?;
    let low = props
        .get("low")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "Clamp needs a low bound".to_string())?;
    let high = props
        .get("high")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "Clamp needs a high bound".to_string())?;
    if high < low {
        return Err("Clamp needs high >= low".to_string());
    }
    let qcol = quote_ident(&column);
    Ok(format!(
        "SELECT * REPLACE (LEAST(GREATEST(CAST({col} AS DOUBLE), {low}), {high}) AS {col}) FROM {up}",
        col = qcol,
        low = low,
        high = high,
        up = quote_ident(upstream)
    ))
}

/// String Padding: pad a string column to a fixed length on the left
/// or right with a fill character. Default fills with space, mode
/// 'left' (lpad) is the classic 'zero-pad numeric IDs' pattern.
fn build_padding(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.text.padding"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Padding needs a column".to_string())?;
    let length = props
        .get("length")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
        .ok_or_else(|| "Padding needs a positive target length".to_string())?;
    let fill = string_prop(props, "fill")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| " ".into());
    let side = string_prop(props, "side").unwrap_or_else(|| "left".into());
    let fn_name = if side == "right" { "rpad" } else { "lpad" };
    let qcol = quote_ident(&column);
    let fill_escaped = sql_escape(&fill);
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| column.clone());
    if output == column {
        Ok(format!(
            "SELECT * REPLACE ({fn}(CAST({col} AS VARCHAR), {n}, '{f}') AS {col}) FROM {up}",
            fn = fn_name,
            col = qcol,
            n = length,
            f = fill_escaped,
            up = quote_ident(upstream)
        ))
    } else {
        Ok(format!(
            "SELECT *, {fn}(CAST({col} AS VARCHAR), {n}, '{f}') AS {out} FROM {up}",
            fn = fn_name,
            col = qcol,
            n = length,
            f = fill_escaped,
            out = quote_ident(&output),
            up = quote_ident(upstream)
        ))
    }
}

/// Date/Time Epoch: convert a TIMESTAMP column to Unix epoch seconds
/// (mode 'to') or epoch seconds back to TIMESTAMP (mode 'from').
/// Both directions use DuckDB core functions, no extension needed.
fn build_dt_epoch(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.dt.epoch"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Epoch needs a column".to_string())?;
    let mode = string_prop(props, "mode").unwrap_or_else(|| "to".into());
    let qcol = quote_ident(&column);
    let expr = if mode == "from" {
        format!("to_timestamp(CAST({} AS DOUBLE))", qcol)
    } else {
        format!("epoch(CAST({} AS TIMESTAMP))", qcol)
    };
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if mode == "from" {
                format!("{}_timestamp", column)
            } else {
                format!("{}_epoch", column)
            }
        });
    Ok(format!(
        "SELECT *, {expr} AS {out} FROM {up}",
        expr = expr,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Current Timestamp: add a column holding the time at which the
/// pipeline runs - the standard 'loaded_at' / 'processed_at' /
/// 'ingested_at' stamp every ETL output usually carries.
fn build_dt_now(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.dt.now"))?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "loaded_at".into());
    Ok(format!(
        "SELECT *, current_timestamp AS {out} FROM {up}",
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// UUID: add a freshly-generated UUID v4 to every row. Standard
/// 'surrogate row id' pattern, especially handy before upserts into
/// systems that need a non-business primary key.
fn build_uuid(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.uuid"))?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "row_id".into());
    Ok(format!(
        "SELECT *, uuid() AS {out} FROM {up}",
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Cumulative: running aggregate over an ordered window
/// (sum / avg / count / min / max), optionally per-group. Classic
/// reporting pattern - 'running total of sales', 'cumulative count
/// of users per region'. Uses the standard ROWS BETWEEN UNBOUNDED
/// PRECEDING AND CURRENT ROW frame so the value at each row reflects
/// everything seen so far in scan order.
fn build_cumulative(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.cumulative"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Cumulative needs a column".to_string())?;
    let order_col = string_prop(props, "orderBy")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Cumulative needs an orderBy column".to_string())?;
    let func = string_prop(props, "function").unwrap_or_else(|| "sum".into()).to_lowercase();
    let fn_name = match func.as_str() {
        "avg" => "avg",
        "count" => "count",
        "min" => "min",
        "max" => "max",
        _ => "sum",
    };
    let partition: Vec<String> = columns_from_props(props, "partitionBy").unwrap_or_default();
    let partition_clause = if partition.is_empty() {
        String::new()
    } else {
        let cols = partition
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        format!("PARTITION BY {} ", cols)
    };
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_running_{}", column, fn_name));
    Ok(format!(
        "SELECT *, {fn}({col}) OVER ({part}ORDER BY {ord} ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS {out} FROM {up}",
        fn = fn_name,
        col = quote_ident(&column),
        part = partition_clause,
        ord = quote_ident(&order_col),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Time Bin: round a timestamp column down to the nearest multiple of
/// the chosen interval (e.g. 5-minute, 1-hour, 1-day buckets) for
/// time-series grouping. Done via epoch math so any (unit, count)
/// combination works, not just the standard date_trunc units.
fn build_dt_bin(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.dt.bin"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Time Bin needs a timestamp column".to_string())?;
    let unit = string_prop(props, "unit").unwrap_or_else(|| "minute".into());
    let count = props
        .get("count")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
        .unwrap_or(5);
    let seconds_per = match unit.to_lowercase().as_str() {
        "second" | "seconds" => 1_i64,
        "minute" | "minutes" => 60,
        "hour" | "hours" => 3_600,
        "day" | "days" => 86_400,
        _ => 60,
    };
    let bucket_seconds = seconds_per * count;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_bin", column));
    let qcol = quote_ident(&column);
    Ok(format!(
        "SELECT *, to_timestamp(floor(epoch(CAST({col} AS TIMESTAMP)) / {bucket}) * {bucket}) AS {out} FROM {up}",
        col = qcol,
        bucket = bucket_seconds,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Array Length: scalar length of an array / list column.
fn build_arr_length(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.arr.length"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Array Length needs a column".to_string())?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_length", column));
    Ok(format!(
        "SELECT *, length({col}) AS {out} FROM {up}",
        col = quote_ident(&column),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Rank Filter: keep the top N rows per group, ordered by a column.
/// Common reporting pattern: 'top 3 spenders per region', 'most
/// recent 5 orders per customer'. Computes ROW_NUMBER over the
/// (partitionBy, orderBy DESC|ASC) window in a subquery, then
/// WHERE filters to rank <= N. desc defaults to true (top N).
fn build_rank_filter(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.rank.filter"))?;
    let order_col = string_prop(props, "orderBy")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Rank Filter needs an orderBy column".to_string())?;
    let partition: Vec<String> = columns_from_props(props, "partitionBy").unwrap_or_default();
    let n = props
        .get("n")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
        .unwrap_or(10);
    let desc = props
        .get("desc")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let direction = if desc { "DESC" } else { "ASC" };
    let partition_clause = if partition.is_empty() {
        String::new()
    } else {
        let cols = partition
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        format!("PARTITION BY {} ", cols)
    };
    Ok(format!(
        "SELECT * EXCLUDE (_duckle_rank) FROM (SELECT u.*, row_number() OVER ({part}ORDER BY {ord} {dir}) AS _duckle_rank FROM {up} u) WHERE _duckle_rank <= {n}",
        part = partition_clause,
        ord = quote_ident(&order_col),
        dir = direction,
        n = n,
        up = quote_ident(upstream)
    ))
}

/// Forward-fill: replace NULL values with the most recent non-null
/// value within a group, ordered by a sort column. The classic
/// time-series gap-fill: missing readings get the previous reading.
/// Uses last_value(col IGNORE NULLS) over an unbounded preceding
/// window - DuckDB evaluates this in one pass.
fn build_fill_forward(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.fill_forward"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Forward Fill needs a column".to_string())?;
    let order_col = string_prop(props, "orderBy")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Forward Fill needs an orderBy column".to_string())?;
    let partition: Vec<String> = columns_from_props(props, "partitionBy").unwrap_or_default();
    let partition_clause = if partition.is_empty() {
        String::new()
    } else {
        let cols = partition
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        format!("PARTITION BY {} ", cols)
    };
    let qcol = quote_ident(&column);
    Ok(format!(
        "SELECT * REPLACE (last_value({col} IGNORE NULLS) OVER ({part}ORDER BY {ord} ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS {col}) FROM {up}",
        col = qcol,
        part = partition_clause,
        ord = quote_ident(&order_col),
        up = quote_ident(upstream)
    ))
}

/// Numeric Bucketize: bin a numeric column into N equal-width
/// buckets between low and high. Output is 1..N for in-range values,
/// 0 for below-low, N+1 for above-high (PostgreSQL width_bucket
/// semantics). DuckDB core doesn't ship width_bucket as a scalar
/// function (only the Postgres extension defines it), so we expand
/// to the explicit floor((v - low) / step) + 1 form, which works on
/// every DuckDB build.
fn build_bucketize(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.num.bucketize"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Bucketize needs a column".to_string())?;
    let low = props
        .get("low")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "Bucketize needs a low bound".to_string())?;
    let high = props
        .get("high")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| "Bucketize needs a high bound".to_string())?;
    if high <= low {
        return Err("Bucketize needs high > low".to_string());
    }
    let buckets = props
        .get("buckets")
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0)
        .unwrap_or(10);
    let step = (high - low) / buckets as f64;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_bucket", column));
    let qcol = quote_ident(&column);
    Ok(format!(
        "SELECT *, CASE WHEN CAST({col} AS DOUBLE) < {low} THEN 0 WHEN CAST({col} AS DOUBLE) >= {high} THEN {overflow} ELSE CAST(floor((CAST({col} AS DOUBLE) - {low}) / {step}) AS INTEGER) + 1 END AS {out} FROM {up}",
        col = qcol,
        low = low,
        high = high,
        step = step,
        overflow = buckets + 1,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// JSON Array Agg: collapse multiple rows into a JSON array per group
/// via json_group_array. With no groupBy, produces one row with the
/// whole input as a single array.
fn build_json_array_agg(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.json.array_agg"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "JSON Array Agg needs a column".to_string())?;
    let group_by: Vec<String> = columns_from_props(props, "groupBy").unwrap_or_default();
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_array", column));
    let agg = format!("json_group_array({}) AS {}", quote_ident(&column), quote_ident(&output));
    if group_by.is_empty() {
        Ok(format!("SELECT {} FROM {}", agg, quote_ident(upstream)))
    } else {
        let cols = group_by
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(format!(
            "SELECT {cols}, {agg} FROM {up} GROUP BY {cols}",
            cols = cols,
            agg = agg,
            up = quote_ident(upstream)
        ))
    }
}

/// Text Similarity: pairwise string similarity between two columns
/// via levenshtein (edit distance), damerau_levenshtein (also counts
/// transpositions), jaccard (set similarity of trigrams), or
/// jaro_winkler_similarity (0..1, weighted toward shared prefixes).
/// The first two are integer distances (lower = more similar); the
/// last two are normalized similarities (higher = more similar).
fn build_text_similarity(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.text.similarity"))?;
    let left_col = string_prop(props, "leftColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Text Similarity needs a left column".to_string())?;
    let right_col = string_prop(props, "rightColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Text Similarity needs a right column".to_string())?;
    let algo = string_prop(props, "algorithm").unwrap_or_else(|| "levenshtein".into());
    let fn_name = match algo.as_str() {
        "damerau_levenshtein" => "damerau_levenshtein",
        "jaccard" => "jaccard",
        "jaro_winkler" => "jaro_winkler_similarity",
        _ => "levenshtein",
    };
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_{}_{}_score", left_col, right_col, fn_name));
    Ok(format!(
        "SELECT *, {fn}(CAST({l} AS VARCHAR), CAST({r} AS VARCHAR)) AS {out} FROM {up}",
        fn = fn_name,
        l = quote_ident(&left_col),
        r = quote_ident(&right_col),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Spatial Join: a two-input join whose predicate is a spatial
/// relationship between left.geom and right.geom (intersects /
/// contains / within / touches / crosses / overlaps / equals).
/// Different from xf.geo.intersects which is a one-input enrichment
/// against a fixed target. The classic "orders inside delivery zone"
/// example is `left=orders.point JOIN right=zones.polygon ON
/// ST_Within(orders.point, zones.polygon)`.
fn build_spatial_join(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let left = inputs
        .main()
        .ok_or_else(|| "Spatial Join needs a driving input".to_string())?;
    let right = inputs
        .first_lookup()
        .ok_or_else(|| "Spatial Join needs a lookup input".to_string())?;
    let left_col = string_prop(props, "leftGeomColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Spatial Join needs leftGeomColumn".to_string())?;
    let right_col = string_prop(props, "rightGeomColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Spatial Join needs rightGeomColumn".to_string())?;
    let relation = string_prop(props, "relation").unwrap_or_else(|| "intersects".into());
    let fn_name = match relation.as_str() {
        "contains" => "ST_Contains",
        "within" => "ST_Within",
        "touches" => "ST_Touches",
        "crosses" => "ST_Crosses",
        "overlaps" => "ST_Overlaps",
        "equals" => "ST_Equals",
        _ => "ST_Intersects",
    };
    let kind = match string_prop(props, "joinType").as_deref() {
        Some("left") => "LEFT",
        _ => "INNER",
    };
    Ok(format!(
        "SELECT m.*, r.* FROM {} m {} JOIN {} r ON {}(CAST(m.{} AS GEOMETRY), CAST(r.{} AS GEOMETRY))",
        quote_ident(left),
        kind,
        quote_ident(right),
        fn_name,
        quote_ident(&left_col),
        quote_ident(&right_col)
    ))
}

/// Spatial Intersects: add a boolean column with ST_Intersects(geom,
/// target). Pair with xf.filter downstream to keep only the rows that
/// overlap a polygon (e.g. "orders inside a delivery zone"). Two-input
/// spatial joins land later as xf.join.spatial.
fn build_geo_intersects(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.geo.intersects"))?;
    let column = string_prop(props, "geomColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Spatial Intersects needs a geometry column".to_string())?;
    let target = string_prop(props, "targetWkt")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Spatial Intersects needs a target geometry (WKT)".to_string())?;
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "intersects".into());
    Ok(format!(
        "SELECT *, ST_Intersects(CAST({col} AS GEOMETRY), ST_GeomFromText('{target}')) AS {out} FROM {up}",
        col = quote_ident(&column),
        target = target.replace('\'', "''"),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Hash: add a column with the md5 / sha1 / sha256 digest (or a
/// DuckDB `hash()` int64) of an input column. Useful for deterministic
/// IDs from natural keys, one-way PII masking, and fingerprinting.
fn build_hash(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.hash"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Hash needs a column".to_string())?;
    let algo = string_prop(props, "algorithm").unwrap_or_else(|| "md5".into());
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_hash", column));
    let fn_name = match algo.as_str() {
        "sha1" => "sha1",
        "sha256" => "sha256",
        "hash" => "hash",
        _ => "md5",
    };
    Ok(format!(
        "SELECT *, {fn_name}(CAST({col} AS VARCHAR)) AS {out} FROM {up}",
        col = quote_ident(&column),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Assert: hard-fail the pipeline if any row violates the given SQL
/// predicate. Unlike qa.* validators which route bad rows to a reject
/// port, this stops the whole pipeline so a downstream sink never
/// sees a partial result. Rows pass through unchanged. The CASE
/// invokes DuckDB's error() in the ELSE branch; the error surfaces
/// as the stage's failure with the user's message. The outer
/// EXCLUDE strips the temporary marker column so downstream stages
/// see the original schema.
fn build_assert(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.assert"))?;
    let predicate = string_prop(props, "predicate")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Assert needs a SQL predicate (e.g. amount >= 0)".to_string())?;
    let raw_msg = string_prop(props, "message")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Assertion violated: {}", predicate));
    let msg = sql_escape(&raw_msg);
    // Aggregate the predicate into a single boolean across the whole
    // input via bool_and, then evaluate one CASE in a MATERIALIZED CTE.
    // This pattern (rather than a per-row CASE in the projection) is the
    // only shape DuckDB reliably keeps - the optimizer prunes unused
    // projection columns even when their CASE has error() in the ELSE,
    // which on some platforms (notably Windows release builds in CI)
    // means the assertion silently never fires. The aggregate has no
    // such hiding place; bool_and is forced to scan every row, and the
    // outer SELECT uses the CTE's value in WHERE so the CTE is
    // genuinely materialized. COALESCE(..., TRUE) treats an empty
    // input as a pass (vacuously true).
    Ok(format!(
        "WITH _duckle_assert AS MATERIALIZED (SELECT CASE WHEN COALESCE(bool_and(CAST(({pred}) AS BOOLEAN)), TRUE) THEN 'ok' ELSE error('{msg}') END AS result FROM {up}) SELECT u.* FROM {up} u WHERE (SELECT result FROM _duckle_assert) IS NOT NULL",
        pred = predicate,
        msg = msg,
        up = quote_ident(upstream)
    ))
}

/// URL Parse: pull a single component out of a URL string column via
/// a fixed regex. Picks one of scheme / host / port / path / query /
/// fragment with the `kind` prop, mirrors xf.ip.parse's shape.
fn build_url_parse(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.url.parse"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "URL Parse needs an input column".to_string())?;
    let kind = string_prop(props, "kind").unwrap_or_else(|| "host".into());
    // Single regex with named groups for every URL component. The
    // expression intentionally accepts URLs with and without a scheme.
    let url_re = "^(?:([a-zA-Z][a-zA-Z0-9+.-]*)://)?([^:/?#]*)(?::([0-9]+))?(/[^?#]*)?(?:\\?([^#]*))?(?:#(.*))?$";
    let group_idx: i64 = match kind.as_str() {
        "scheme" => 1,
        "host" => 2,
        "port" => 3,
        "path" => 4,
        "query" => 5,
        "fragment" => 6,
        _ => 2,
    };
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_{}", column, kind));
    Ok(format!(
        "SELECT *, regexp_extract(CAST({col} AS VARCHAR), '{re}', {idx}) AS {out} FROM {up}",
        col = quote_ident(&column),
        re = sql_escape(url_re),
        idx = group_idx,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// IP Parse: CAST a text/IP column to INET and extract a single
/// component via the inet extension. `kind` picks which piece comes
/// out (host / family / broadcast / netmask / hostmask / masklen /
/// network), so one row gives one output column and the upstream
/// schema is untouched. The CAST handles both bare addresses
/// (1.2.3.4 / ::1) and CIDR notation (10.0.0.0/8).
fn build_ip_parse(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs.main().ok_or_else(|| missing_input_msg("xf.ip.parse"))?;
    let column = string_prop(props, "column")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "IP Parse needs an input column".to_string())?;
    let kind = string_prop(props, "kind").unwrap_or_else(|| "host".into());
    let fn_name = match kind.as_str() {
        "family" => "family",
        "broadcast" => "broadcast",
        "netmask" => "netmask",
        "hostmask" => "hostmask",
        "masklen" => "masklen",
        "network" => "network",
        _ => "host",
    };
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}_{}", column, fn_name));
    Ok(format!(
        "SELECT *, {fn_name}(CAST({col} AS INET)) AS {out} FROM {up}",
        col = quote_ident(&column),
        out = quote_ident(&output),
        up = quote_ident(upstream)
    ))
}

/// Vector Similarity Search via the DuckDB vss extension. Adds a
/// similarity score column to each upstream row (against a fixed query
/// vector) and optionally returns only the top-K most similar rows.
/// The vector column is CAST to FLOAT[dim] so vss accepts it; the
/// target vector is embedded as an array literal (validated as a JSON
/// array of numbers at plan time).
fn build_vector_search(inputs: &NodeInputs, props: &JsonValue) -> Result<String, String> {
    let upstream = inputs
        .main()
        .ok_or_else(|| missing_input_msg("xf.ai.vector_search"))?;
    let column = string_prop(props, "vectorColumn")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Vector Search needs a vector column".to_string())?;
    let target = string_prop(props, "targetVector")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Vector Search needs a target vector (JSON array of floats)".to_string())?;
    let dim = props
        .get("dimension")
        .and_then(|v| v.as_u64())
        .filter(|d| *d > 0)
        .ok_or_else(|| "Vector Search needs a positive dimension".to_string())?;
    let metric = string_prop(props, "distanceMetric").unwrap_or_else(|| "cosine".into());
    let top_k = props
        .get("topK")
        .and_then(|v| v.as_u64())
        .filter(|k| *k > 0);
    let output = string_prop(props, "outputColumn")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "similarity_score".into());

    let vec_vals: Vec<f64> = serde_json::from_str(&target)
        .map_err(|e| format!("Vector Search: targetVector must be a JSON array of numbers ({})", e))?;
    if vec_vals.len() as u64 != dim {
        return Err(format!(
            "Vector Search: target vector has {} elements but dimension is {}",
            vec_vals.len(),
            dim
        ));
    }
    let target_literal = format!(
        "[{}]::FLOAT[{}]",
        vec_vals
            .iter()
            .map(|f| format!("{}", f))
            .collect::<Vec<_>>()
            .join(","),
        dim
    );
    let col_cast = format!("CAST({} AS FLOAT[{}])", quote_ident(&column), dim);
    let (fn_name, order_dir) = match metric.as_str() {
        "l2" | "distance" => ("array_distance", "ASC"),
        "inner_product" | "dot" => ("array_inner_product", "DESC"),
        _ => ("array_cosine_similarity", "DESC"),
    };
    let score_expr = format!("{fn_name}({col_cast}, {target_literal})");
    let mut sql = format!(
        "SELECT *, {score} AS {out} FROM {up}",
        score = score_expr,
        out = quote_ident(&output),
        up = quote_ident(upstream)
    );
    if let Some(k) = top_k {
        sql = format!(
            "{sql} ORDER BY {out} {dir} LIMIT {k}",
            out = quote_ident(&output),
            dir = order_dir
        );
    }
    Ok(sql)
}

/// Geospatial source via the DuckDB spatial extension. ST_Read is
/// GDAL-backed, so the same builder handles GeoJSON, Shapefile,
/// GeoPackage, KML, GPX, and many more (format auto-detected by file
/// extension). The geometry column comes through as binary; downstream
/// transforms (e.g. ST_AsText) can convert it.
fn build_spatial_source(props: &JsonValue) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    format!("SELECT * FROM ST_Read('{}')", sql_escape(&path))
}

/// Iceberg source via the DuckDB iceberg extension's `iceberg_scan`.
/// The `path` is the iceberg table location (a local directory or an
/// `s3://...` URL backed by a cloud SECRET created elsewhere).
fn build_iceberg_source(props: &JsonValue) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    format!("SELECT * FROM iceberg_scan('{}')", sql_escape(&path))
}

/// Delta Lake source via the DuckDB delta extension's `delta_scan`.
fn build_delta_source(props: &JsonValue) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    format!("SELECT * FROM delta_scan('{}')", sql_escape(&path))
}

/// Excel (.xlsx) source via DuckDB v1.2+ `read_xlsx`. Supports an
/// optional `sheet` form field (omitted defaults to the first sheet)
/// and a `hasHeader` toggle.
fn build_excel_source(props: &JsonValue) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    let mut args = vec![format!("'{}'", sql_escape(&path))];
    if let Some(sheet) = string_prop(props, "sheet").filter(|s| !s.is_empty()) {
        args.push(format!("sheet = '{}'", sql_escape(&sheet)));
    }
    if let Some(has_header) = props.get("hasHeader").and_then(JsonValue::as_bool) {
        args.push(format!("header = {}", has_header));
    }
    format!("SELECT * FROM read_xlsx({})", args.join(", "))
}

/// Cloud sources (S3 / GCS / Azure Blob / HTTP). DuckDB's httpfs +
/// azure extensions let us read these directly via the same
/// read_csv_auto / read_parquet / read_json_auto family of functions.
/// Format is inferred from the URL extension unless the user picks one.
fn build_cloud_source(scheme: &str, props: &JsonValue) -> String {
    let path = string_prop(props, "path")
        .or_else(|| string_prop(props, "url"))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            // The storage form supplies bucket + key rather than a full
            // URL; assemble one using the connector's scheme.
            let bucket = string_prop(props, "bucket").filter(|s| !s.is_empty())?;
            let key = string_prop(props, "key").unwrap_or_default();
            let prefix = match scheme {
                "s3" => "s3://",
                "gcs" => "gs://",
                "azureblob" => "az://",
                _ => "https://",
            };
            Some(format!("{}{}/{}", prefix, bucket, key.trim_start_matches('/')))
        })
        .unwrap_or_default();
    let override_fmt = string_prop(props, "format");
    let lower = path.to_ascii_lowercase();
    let chosen = override_fmt.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        if lower.ends_with(".parquet") || lower.ends_with(".pq") {
            "parquet".into()
        } else if lower.ends_with(".json")
            || lower.ends_with(".jsonl")
            || lower.ends_with(".ndjson")
        {
            "json".into()
        } else if lower.ends_with(".tsv") {
            "tsv".into()
        } else {
            "csv".into()
        }
    });
    match chosen.as_str() {
        "parquet" => format!("SELECT * FROM read_parquet('{}')", sql_escape(&path)),
        "json" => format!("SELECT * FROM read_json_auto('{}')", sql_escape(&path)),
        "tsv" => format!(
            "SELECT * FROM read_csv_auto('{}', header=true, delim='\\t')",
            sql_escape(&path)
        ),
        _ => format!(
            "SELECT * FROM read_csv_auto('{}', header=true)",
            sql_escape(&path)
        ),
    }
}

// ---- Sinks --------------------------------------------------------------

fn build_sink_sql(
    component_id: &str,
    props: &JsonValue,
    from_view: &str,
) -> Result<String, EngineError> {
    match component_id {
        "snk.csv" => Ok(build_csv_sink(props, from_view)),
        "snk.tsv" => {
            let mut p = props.clone();
            if let Some(obj) = p.as_object_mut() {
                obj.insert("delimiter".into(), JsonValue::String("\t".into()));
            }
            Ok(build_csv_sink(&p, from_view))
        }
        "snk.parquet" => Ok(build_parquet_sink(props, from_view)),
        "snk.json" | "snk.jsonl" => Ok(build_json_sink(props, from_view)),
        "snk.s3" | "snk.gcs" | "snk.azureblob" => Ok(build_cloud_sink(props, from_view)),
        "snk.sqlite" | "snk.duckdb" => Ok(build_db_sink(props, from_view)),
        "snk.postgres" | "snk.cockroach" | "snk.mysql" | "snk.mariadb"
        | "snk.motherduck" | "snk.ducklake" | "snk.pgvector"
        | "snk.redshift" | "snk.bigquery" => build_relational_sink(component_id, props, from_view),
        "snk.excel" => Ok(build_excel_sink(props, from_view)),
        "snk.spatial" => Ok(build_spatial_sink(props, from_view)),
        "snk.iceberg" => Ok(build_iceberg_sink(props, from_view)),
        other => Err(EngineError::Unsupported(format!(
            "Sink '{}' is not yet implemented",
            other
        ))),
    }
}

/// Cloud sink - COPY a view out to an s3:// / gs:// / az:// URL.
/// DuckDB's httpfs handles the upload; credentials come from the
/// SECRET wired up in execute_pipeline_with_events. Format is inferred
/// from the URL extension unless overridden.
fn build_cloud_sink(props: &JsonValue, from_view: &str) -> String {
    let path = string_prop(props, "path")
        .or_else(|| string_prop(props, "url"))
        .unwrap_or_default();
    let override_fmt = string_prop(props, "format").filter(|s| !s.is_empty());
    let lower = path.to_ascii_lowercase();
    let chosen = override_fmt.unwrap_or_else(|| {
        if lower.ends_with(".parquet") || lower.ends_with(".pq") {
            "parquet".into()
        } else if lower.ends_with(".json") || lower.ends_with(".jsonl") || lower.ends_with(".ndjson") {
            "json".into()
        } else {
            "csv".into()
        }
    });
    let options = match chosen.as_str() {
        "parquet" => "FORMAT PARQUET, COMPRESSION 'ZSTD'".to_string(),
        "json" => "FORMAT JSON".to_string(),
        _ => "FORMAT CSV, HEADER true".to_string(),
    };
    format!(
        "COPY (SELECT * FROM {}) TO '{}' ({})",
        quote_ident(from_view),
        sql_escape(&path),
        options
    )
}

fn build_csv_sink(props: &JsonValue, from_view: &str) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    // The sink form writes `writeHeader`; the source uses `hasHeader`.
    let header = props
        .get("writeHeader")
        .or_else(|| props.get("hasHeader"))
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    let delim = string_prop(props, "delimiter").unwrap_or_else(|| ",".into());
    let null_val = string_prop(props, "nullValue").unwrap_or_default();
    let mut options = vec![
        "FORMAT CSV".to_string(),
        format!("HEADER {}", header),
        format!("DELIM '{}'", sql_escape(&delim)),
    ];
    if !null_val.is_empty() {
        options.push(format!("NULLSTR '{}'", sql_escape(&null_val)));
    }
    let partition = columns_from_props(props, "partitionBy").unwrap_or_default();
    if !partition.is_empty() {
        let cols = partition
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        options.push(format!("PARTITION_BY ({})", cols));
        options.push("OVERWRITE_OR_IGNORE".to_string());
    }
    format!(
        "COPY (SELECT * FROM {}) TO '{}' ({})",
        quote_ident(from_view),
        sql_escape(&path),
        options.join(", ")
    )
}

fn build_parquet_sink(props: &JsonValue, from_view: &str) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    let compression = string_prop(props, "compression").unwrap_or_else(|| "ZSTD".into());
    let partition = columns_from_props(props, "partitionBy").unwrap_or_default();
    let mut options = vec![
        "FORMAT PARQUET".to_string(),
        format!("COMPRESSION '{}'", sql_escape(&compression)),
    ];
    if !partition.is_empty() {
        let cols = partition
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        options.push(format!("PARTITION_BY ({})", cols));
        // DuckDB refuses to write into an existing partition directory
        // unless one of these is set; OVERWRITE_OR_IGNORE matches what
        // most ETL pipelines want (rewrite the slice we just emitted,
        // leave untouched siblings alone).
        options.push("OVERWRITE_OR_IGNORE".to_string());
    }
    format!(
        "COPY (SELECT * FROM {}) TO '{}' ({})",
        quote_ident(from_view),
        sql_escape(&path),
        options.join(", ")
    )
}

fn build_json_sink(props: &JsonValue, from_view: &str) -> String {
    let path = string_prop(props, "path").unwrap_or_default();
    let array = string_prop(props, "format")
        .map(|f| f.eq_ignore_ascii_case("array"))
        .unwrap_or(false);
    format!(
        "COPY (SELECT * FROM {}) TO '{}' (FORMAT JSON, ARRAY {})",
        quote_ident(from_view),
        sql_escape(&path),
        if array { "true" } else { "false" }
    )
}

// ---- Helpers ------------------------------------------------------------

fn columns_from_props(props: &JsonValue, key: &str) -> Option<Vec<String>> {
    props
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
}

fn string_prop(props: &JsonValue, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(JsonValue::as_str)
        .map(String::from)
}

/// Reads the `headers` key-value pairs from a HTTP connector's props.
/// Forms write them as either an object ({k: v}) or an array of
/// {key, value} entries; accept both shapes.
fn headers_from_props(props: &JsonValue) -> Vec<(String, String)> {
    let raw = match props.get("headers") {
        Some(v) => v,
        None => return Vec::new(),
    };
    if let Some(obj) = raw.as_object() {
        return obj
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect();
    }
    if let Some(arr) = raw.as_array() {
        return arr
            .iter()
            .filter_map(|item| {
                let k = item.get("key").and_then(|x| x.as_str())?;
                let v = item.get("value").and_then(|x| x.as_str())?;
                Some((k.to_string(), v.to_string()))
            })
            .collect();
    }
    Vec::new()
}

pub(crate) fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn duckle_type_to_duckdb(t: &str) -> String {
    match t.to_lowercase().as_str() {
        "string" | "varchar" | "text" => "VARCHAR".into(),
        "int32" | "int" | "integer" => "INTEGER".into(),
        "int64" | "bigint" => "BIGINT".into(),
        "float32" | "real" | "float" => "REAL".into(),
        "float64" | "double" => "DOUBLE".into(),
        "bool" | "boolean" => "BOOLEAN".into(),
        "date" => "DATE".into(),
        "timestamp" => "TIMESTAMP".into(),
        "time" => "TIME".into(),
        "decimal" => "DECIMAL(18,4)".into(),
        "json" => "JSON".into(),
        "binary" | "blob" => "BLOB".into(),
        other => other.to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pipeline_from_json(s: &str) -> PipelineDoc {
        serde_json::from_str(s).expect("valid pipeline JSON")
    }

    #[test]
    fn compiles_csv_filter_parquet() {
        let p = pipeline_from_json(
            r#"{
              "nodes": [
                {"id":"s1","position":{"x":0,"y":0},"data":{
                  "label":"CSV","componentId":"src.csv",
                  "properties":{"path":"/tmp/orders.csv","hasHeader":true}}},
                {"id":"f1","position":{"x":0,"y":0},"data":{
                  "label":"Filter","componentId":"xf.filter",
                  "properties":{"predicate":"status = 'paid'"}}},
                {"id":"k1","position":{"x":0,"y":0},"data":{
                  "label":"Parquet","componentId":"snk.parquet",
                  "properties":{"path":"/tmp/out.parquet"}}}
              ],
              "edges": [
                {"id":"e1","source":"s1","target":"f1",
                  "data":{"connectionType":"main"}},
                {"id":"e2","source":"f1","target":"k1",
                  "data":{"connectionType":"main"}}
              ]
            }"#,
        );
        let compiled = compile(&p).unwrap();
        assert_eq!(compiled.stages.len(), 3);
        assert_eq!(compiled.stages[0].node_id, "s1");
        assert!(compiled.stages[0]
            .sql
            .contains("read_csv_auto('/tmp/orders.csv'"));
        assert!(compiled.stages[1].sql.contains("WHERE status = 'paid'"));
        assert_eq!(compiled.stages[2].kind, StageKind::Sink);
        assert!(compiled.stages[2]
            .sql
            .contains("TO '/tmp/out.parquet' (FORMAT PARQUET"));
    }

    #[test]
    fn rejects_cycles() {
        let p = pipeline_from_json(
            r#"{
              "nodes":[
                {"id":"a","position":{"x":0,"y":0},"data":{"label":"A","componentId":"xf.filter","properties":{}}},
                {"id":"b","position":{"x":0,"y":0},"data":{"label":"B","componentId":"xf.filter","properties":{}}}
              ],
              "edges":[
                {"id":"e1","source":"a","target":"b","data":{"connectionType":"main"}},
                {"id":"e2","source":"b","target":"a","data":{"connectionType":"main"}}
              ]
            }"#,
        );
        assert!(compile(&p).is_err());
    }
}
