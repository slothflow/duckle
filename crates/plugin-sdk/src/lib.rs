//! Duckle plugin SDK.
//!
//! Public Rust contract for shipping connectors, transforms, and engines.
//! Phase 1 exposes the schema-inspection contract so the desktop runtime
//! can ask any connector for the schema of an input it controls; richer
//! data-flow traits land as the execution layer matures.

use async_trait::async_trait;
use duckle_metadata::Schema;
use serde_json::Value as JsonValue;
use thiserror::Error;

/// What can go wrong during schema inspection.
#[derive(Debug, Error)]
pub enum InspectError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("config: {0}")]
    Config(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("other: {0}")]
    Other(String),
}

/// Result of inspecting a source - schema plus an optional preview.
#[derive(Debug, Clone)]
pub struct Inspection {
    pub schema: Schema,
    pub sample_rows: Vec<JsonValue>,
}

/// Anything that can describe the schema of an input it controls - files,
/// databases, APIs, streaming subscriptions. The connector receives its
/// already-validated configuration as a JSON value (the same shape the
/// frontend collects in its property form).
#[async_trait]
pub trait SchemaInspector: Send + Sync {
    /// Stable identifier matching the palette `componentId`
    /// (e.g. `"src.csv"`).
    fn component_id(&self) -> &str;

    async fn inspect(&self, config: JsonValue) -> Result<Inspection, InspectError>;
}

/// A connector that produces rows for a source-side use, or consumes
/// them for a sink. Phase 1 declares the trait so engines can be coded
/// against it; concrete read/write methods land alongside the execution
/// crate.
#[async_trait]
pub trait Connector: SchemaInspector {
    fn kind(&self) -> ConnectorKind;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorKind {
    Source,
    Sink,
}
