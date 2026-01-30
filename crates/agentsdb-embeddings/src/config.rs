use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::cache::DiskEmbeddingCache;
use crate::embedder::Embedder;
use crate::hash::HashEmbedder;

pub const KIND_OPTIONS: &str = "options";

pub const DEFAULT_LOCAL_MODEL: &str = "all-minilm-l6-v2";
pub const DEFAULT_LOCAL_REVISION: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModelRevision {
    pub model: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChecksumPin {
    pub model: String,
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowlistOp {
    Add,
    Remove,
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksumAllowlistRecord {
    pub op: AllowlistOp,
    pub entries: Vec<ModelChecksumPin>,
}

#[derive(Debug, Clone)]
pub struct StandardLayerPaths {
    pub base: std::path::PathBuf,
    pub user: std::path::PathBuf,
    pub delta: std::path::PathBuf,
    pub local: std::path::PathBuf,
}

pub fn standard_layer_paths_for_dir(dir: &std::path::Path) -> StandardLayerPaths {
    StandardLayerPaths {
        base: dir.join("AGENTS.db"),
        user: dir.join("AGENTS.user.db"),
        delta: dir.join("AGENTS.delta.db"),
        local: dir.join("AGENTS.local.db"),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingOptionsPatch {
    pub backend: Option<String>,
    pub model: Option<String>,
    pub revision: Option<String>,
    /// Optional local model path (directory or file) for offline/local backends.
    pub model_path: Option<String>,
    /// Optional expected SHA-256 (hex) for local model bytes (e.g. downloaded ONNX).
    pub model_sha256: Option<String>,
    pub dim: Option<usize>,
    pub api_base: Option<String>,
    pub api_key_env: Option<String>,
    pub cache_enabled: Option<bool>,
    pub cache_dir: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OptionsRecord {
    pub embedding: Option<EmbeddingOptionsPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum_allowlist: Option<ChecksumAllowlistRecord>,
}

#[derive(Debug, Clone)]
pub struct ResolvedEmbeddingOptions {
    pub backend: String,
    pub model: Option<String>,
    pub revision: Option<String>,
    pub model_path: Option<String>,
    pub model_sha256: Option<String>,
    pub dim: Option<usize>,
    pub api_base: Option<String>,
    pub api_key_env: Option<String>,
    pub cache_enabled: bool,
    pub cache_dir: Option<String>,
    pub checksum_allowlist: BTreeMap<ModelRevision, String>,
}

impl ResolvedEmbeddingOptions {
    pub fn into_embedder(
        self,
        fallback_dim: usize,
    ) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
        let dim = self.dim.unwrap_or(fallback_dim);
        let inner: Box<dyn Embedder + Send + Sync> = match self.backend.as_str() {
            "hash" => Box::new(HashEmbedder::new(dim)),
            "openai" => {
                #[cfg(feature = "openai")]
                {
                    let model = self
                        .model
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("openai backend requires model"))?;
                    crate::backends::openai_embedder(
                        dim,
                        model,
                        self.api_base.as_deref(),
                        self.api_key_env.as_deref(),
                    )?
                }
                #[cfg(not(feature = "openai"))]
                {
                    anyhow::bail!(
                        "embedding backend \"openai\" is not enabled in this build (rebuild with cargo feature \"agentsdb-embeddings/openai\")"
                    )
                }
            }
            "voyage" => {
                #[cfg(feature = "voyage")]
                {
                    let model = self
                        .model
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("voyage backend requires model"))?;
                    crate::backends::voyage_embedder(
                        dim,
                        model,
                        self.api_base.as_deref(),
                        self.api_key_env.as_deref(),
                    )?
                }
                #[cfg(not(feature = "voyage"))]
                {
                    anyhow::bail!(
                        "embedding backend \"voyage\" is not enabled in this build (rebuild with cargo feature \"agentsdb-embeddings/voyage\")"
                    )
                }
            }
            "cohere" => {
                #[cfg(feature = "cohere")]
                {
                    let model = self
                        .model
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("cohere backend requires model"))?;
                    crate::backends::cohere_embedder(
                        dim,
                        model,
                        self.api_base.as_deref(),
                        self.api_key_env.as_deref(),
                    )?
                }
                #[cfg(not(feature = "cohere"))]
                {
                    anyhow::bail!(
                        "embedding backend \"cohere\" is not enabled in this build (rebuild with cargo feature \"agentsdb-embeddings/cohere\")"
                    )
                }
            }
            "ort" => {
                #[cfg(feature = "ort")]
                {
                    let model = self.model.as_deref().unwrap_or(DEFAULT_LOCAL_MODEL);
                    let revision = self
                        .revision
                        .as_deref()
                        .unwrap_or(DEFAULT_LOCAL_REVISION);
                    let expected_sha256 = match self.model_sha256.as_deref() {
                        Some(v) => Some(v),
                        None => self
                            .checksum_allowlist
                            .get(&ModelRevision {
                                model: model.to_string(),
                                revision: revision.to_string(),
                            })
                            .map(|v| v.as_str()),
                    };
                    crate::backends::local_fastembed_embedder(
                        "ort",
                        dim,
                        model,
                        Some(revision),
                        self.model_path.as_deref(),
                        expected_sha256,
                    )?
                }
                #[cfg(not(feature = "ort"))]
                {
                    anyhow::bail!(
                        "embedding backend \"ort\" is not enabled in this build (rebuild with cargo feature \"agentsdb-embeddings/ort\")"
                    )
                }
            }
            "candle" => {
                #[cfg(feature = "candle")]
                {
                    let model = self.model.as_deref().unwrap_or(DEFAULT_LOCAL_MODEL);
                    let revision = self
                        .revision
                        .as_deref()
                        .unwrap_or(DEFAULT_LOCAL_REVISION);
                    let expected_sha256 = match self.model_sha256.as_deref() {
                        Some(v) => Some(v),
                        None => self
                            .checksum_allowlist
                            .get(&ModelRevision {
                                model: model.to_string(),
                                revision: revision.to_string(),
                            })
                            .map(|v| v.as_str()),
                    };
                    crate::backends::local_candle_embedder(
                        dim,
                        model,
                        Some(revision),
                        expected_sha256,
                    )?
                }
                #[cfg(not(feature = "candle"))]
                {
                    anyhow::bail!(
                        "embedding backend \"candle\" is not enabled in this build (rebuild with cargo feature \"agentsdb-embeddings/candle\")"
                    )
                }
            }
            "anthropic" => {
                #[cfg(feature = "anthropic")]
                {
                    let model = self
                        .model
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("anthropic backend requires model"))?;
                    crate::backends::anthropic_embedder(
                        dim,
                        model,
                        self.api_base.as_deref(),
                        self.api_key_env.as_deref(),
                    )?
                }
                #[cfg(not(feature = "anthropic"))]
                {
                    anyhow::bail!(
                        "embedding backend \"anthropic\" is not enabled in this build (rebuild with cargo feature \"agentsdb-embeddings/anthropic\")"
                    )
                }
            }
            "bedrock" => {
                #[cfg(feature = "bedrock")]
                {
                    let model = self
                        .model
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("bedrock backend requires model"))?;
                    crate::backends::bedrock_embedder(
                        dim,
                        model,
                        self.api_base.as_deref(),
                        self.api_key_env.as_deref(),
                    )?
                }
                #[cfg(not(feature = "bedrock"))]
                {
                    anyhow::bail!(
                        "embedding backend \"bedrock\" is not enabled in this build (rebuild with cargo feature \"agentsdb-embeddings/bedrock\")"
                    )
                }
            }
            "gemini" => {
                #[cfg(feature = "gemini")]
                {
                    let model = self
                        .model
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("gemini backend requires model"))?;
                    crate::backends::gemini_embedder(
                        dim,
                        model,
                        self.api_base.as_deref(),
                        self.api_key_env.as_deref(),
                    )?
                }
                #[cfg(not(feature = "gemini"))]
                {
                    anyhow::bail!(
                        "embedding backend \"gemini\" is not enabled in this build (rebuild with cargo feature \"agentsdb-embeddings/gemini\")"
                    )
                }
            }
            other => anyhow::bail!(
                "unknown embedding backend {other:?} (supported: \"hash\", \"candle\", \"ort\", \"openai\", \"voyage\", \"cohere\", \"anthropic\", \"bedrock\", \"gemini\")"
            ),
        };

        if !self.cache_enabled {
            return Ok(inner);
        }

        let cache_dir = match self.cache_dir {
            Some(v) => std::path::PathBuf::from(v),
            None => DiskEmbeddingCache::default_dir().context("resolve default cache dir")?,
        };
        let cache = DiskEmbeddingCache::new(cache_dir).context("init embedding cache")?;
        Ok(Box::new(CachedEmbedder { inner, cache }))
    }
}

