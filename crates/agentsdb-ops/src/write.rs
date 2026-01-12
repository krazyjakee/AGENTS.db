use anyhow::Context;
use std::path::Path;

use agentsdb_embeddings::config::get_immutable_embedding_options;
use agentsdb_embeddings::layer_metadata::LayerMetadataV1;
use agentsdb_format::{ChunkInput, ChunkSource, LayerFile};

use crate::util::now_unix_ms;

/// Append a chunk to a layer file (local or delta)
///
/// # Arguments
/// * `path` - Path to the layer file
/// * `scope` - Either "local" or "delta"
/// * `id` - Optional chunk ID (None = auto-assign)
/// * `kind` - Chunk kind (e.g., "note", "invariant")
/// * `content` - Chunk content
/// * `confidence` - Confidence score (0.0-1.0)
/// * `dim` - Embedding dimension (required only if creating a new layer)
/// * `sources` - Source strings (e.g., file:line references)
/// * `source_chunks` - Source chunk IDs
/// * `tool_name` - Name of the tool appending the chunk
/// * `tool_version` - Version of the tool
///
/// # Returns
/// The assigned chunk ID
#[allow(clippy::too_many_arguments)]
pub fn append_chunk(
    path: &Path,
    scope: &str,
    id: Option<u32>,
    kind: &str,
    content: &str,
    confidence: f32,
    dim: Option<u32>,
    sources: &[String],
    source_chunks: &[u32],
    tool_name: &str,
    tool_version: &str,
) -> anyhow::Result<u32> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !matches!(file_name, "AGENTS.local.db" | "AGENTS.delta.db") {
        anyhow::bail!("writes are only allowed for AGENTS.local.db / AGENTS.delta.db");
    }
    if scope == "local" && file_name != "AGENTS.local.db" {
        anyhow::bail!("scope local only allowed for AGENTS.local.db");
    }
    if scope == "delta" && file_name != "AGENTS.delta.db" {
        anyhow::bail!("scope delta only allowed for AGENTS.delta.db");
    }

    let exists = path.exists();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));

    let embedder_for_dim = |dim_usize: usize| -> anyhow::Result<
        Box<dyn agentsdb_embeddings::embedder::Embedder + Send + Sync>,
    > {
        let options = get_immutable_embedding_options(dir)
            .context("get immutable embedding options")?;
        if let Some(cfg_dim) = options.dim {
            if cfg_dim != dim_usize {
                anyhow::bail!(
                    "embedding dim mismatch (layer is dim={dim_usize}, options specify dim={cfg_dim})"
                );
            }
        }
        options
            .into_embedder(dim_usize)
            .context("resolve embedder from options")
    };

    if exists {
        let file =
            LayerFile::open(path).with_context(|| format!("open for append {}", path.display()))?;
        let dim_usize = file.embedding_dim();

        let mut chunk = ChunkInput {
            id: id.unwrap_or(0), // 0 = auto-assign
            kind: kind.to_string(),
            author: "human".to_string(),
            confidence,
            created_at_unix_ms: now_unix_ms(),
            content: content.to_string(),
            embedding: Vec::new(),
            sources: Vec::new(),
        };
        let embedder = embedder_for_dim(dim_usize)?;
        chunk.embedding = embedder
            .embed(&[chunk.content.clone()])?
            .into_iter()
            .next()
            .unwrap_or_else(|| vec![0.0; dim_usize]);
        let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
            .with_embedder_metadata(embedder.metadata())
            .with_tool(tool_name, tool_version);
        let layer_metadata_json = layer_metadata
            .to_json_bytes()
            .context("serialize layer metadata")?;

        for s in sources.iter() {
            chunk.sources.push(ChunkSource::SourceString(s.to_string()));
        }
        for cid in source_chunks.iter() {
            chunk.sources.push(ChunkSource::ChunkId(*cid));
        }

        let mut new_chunks = vec![chunk];
        let assigned = if let Some(existing) = file.layer_metadata_bytes() {
            let existing = LayerMetadataV1::from_json_bytes(existing)
                .context("parse existing layer metadata")?;
            if existing.embedding_profile != *embedder.profile() {
                anyhow::bail!(
                    "embedder profile mismatch vs existing layer metadata (existing={:?}, current={:?})",
                    existing.embedding_profile,
                    embedder.profile()
                );
            }
            agentsdb_format::append_layer_atomic(path, &mut new_chunks, None)
                .context("append chunk")?
        } else {
            agentsdb_format::append_layer_atomic(path, &mut new_chunks, Some(&layer_metadata_json))
                .context("append chunk")?
        };
        Ok(*assigned.first().unwrap_or(&0))
    } else {
        let dim = dim.context("creating a new layer requires dim")?;
        let assigned = id.unwrap_or(1);
        let mut chunk = ChunkInput {
            id: assigned,
            kind: kind.to_string(),
            author: "human".to_string(),
            confidence,
            created_at_unix_ms: now_unix_ms(),
            content: content.to_string(),
            embedding: Vec::new(),
            sources: Vec::new(),
        };
        let dim_usize = dim as usize;
        let embedder = embedder_for_dim(dim_usize)?;
        chunk.embedding = embedder
            .embed(&[chunk.content.clone()])?
            .into_iter()
            .next()
            .unwrap_or_else(|| vec![0.0; dim_usize]);
        let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
            .with_embedder_metadata(embedder.metadata())
            .with_tool(tool_name, tool_version);
        let layer_metadata_json = layer_metadata
            .to_json_bytes()
            .context("serialize layer metadata")?;

        for s in sources.iter() {
            chunk.sources.push(ChunkSource::SourceString(s.to_string()));
        }
        for cid in source_chunks.iter() {
            chunk.sources.push(ChunkSource::ChunkId(*cid));
        }

        if chunk.id == 0 {
            chunk.id = 1;
        }
        let schema = agentsdb_format::LayerSchema {
            dim,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        let mut chunks = [chunk];
        agentsdb_format::write_layer_atomic(path, &schema, &mut chunks, Some(&layer_metadata_json))
            .context("create layer")?;
        Ok(assigned)
    }
}
