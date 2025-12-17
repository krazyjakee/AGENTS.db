use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::cache::CacheKeyAlg;
use crate::embedder::{Embedder, EmbedderMetadata, EmbeddingProfile};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayerMetadataV1 {
    pub v: u32,
    pub embedding_profile: EmbeddingProfile,
    pub cache_key_alg: CacheKeyAlg,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedder_metadata: Option<EmbedderMetadata>,
    pub tool_name: Option<String>,
    pub tool_version: Option<String>,
}

impl LayerMetadataV1 {
    pub fn new(embedding_profile: EmbeddingProfile) -> Self {
        Self {
            v: 1,
            embedding_profile,
            cache_key_alg: CacheKeyAlg::Sha256ProfileJsonV2NullContentUtf8,
            embedder_metadata: None,
            tool_name: None,
            tool_version: None,
        }
    }

    pub fn with_embedder_metadata(mut self, meta: EmbedderMetadata) -> Self {
        self.embedder_metadata = Some(meta);
        self
    }

    pub fn with_tool(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.tool_name = Some(name.into());
        self.tool_version = Some(version.into());
        self
    }

    pub fn to_json_bytes(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec(self).context("serialize layer metadata")
    }

    pub fn from_json_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(bytes).context("parse layer metadata")
    }
}

pub fn ensure_layer_metadata_compatible_with_embedder(
    file: &agentsdb_format::LayerFile,
    embedder: &dyn Embedder,
) -> anyhow::Result<()> {
    let Some(existing) = file.layer_metadata_bytes() else {
        return Ok(());
    };
    let existing =
        LayerMetadataV1::from_json_bytes(existing).context("parse existing layer metadata")?;
    if existing.embedding_profile != *embedder.profile() {
        anyhow::bail!(
            "embedder profile mismatch vs layer metadata (layer={}, existing={:?}, current={:?})",
            file.path().display(),
            existing.embedding_profile,
            embedder.profile()
        );
    }
    Ok(())
}