struct CachedEmbedder {
    inner: Box<dyn Embedder + Send + Sync>,
    cache: DiskEmbeddingCache,
}

impl Embedder for CachedEmbedder {
    fn profile(&self) -> &crate::embedder::EmbeddingProfile {
        self.inner.profile()
    }

    fn metadata(&self) -> crate::embedder::EmbedderMetadata {
        self.inner.metadata()
    }

    fn embed(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let dim = self.profile().dim;
        let mut out: Vec<Option<Vec<f32>>> = vec![None; inputs.len()];
        let mut misses: Vec<(usize, &str)> = Vec::new();

        for (i, s) in inputs.iter().enumerate() {
            let key = crate::cache::cache_key_hex(self.profile(), s).context("cache key")?;
            if let Some(v) = self.cache.load_f32(&key).context("cache read")? {
                if v.len() == dim {
                    out[i] = Some(v);
                    continue;
                }
            }
            misses.push((i, s));
        }

        if !misses.is_empty() {
            let miss_inputs: Vec<String> = misses.iter().map(|(_, s)| (*s).to_string()).collect();
            let miss_embeds = self
                .inner
                .embed(&miss_inputs)
                .context("embed cache misses")?;
            if miss_embeds.len() != misses.len() {
                anyhow::bail!(
                    "embedder returned {} embeddings for {} inputs",
                    miss_embeds.len(),
                    misses.len()
                );
            }
            for ((idx, s), emb) in misses.into_iter().zip(miss_embeds.into_iter()) {
                if emb.len() != dim {
                    anyhow::bail!(
                        "embedder returned embedding of dim={} but expected dim={}",
                        emb.len(),
                        dim
                    );
                }
                let key = crate::cache::cache_key_hex(self.profile(), s).context("cache key")?;
                self.cache
                    .store_f32(&key, self.profile(), &emb)
                    .context("cache write")?;
                out[idx] = Some(emb);
            }
        }

        Ok(out
            .into_iter()
            .map(|v| v.unwrap_or_else(|| vec![0.0; dim]))
            .collect())
    }
}

