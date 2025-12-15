use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingProfile {
    pub backend: String,
    pub model: Option<String>,
    pub revision: Option<String>,
    pub dim: usize,
    #[serde(default)]
    pub output_norm: OutputNorm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputNorm {
    None,
    L2,
}

impl Default for OutputNorm {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedderMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_api_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_model_revision: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,

    /// Provider request parameters relevant to reproducibility (excludes raw inputs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request: Option<serde_json::Value>,
    /// Provider response metadata relevant to reproducibility (excludes embeddings payload).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_response: Option<serde_json::Value>,
    /// Selected response headers (e.g. request id, server version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_response_headers: Option<BTreeMap<String, String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

pub trait Embedder {
    fn profile(&self) -> &EmbeddingProfile;
    fn metadata(&self) -> EmbedderMetadata {
        EmbedderMetadata::default()
    }
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>>;
}
