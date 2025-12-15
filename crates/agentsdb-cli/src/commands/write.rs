use anyhow::Context;
use serde::Serialize;

use agentsdb_embeddings::config::{
    roll_up_embedding_options_from_paths, standard_layer_paths_for_dir,
};
use agentsdb_embeddings::layer_metadata::LayerMetadataV1;

use crate::util::parse_vec_json;

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_write(
    path: &str,
    scope: &str,
    id: Option<u32>,
    kind: &str,
    content: &str,
    confidence: f32,
    embedding_json: Option<&str>,
    dim: Option<u32>,
    sources: &[String],
    source_chunks: &[u32],
    json: bool,
) -> anyhow::Result<()> {
    if scope != "local" && scope != "delta" {
        anyhow::bail!("--scope must be 'local' or 'delta'");
    }
    let expected_name = match scope {
        "local" => "AGENTS.local.db",
        "delta" => "AGENTS.delta.db",
        _ => unreachable!(),
    };
    if std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n != expected_name)
    {
        anyhow::bail!("scope {scope:?} expects file named {expected_name}");
    }

    agentsdb_format::ensure_writable_layer_path(path).context("permission check")?;

    let embedding = match embedding_json {
        Some(v) => parse_vec_json(v)?,
        None => Vec::new(),
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut chunk = agentsdb_format::ChunkInput {
        id: id.unwrap_or(0),
        kind: kind.to_string(),
        content: content.to_string(),
        author: "mcp".to_string(),
        confidence,
        created_at_unix_ms: now_ms,
        embedding: embedding.clone(),
        sources: sources
            .iter()
            .cloned()
            .map(agentsdb_format::ChunkSource::SourceString)
            .chain(
                source_chunks
                    .iter()
                    .copied()
                    .map(agentsdb_format::ChunkSource::ChunkId),
            )
            .collect(),
    };

    let p = std::path::Path::new(path);
    let dir = p.parent().unwrap_or_else(|| std::path::Path::new("."));
    let siblings = standard_layer_paths_for_dir(dir);
    let mut layer_metadata_json: Option<Vec<u8>> = None;
    let assigned = if p.exists() {
        if embedding.is_empty() {
            let file = agentsdb_format::LayerFile::open(path).context("open layer")?;
            let dim = file.embedding_dim();
            let options = roll_up_embedding_options_from_paths(
                Some(siblings.local.as_path()),
                Some(siblings.user.as_path()),
                Some(siblings.delta.as_path()),
                Some(siblings.base.as_path()),
            )
            .context("roll up options")?;
            if let Some(cfg_dim) = options.dim {
                if cfg_dim != dim {
                    anyhow::bail!(
                        "embedding dim mismatch (layer is dim={dim}, options specify dim={cfg_dim})"
                    );
                }
            }
            let embedder = options
                .into_embedder(dim)
                .context("resolve embedder from options")?;
            chunk.embedding = embedder
                .embed(&[chunk.content.clone()])?
                .into_iter()
                .next()
                .unwrap_or_else(|| vec![0.0; dim]);
            let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
                .with_embedder_metadata(embedder.metadata())
                .with_tool("agentsdb-cli", env!("CARGO_PKG_VERSION"));
            layer_metadata_json = Some(
                layer_metadata
                    .to_json_bytes()
                    .context("serialize layer metadata")?,
            );
        }
        let mut chunks = vec![chunk];
        let file = agentsdb_format::LayerFile::open(path).context("open layer")?;
        if let Some(existing) = file.layer_metadata_bytes() {
            let existing = LayerMetadataV1::from_json_bytes(existing)
                .context("parse existing layer metadata")?;
            if let Some(meta_json) = layer_metadata_json.as_ref() {
                let desired = LayerMetadataV1::from_json_bytes(meta_json)
                    .context("parse desired metadata")?;
                if existing.embedding_profile != desired.embedding_profile {
                    anyhow::bail!(
                        "embedder profile mismatch vs existing layer metadata (existing={:?}, current={:?})",
                        existing.embedding_profile,
                        desired.embedding_profile
                    );
                }
            }
            let ids =
                agentsdb_format::append_layer_atomic(path, &mut chunks, None).context("append")?;
            ids[0]
        } else {
            let ids = agentsdb_format::append_layer_atomic(
                path,
                &mut chunks,
                layer_metadata_json.as_deref(),
            )
            .context("append")?;
            ids[0]
        }
    } else {
        let dim = match (dim, embedding.is_empty()) {
            (Some(d), _) => d as usize,
            (None, true) => {
                anyhow::bail!("creating a new layer without --embedding requires --dim")
            }
            (None, false) => embedding.len(),
        };
        if chunk.embedding.is_empty() {
            let options = roll_up_embedding_options_from_paths(
                Some(siblings.local.as_path()),
                Some(siblings.user.as_path()),
                Some(siblings.delta.as_path()),
                Some(siblings.base.as_path()),
            )
            .context("roll up options")?;
            if let Some(cfg_dim) = options.dim {
                if cfg_dim != dim {
                    anyhow::bail!(
                        "embedding dim mismatch (creating layer with dim={dim}, options specify dim={cfg_dim})"
                    );
                }
            }
            let embedder = options
                .into_embedder(dim)
                .context("resolve embedder from options")?;
            chunk.embedding = embedder
                .embed(&[chunk.content.clone()])?
                .into_iter()
                .next()
                .unwrap_or_else(|| vec![0.0; dim]);
            let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
                .with_embedder_metadata(embedder.metadata())
                .with_tool("agentsdb-cli", env!("CARGO_PKG_VERSION"));
            layer_metadata_json = Some(
                layer_metadata
                    .to_json_bytes()
                    .context("serialize layer metadata")?,
            );
        }
        if chunk.id == 0 {
            chunk.id = 1;
        }
        let schema = agentsdb_format::LayerSchema {
            dim: dim as u32,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        agentsdb_format::write_layer_atomic(
            path,
            &schema,
            &[chunk],
            layer_metadata_json.as_deref(),
        )
        .context("create layer")?;
        id.unwrap_or(1)
    };

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            path: &'a str,
            id: u32,
        }
        let out = Out {
            ok: true,
            path,
            id: assigned,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Appended id={assigned} to {path}");
    }

    Ok(())
}