pub fn roll_up_embedding_options(
    layers_high_to_low: &[Option<&agentsdb_format::LayerFile>],
) -> anyhow::Result<ResolvedEmbeddingOptions> {
    let mut out = ResolvedEmbeddingOptions {
        backend: "hash".into(),
        model: None,
        revision: None,
        model_path: None,
        model_sha256: None,
        dim: None,
        api_base: None,
        api_key_env: None,
        cache_enabled: false,
        cache_dir: None,
        checksum_allowlist: BTreeMap::new(),
    };

    // Allowlist rolls up from low->high (base < delta < user < local), applying append-only ops.
    // This permits local layers to override (add/remove/clear) pins defined by lower layers.
    let mut allowlist: BTreeMap<ModelRevision, String> = BTreeMap::new();
    for layer_opt in layers_high_to_low.iter().rev() {
        let Some(layer) = layer_opt else { continue };
        for chunk in layer.chunks() {
            let chunk = chunk.context("read chunk")?;
            if chunk.kind != KIND_OPTIONS {
                continue;
            }
            let record: OptionsRecord =
                serde_json::from_str(chunk.content).context("parse options JSON")?;
            let Some(op) = record.checksum_allowlist else {
                continue;
            };
            match op.op {
                AllowlistOp::Clear => allowlist.clear(),
                AllowlistOp::Add => {
                    for e in op.entries {
                        let sha256 = e.sha256.ok_or_else(|| {
                            anyhow::anyhow!(
                                "allowlist add entry missing sha256 (model={:?} revision={:?})",
                                e.model,
                                e.revision
                            )
                        })?;
                        allowlist.insert(
                            ModelRevision {
                                model: e.model,
                                revision: e.revision,
                            },
                            sha256,
                        );
                    }
                }
                AllowlistOp::Remove => {
                    for e in op.entries {
                        allowlist.remove(&ModelRevision {
                            model: e.model,
                            revision: e.revision,
                        });
                    }
                }
            }
        }
    }
    out.checksum_allowlist = allowlist;

    let mut found_any_options = false;
    for layer_opt in layers_high_to_low {
        let Some(layer) = layer_opt else { continue };
        if let Some(patch) = last_options_patch_in_layer(layer)? {
            found_any_options = true;
            if let Some(backend) = patch.backend {
                out.backend = backend;
            }
            if patch.model.is_some() {
                out.model = patch.model;
            }
            if patch.revision.is_some() {
                out.revision = patch.revision;
            }
            if patch.model_path.is_some() {
                out.model_path = patch.model_path;
            }
            if patch.model_sha256.is_some() {
                out.model_sha256 = patch.model_sha256;
            }
            if patch.dim.is_some() {
                out.dim = patch.dim;
            }
            if patch.api_base.is_some() {
                out.api_base = patch.api_base;
            }
            if patch.api_key_env.is_some() {
                out.api_key_env = patch.api_key_env;
            }
            if patch.cache_enabled.is_some() {
                out.cache_enabled = patch.cache_enabled.unwrap_or(false);
            }
            if patch.cache_dir.is_some() {
                out.cache_dir = patch.cache_dir;
            }
        }
    }

    // If no options were found in any layer, fall back to base layer's embedding metadata
    if !found_any_options && out.backend == "hash" {
        // Check the last layer (base) for embedding metadata
        if let Some(Some(base_layer)) = layers_high_to_low.last() {
            if let Some(metadata_bytes) = base_layer.layer_metadata_bytes() {
                if let Ok(metadata) = crate::layer_metadata::LayerMetadataV1::from_json_bytes(metadata_bytes) {
                    // Use the embedding profile from the base layer
                    out.backend = metadata.embedding_profile.backend;
                    out.model = metadata.embedding_profile.model;
                    out.revision = metadata.embedding_profile.revision;
                    out.dim = Some(metadata.embedding_profile.dim);
                }
            }
        }
    }

    Ok(out)
}

