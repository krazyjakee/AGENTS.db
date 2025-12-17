#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ExportBundleV1 {
    /// Represents the top-level structure of a complete AGENTS.db export bundle (version 1).
    ///
    /// This bundle contains metadata about the export tool and a collection of exported layers.
    pub format: String, // "agentsdb.export.v1"
    pub tool: ExportToolInfo,
    pub layers: Vec<ExportLayerV1>,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ExportToolInfo {
    /// Provides information about the tool that generated the export.
    pub name: String,
    pub version: String,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ExportLayerV1 {
    /// Represents a single exported AGENTS.db layer (version 1).
    ///
    /// Contains the layer's path, optional logical ID, schema, metadata, and all its chunks.
    /// Layer path (as referenced by the caller; typically relative to a root).
    pub path: String,
    /// Optional logical layer id: "base" | "user" | "delta" | "local".
    #[cfg_attr(feature = "serde", serde(default))]
    pub layer: Option<String>,
    pub schema: ExportLayerSchemaV1,
    /// Raw JSON string (if present in the layer file).
    #[cfg_attr(feature = "serde", serde(default))]
    pub layer_metadata_json: Option<String>,
    pub chunks: Vec<ExportChunkV1>,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ExportLayerSchemaV1 {
    /// Represents the schema of an exported AGENTS.db layer (version 1).
    ///
    /// This includes the embedding dimension, element type, and quantization scale.
    pub dim: u32,
    pub element_type: String, // "f32" | "i8"
    pub quant_scale: f32,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct ExportChunkV1 {
    /// Represents a single exported chunk of data (version 1).
    ///
    /// Contains the chunk's ID, kind, content, author, confidence, creation timestamp,
    /// sources, optional embedding, and content SHA-256 hash.
    pub id: u32,
    pub kind: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub content: Option<String>,
    pub author: String, // "human" | "mcp"
    pub confidence: f32,
    pub created_at_unix_ms: u64,
    pub sources: Vec<ExportSourceV1>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub embedding: Option<Vec<f32>>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub content_sha256: Option<String>, // 64 lowercase hex chars
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", serde(tag = "type"))]
pub enum ExportSourceV1 {
    /// Represents a source reference for an exported chunk (version 1).
    ///
    /// Can be either a reference to another chunk by its ID or a free-form string.
    #[cfg_attr(feature = "serde", serde(rename = "chunk_id"))]
    ChunkId { id: u32 },
    #[cfg_attr(feature = "serde", serde(rename = "source_string"))]
    SourceString { value: String },
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", serde(tag = "type"))]
pub enum ExportNdjsonRecordV1 {
    /// Represents a single record in the NDJSON export format (version 1).
    ///
    /// This enum allows for streaming export of header, layer metadata, and individual chunks.
    #[cfg_attr(feature = "serde", serde(rename = "header"))]
    Header {
        format: String, // "agentsdb.export.ndjson.v1"
        tool: ExportToolInfo,
    },
    #[cfg_attr(feature = "serde", serde(rename = "layer"))]
    Layer {
        path: String,
        #[cfg_attr(feature = "serde", serde(default))]
        layer: Option<String>,
        schema: ExportLayerSchemaV1,
        #[cfg_attr(feature = "serde", serde(default))]
        layer_metadata_json: Option<String>,
    },
    #[cfg_attr(feature = "serde", serde(rename = "chunk"))]
    Chunk {
        layer_path: String,
        chunk: ExportChunkV1,
    },
}
