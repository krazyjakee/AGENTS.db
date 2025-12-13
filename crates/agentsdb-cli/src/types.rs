use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(crate) struct ValidateJson<'a> {
    pub(crate) ok: bool,
    pub(crate) path: &'a str,
    pub(crate) error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CleanJson<'a> {
    pub(crate) root: &'a str,
    pub(crate) dry_run: bool,
    pub(crate) paths: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ListEntryJson {
    pub(crate) path: String,
    pub(crate) chunk_count: u64,
    pub(crate) file_length_bytes: u64,
}

#[derive(Serialize)]
pub(crate) struct InspectJson<'a> {
    pub(crate) path: &'a str,
    pub(crate) header: HeaderJson,
    pub(crate) sections: Vec<SectionJson>,
    pub(crate) string_count: u64,
    pub(crate) chunk_count: u64,
    pub(crate) embedding: EmbeddingJson,
    pub(crate) relationships: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct HeaderJson {
    pub(crate) magic: u32,
    pub(crate) version_major: u16,
    pub(crate) version_minor: u16,
    pub(crate) file_length_bytes: u64,
    pub(crate) section_count: u64,
    pub(crate) sections_offset: u64,
    pub(crate) flags: u64,
}

#[derive(Serialize)]
pub(crate) struct SectionJson {
    pub(crate) kind: String,
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

#[derive(Serialize)]
pub(crate) struct EmbeddingJson {
    pub(crate) row_count: u64,
    pub(crate) dim: u32,
    pub(crate) element_type: String,
    pub(crate) data_offset: u64,
    pub(crate) data_length: u64,
    pub(crate) quant_scale: f32,
}

#[derive(Serialize)]
pub(crate) struct SearchJson {
    pub(crate) query_dim: usize,
    pub(crate) k: usize,
    pub(crate) results: Vec<SearchResultJson>,
}

#[derive(Serialize)]
pub(crate) struct SearchResultJson {
    pub(crate) layer: String,
    pub(crate) id: u32,
    pub(crate) kind: String,
    pub(crate) score: f32,
    pub(crate) author: String,
    pub(crate) confidence: f32,
    pub(crate) created_at_unix_ms: u64,
    pub(crate) sources: Vec<String>,
    pub(crate) hidden_layers: Vec<String>,
    pub(crate) content: String,
}

#[derive(Deserialize)]
pub(crate) struct CompileInput {
    pub(crate) schema: CompileSchema,
    pub(crate) chunks: Vec<CompileChunk>,
}

#[derive(Deserialize)]
pub(crate) struct CompileSchema {
    pub(crate) dim: u32,
    pub(crate) element_type: String, // "f32" | "i8"
    pub(crate) quant_scale: Option<f32>,
}

#[derive(Deserialize)]
pub(crate) struct CompileChunk {
    pub(crate) id: u32,
    pub(crate) kind: String,
    pub(crate) content: String,
    pub(crate) author: String,
    pub(crate) confidence: f32,
    pub(crate) created_at_unix_ms: u64,
    #[serde(default)]
    pub(crate) embedding: Option<Vec<f32>>,
    #[serde(default)]
    pub(crate) sources: Vec<CompileSource>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum CompileSource {
    String(String),
    Chunk { chunk_id: u32 },
}

#[derive(Serialize)]
pub(crate) struct CollectOutput {
    pub(crate) schema: CompileSchemaOut,
    pub(crate) chunks: Vec<CollectChunk>,
}

#[derive(Serialize)]
pub(crate) struct CompileSchemaOut {
    pub(crate) dim: u32,
    pub(crate) element_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) quant_scale: Option<f32>,
}

#[derive(Serialize)]
pub(crate) struct CollectChunk {
    pub(crate) id: u32,
    pub(crate) kind: String,
    pub(crate) content: String,
    pub(crate) author: String,
    pub(crate) confidence: f32,
    pub(crate) created_at_unix_ms: u64,
    pub(crate) sources: Vec<CollectSource>,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum CollectSource {
    String(String),
}