pub fn roll_up_embedding_options_from_paths(
    local: Option<&std::path::Path>,
    user: Option<&std::path::Path>,
    delta: Option<&std::path::Path>,
    base: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedEmbeddingOptions> {
    let local_file = open_if_exists(local).context("open local layer")?;
    let user_file = open_if_exists(user).context("open user layer")?;
    let delta_file = open_if_exists(delta).context("open delta layer")?;
    let base_file = open_if_exists(base).context("open base layer")?;

    roll_up_embedding_options(&[
        local_file.as_ref(),
        user_file.as_ref(),
        delta_file.as_ref(),
        base_file.as_ref(),
    ])
}

/// Get immutable embedding options from base layer only.
///
/// This ensures all operations use the same embedding configuration from AGENTS.db,
/// preventing inconsistencies when different operations would otherwise use different
/// embedding settings from higher-priority layers.
///
/// # Arguments
/// * `dir` - Directory containing the AGENTS.db file
///
/// # Returns
/// Resolved embedding options read only from AGENTS.db (base layer)
pub fn get_immutable_embedding_options(
    dir: &std::path::Path,
) -> anyhow::Result<ResolvedEmbeddingOptions> {
    let standard = standard_layer_paths_for_dir(dir);
    roll_up_embedding_options_from_paths(
        None,  // local - not read
        None,  // user - not read
        None,  // delta - not read
        Some(standard.base.as_path()),  // base only
    )
}

fn open_if_exists(
    path: Option<&std::path::Path>,
) -> anyhow::Result<Option<agentsdb_format::LayerFile>> {
    let Some(path) = path else { return Ok(None) };
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(
        agentsdb_format::LayerFile::open(path)
            .with_context(|| format!("open {}", path.display()))?,
    ))
}

fn last_options_patch_in_layer(
    layer: &agentsdb_format::LayerFile,
) -> anyhow::Result<Option<EmbeddingOptionsPatch>> {
    let mut last: Option<EmbeddingOptionsPatch> = None;
    for chunk in layer.chunks() {
        let chunk = chunk.context("read chunk")?;
        if chunk.kind != KIND_OPTIONS {
            continue;
        }
        let record: OptionsRecord =
            serde_json::from_str(chunk.content).context("parse options JSON")?;
        if let Some(embedding) = record.embedding {
            last = Some(embedding);
        }
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingEmbedder {
        profile: crate::embedder::EmbeddingProfile,
        calls: Arc<AtomicUsize>,
    }

    impl crate::embedder::Embedder for CountingEmbedder {
        fn profile(&self) -> &crate::embedder::EmbeddingProfile {
            &self.profile
        }

        fn metadata(&self) -> crate::embedder::EmbedderMetadata {
            crate::embedder::EmbedderMetadata {
                runtime: Some("counting".to_string()),
                ..Default::default()
            }
        }

        fn embed(&self, inputs: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(inputs
                .iter()
                .map(|_| vec![1.0f32; self.profile.dim])
                .collect())
        }
    }

    #[test]
    fn cached_embedder_hits_disk_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskEmbeddingCache::new(dir.path().to_path_buf()).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let inner = CountingEmbedder {
            profile: crate::embedder::EmbeddingProfile {
                backend: "hash".to_string(),
                model: None,
                revision: None,
                dim: 4,
                output_norm: crate::embedder::OutputNorm::None,
            },
            calls: calls.clone(),
        };
        let cached = CachedEmbedder {
            inner: Box::new(inner),
            cache,
        };

        let out1 = cached.embed(&["hello".to_string()]).unwrap();
        assert_eq!(out1, vec![vec![1.0; 4]]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let out2 = cached.embed(&["hello".to_string()]).unwrap();
        assert_eq!(out2, vec![vec![1.0; 4]]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn roll_up_allowlist_applies_ops_low_to_high() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("AGENTS.db");
        let local = dir.path().join("AGENTS.local.db");

        let schema = agentsdb_format::LayerSchema {
            dim: 4,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };

        let base_record = OptionsRecord {
            embedding: None,
            checksum_allowlist: Some(ChecksumAllowlistRecord {
                op: AllowlistOp::Add,
                entries: vec![ModelChecksumPin {
                    model: "all-minilm-l6-v2".to_string(),
                    revision: "main".to_string(),
                    sha256: Some(
                        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                            .to_string(),
                    ),
                }],
            }),
        };
        let base_chunk = agentsdb_format::ChunkInput {
            id: 1,
            kind: KIND_OPTIONS.to_string(),
            content: serde_json::to_string(&base_record).unwrap(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: vec![0.0; schema.dim as usize],
            sources: Vec::new(),
        };
        let mut chunks = [base_chunk];
        agentsdb_format::write_layer_atomic(&base, &schema, &mut chunks, None).unwrap();

        let local_record_remove = OptionsRecord {
            embedding: None,
            checksum_allowlist: Some(ChecksumAllowlistRecord {
                op: AllowlistOp::Remove,
                entries: vec![ModelChecksumPin {
                    model: "all-minilm-l6-v2".to_string(),
                    revision: "main".to_string(),
                    sha256: None,
                }],
            }),
        };
        let local_record_add = OptionsRecord {
            embedding: None,
            checksum_allowlist: Some(ChecksumAllowlistRecord {
                op: AllowlistOp::Add,
                entries: vec![ModelChecksumPin {
                    model: "all-minilm-l6-v2".to_string(),
                    revision: "pinned".to_string(),
                    sha256: Some(
                        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                            .to_string(),
                    ),
                }],
            }),
        };
        let mut chunks = [
            agentsdb_format::ChunkInput {
                id: 1,
                kind: KIND_OPTIONS.to_string(),
                content: serde_json::to_string(&local_record_remove).unwrap(),
                author: "human".to_string(),
                confidence: 1.0,
                created_at_unix_ms: 0,
                embedding: vec![0.0; schema.dim as usize],
                sources: Vec::new(),
            },
            agentsdb_format::ChunkInput {
                id: 2,
                kind: KIND_OPTIONS.to_string(),
                content: serde_json::to_string(&local_record_add).unwrap(),
                author: "human".to_string(),
                confidence: 1.0,
                created_at_unix_ms: 0,
                embedding: vec![0.0; schema.dim as usize],
                sources: Vec::new(),
            },
        ];
        agentsdb_format::write_layer_atomic(
            &local,
            &schema,
            &mut chunks,
            None,
        )
        .unwrap();

        let resolved = roll_up_embedding_options_from_paths(
            Some(local.as_path()),
            None,
            None,
            Some(base.as_path()),
        )
        .unwrap();
        assert!(!resolved.checksum_allowlist.contains_key(&ModelRevision {
            model: "all-minilm-l6-v2".to_string(),
            revision: "main".to_string()
        }));
        assert_eq!(
            resolved
                .checksum_allowlist
                .get(&ModelRevision {
                    model: "all-minilm-l6-v2".to_string(),
                    revision: "pinned".to_string()
                })
                .map(|v| v.as_str()),
            Some("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
        );
    }
}
