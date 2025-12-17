use anyhow::Context;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use agentsdb_core::export::{
    ExportBundleV1, ExportChunkV1, ExportLayerSchemaV1, ExportLayerV1, ExportNdjsonRecordV1,
    ExportSourceV1, ExportToolInfo,
};
use agentsdb_embeddings::config::{
    roll_up_embedding_options_from_paths, standard_layer_paths_for_dir,
};
use agentsdb_embeddings::layer_metadata::LayerMetadataV1;

use crate::util::content_sha256_hex;

#[derive(Debug, Clone, Serialize)]
pub struct ImportOutcome {
    pub imported: usize,
    pub skipped: usize,
    pub dry_run: bool,
}

/// Parse an export file into a structured bundle (supports both JSON and NDJSON formats).
pub fn parse_export_bytes(input: &[u8]) -> anyhow::Result<ExportBundleV1> {
    let s = std::str::from_utf8(input).context("input must be valid UTF-8")?;
    let trimmed = s.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(bundle) = serde_json::from_str::<ExportBundleV1>(trimmed) {
            return Ok(bundle);
        }
    }

    // NDJSON
    let mut tool = ExportToolInfo {
        name: "unknown".to_string(),
        version: "unknown".to_string(),
    };
    let mut layers: Vec<ExportLayerV1> = Vec::new();
    let mut layer_ix_by_path: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (i, line) in s.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec: ExportNdjsonRecordV1 =
            serde_json::from_str(line).with_context(|| format!("parse NDJSON line {}", i + 1))?;
        match rec {
            ExportNdjsonRecordV1::Header { tool: t, .. } => tool = t,
            ExportNdjsonRecordV1::Layer {
                path,
                layer,
                schema,
                layer_metadata_json,
            } => {
                let ix = if let Some(ix) = layer_ix_by_path.get(&path).copied() {
                    ix
                } else {
                    let ix = layers.len();
                    layers.push(ExportLayerV1 {
                        path: path.clone(),
                        layer: None,
                        schema: ExportLayerSchemaV1 {
                            dim: 0,
                            element_type: "f32".to_string(),
                            quant_scale: 1.0,
                        },
                        layer_metadata_json: None,
                        chunks: Vec::new(),
                    });
                    layer_ix_by_path.insert(path.clone(), ix);
                    ix
                };
                let l = layers.get_mut(ix).expect("layer index");
                l.layer = layer;
                l.schema = schema;
                l.layer_metadata_json = layer_metadata_json;
            }
            ExportNdjsonRecordV1::Chunk { layer_path, chunk } => {
                let ix = if let Some(ix) = layer_ix_by_path.get(&layer_path).copied() {
                    ix
                } else {
                    let ix = layers.len();
                    layers.push(ExportLayerV1 {
                        path: layer_path.clone(),
                        layer: None,
                        schema: ExportLayerSchemaV1 {
                            dim: 0,
                            element_type: "f32".to_string(),
                            quant_scale: 1.0,
                        },
                        layer_metadata_json: None,
                        chunks: Vec::new(),
                    });
                    layer_ix_by_path.insert(layer_path.clone(), ix);
                    ix
                };
                let l = layers.get_mut(ix).expect("layer index");
                l.chunks.push(chunk);
            }
        }
    }

    Ok(ExportBundleV1 {
        format: "agentsdb.export.ndjson.v1".to_string(),
        tool,
        layers,
    })
}

/// Parse import data from bytes (supports both JSON and NDJSON formats)
fn parse_input_bytes(input: &[u8]) -> anyhow::Result<Vec<ExportChunkV1>> {
    let bundle = parse_export_bytes(input)?;
    let mut out = Vec::new();
    for l in bundle.layers {
        out.extend(l.chunks);
    }
    Ok(out)
}

/// Parse import data from string (supports both JSON and NDJSON formats)
fn parse_input_string(input: &str) -> anyhow::Result<Vec<ExportChunkV1>> {
    parse_input_bytes(input.as_bytes())
}

fn sources_to_chunk_sources(sources: Vec<ExportSourceV1>) -> Vec<agentsdb_format::ChunkSource> {
    sources
        .into_iter()
        .map(|s| match s {
            ExportSourceV1::ChunkId { id } => agentsdb_format::ChunkSource::ChunkId(id),
            ExportSourceV1::SourceString { value } => {
                agentsdb_format::ChunkSource::SourceString(value)
            }
        })
        .collect()
}

fn ensure_target_permissions(path: &Path, scope: &str, allow_base: bool) -> anyhow::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    match scope {
        "local" => {
            if file_name != "AGENTS.local.db" {
                anyhow::bail!("scope local expects file named AGENTS.local.db");
            }
            agentsdb_format::ensure_writable_layer_path(path).context("permission check")?;
        }
        "delta" => {
            if file_name != "AGENTS.delta.db" {
                anyhow::bail!("scope delta expects file named AGENTS.delta.db");
            }
            agentsdb_format::ensure_writable_layer_path(path).context("permission check")?;
        }
        "user" => {
            if file_name != "AGENTS.user.db" {
                anyhow::bail!("scope user expects file named AGENTS.user.db");
            }
            agentsdb_format::ensure_writable_layer_path_allow_user(path)
                .context("permission check")?;
        }
        "base" => {
            if !allow_base {
                anyhow::bail!("refusing to write AGENTS.db without allow_base");
            }
            if file_name != "AGENTS.db" {
                anyhow::bail!("scope base expects file named AGENTS.db");
            }
            agentsdb_format::ensure_writable_layer_path_allow_base(path)
                .context("permission check")?;
        }
        _ => anyhow::bail!("scope must be local, delta, user, or base"),
    }
    Ok(())
}

/// Import chunks into a layer from exported data
///
/// # Arguments
/// * `abs_path` - Absolute path to the target layer file
/// * `scope` - Scope: "local", "delta", "user", or "base"
/// * `data` - Import data as string (JSON or NDJSON format)
/// * `dry_run` - If true, validate but don't write
/// * `dedupe` - If true, skip chunks with duplicate content hashes
/// * `preserve_ids` - If true, preserve chunk IDs from import data
/// * `allow_base` - If true, allow writing to AGENTS.db
/// * `dim` - Embedding dimension (required if creating new layer without embeddings in data)
/// * `tool_name` - Name of the tool performing the import
/// * `tool_version` - Version of the tool
///
/// # Returns
/// An ImportOutcome with counts of imported and skipped chunks
#[allow(clippy::too_many_arguments)]
pub fn import_into_layer(
    abs_path: &Path,
    scope: &str,
    data: &str,
    dry_run: bool,
    dedupe: bool,
    preserve_ids: bool,
    allow_base: bool,
    dim: Option<u32>,
    tool_name: &str,
    tool_version: &str,
) -> anyhow::Result<ImportOutcome> {
    ensure_target_permissions(abs_path, scope, allow_base)?;

    let mut imported = parse_input_string(data).context("parse import data")?;
    if imported.is_empty() {
        anyhow::bail!("no chunks found in import");
    }

    // Validate required fields and normalize hashes
    for c in &mut imported {
        if c.content.is_none() {
            anyhow::bail!("import contains redacted/missing content; cannot import");
        }
        let h = content_sha256_hex(c.content.as_deref().unwrap_or_default());
        c.content_sha256 = Some(h);
    }

    let dir = abs_path.parent().unwrap_or_else(|| Path::new("."));
    let siblings = standard_layer_paths_for_dir(dir);

    let mut existing_hashes: HashSet<String> = HashSet::new();
    let mut existing_ids: HashSet<u32> = HashSet::new();
    let (exists, dim_usize, existing_meta) = if abs_path.exists() {
        let file = agentsdb_format::LayerFile::open(abs_path).context("open target layer")?;
        let chunks = agentsdb_format::read_all_chunks(&file).context("read target chunks")?;
        if dedupe {
            for c in &chunks {
                existing_hashes.insert(content_sha256_hex(&c.content));
            }
        }
        for c in &chunks {
            existing_ids.insert(c.id);
        }
        (
            true,
            file.embedding_dim(),
            file.layer_metadata_bytes().map(|b| b.to_vec()),
        )
    } else {
        (false, 0usize, None)
    };

    let inferred_dim = if exists {
        dim_usize
    } else if let Some(d) = dim {
        d as usize
    } else {
        let mut inferred: Option<usize> = None;
        for c in &imported {
            if let Some(emb) = c.embedding.as_ref() {
                inferred = Some(emb.len());
                break;
            }
        }
        inferred.context("creating a new layer requires dim or input embeddings")?
    };

    let embedder_for_dim = |dim_usize: usize| -> anyhow::Result<
        Box<dyn agentsdb_embeddings::embedder::Embedder + Send + Sync>,
    > {
        let options = roll_up_embedding_options_from_paths(
            Some(siblings.local.as_path()),
            Some(siblings.user.as_path()),
            Some(siblings.delta.as_path()),
            Some(siblings.base.as_path()),
        )
        .context("roll up options")?;
        if let Some(cfg_dim) = options.dim {
            if cfg_dim != dim_usize {
                anyhow::bail!(
                    "embedding dim mismatch (target dim={dim_usize}, options specify dim={cfg_dim})"
                );
            }
        }
        options
            .into_embedder(dim_usize)
            .context("resolve embedder from options")
    };

    let mut layer_metadata_json: Option<Vec<u8>> = None;
    let mut embedder: Option<Box<dyn agentsdb_embeddings::embedder::Embedder + Send + Sync>> = None;

    let mut prepared: Vec<agentsdb_format::ChunkInput> = Vec::new();
    let mut skipped = 0usize;
    let mut next_new_id = 1u32;

    if !exists && preserve_ids {
        for c in &imported {
            let id = c.id;
            if id == 0 {
                anyhow::bail!("preserve_ids requires non-zero ids in input");
            }
            if existing_ids.contains(&id) {
                anyhow::bail!("id {id} already exists in target");
            }
            existing_ids.insert(id);
        }
    }

    for c in imported {
        let content = c.content.as_ref().expect("validated");
        let hash = c.content_sha256.as_deref().unwrap_or_default();
        if dedupe && existing_hashes.contains(hash) {
            skipped += 1;
            continue;
        }
        if dedupe {
            existing_hashes.insert(hash.to_string());
        }

        // Check if existing embedding has correct dimension
        let needs_reembedding = match c.embedding.as_ref() {
            Some(v) => v.len() != inferred_dim,
            None => true,
        };

        let embedding = if needs_reembedding {
            // Re-embed if dimension mismatch or no embedding
            if embedder.is_none() {
                let e = embedder_for_dim(inferred_dim)?;
                let meta = LayerMetadataV1::new(e.profile().clone())
                    .with_embedder_metadata(e.metadata())
                    .with_tool(tool_name, tool_version);
                layer_metadata_json =
                    Some(meta.to_json_bytes().context("serialize layer metadata")?);
                embedder = Some(e);
            }
            let e = embedder.as_ref().expect("embedder");
            e.embed(&[content.clone()])?
                .into_iter()
                .next()
                .unwrap_or_else(|| vec![0.0; inferred_dim])
        } else {
            // Use existing embedding if dimension matches
            c.embedding.unwrap()
        };

        let id = if exists {
            if preserve_ids {
                if existing_ids.contains(&c.id) {
                    anyhow::bail!("id {} already exists in target", c.id);
                }
                existing_ids.insert(c.id);
                c.id
            } else {
                0
            }
        } else if preserve_ids {
            c.id
        } else {
            while existing_ids.contains(&next_new_id) {
                next_new_id = next_new_id.saturating_add(1);
            }
            existing_ids.insert(next_new_id);
            let assigned = next_new_id;
            next_new_id = next_new_id.saturating_add(1);
            assigned
        };

        prepared.push(agentsdb_format::ChunkInput {
            id,
            kind: c.kind,
            content: content.clone(),
            author: c.author,
            confidence: c.confidence,
            created_at_unix_ms: c.created_at_unix_ms,
            embedding,
            sources: sources_to_chunk_sources(c.sources),
        });
    }

    if prepared.is_empty() {
        return Ok(ImportOutcome {
            imported: 0,
            skipped,
            dry_run,
        });
    }

    // If computing embeddings into an existing layer, enforce embedder profile compatibility
    if let (Some(existing_meta), Some(layer_metadata_json)) =
        (existing_meta.as_ref(), layer_metadata_json.as_ref())
    {
        let existing = LayerMetadataV1::from_json_bytes(existing_meta)
            .context("parse existing layer metadata")?;
        let desired = LayerMetadataV1::from_json_bytes(layer_metadata_json)
            .context("parse desired layer metadata")?;
        if existing.embedding_profile != desired.embedding_profile {
            anyhow::bail!(
                "embedder profile mismatch vs target layer metadata (existing={:?}, current={:?})",
                existing.embedding_profile,
                desired.embedding_profile
            );
        }
    }

    let prepared_len = prepared.len();

    if dry_run {
        return Ok(ImportOutcome {
            imported: prepared_len,
            skipped,
            dry_run: true,
        });
    }

    if exists {
        let mut new_chunks = prepared;
        agentsdb_format::append_layer_atomic(
            abs_path,
            &mut new_chunks,
            layer_metadata_json.as_deref(),
        )
        .context("append")?;
    } else {
        let schema = agentsdb_format::LayerSchema {
            dim: inferred_dim as u32,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        agentsdb_format::write_layer_atomic(
            abs_path,
            &schema,
            &prepared,
            layer_metadata_json.as_deref(),
        )
        .context("create layer")?;
    }

    Ok(ImportOutcome {
        imported: prepared_len,
        skipped,
        dry_run: false,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn import_export_bundle_into_dir(
    dir: &Path,
    data: &[u8],
    dry_run: bool,
    dedupe: bool,
    preserve_ids: bool,
    allow_base: bool,
    dim: Option<u32>,
    tool_name: &str,
    tool_version: &str,
) -> anyhow::Result<Vec<(String, ImportOutcome)>> {
    let bundle = parse_export_bytes(data).context("parse export")?;
    let mut out = Vec::new();

    for layer in bundle.layers {
        let rel = std::path::Path::new(&layer.path);
        if rel.components().count() != 1 {
            anyhow::bail!(
                "export layer path {:?} must be a simple file name (no directories)",
                layer.path
            );
        }
        let file_name = rel
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();

        let scope = crate::util::logical_layer_for_path(file_name).with_context(|| {
            format!(
                "unsupported export layer path {:?} (expected AGENTS.db / AGENTS.user.db / AGENTS.delta.db / AGENTS.local.db)",
                layer.path
            )
        })?;

        if scope == "base" && !allow_base {
            anyhow::bail!(
                "export includes AGENTS.db; pass --allow-base to import it, or export without base"
            );
        }

        let abs_path = dir.join(file_name);

        let single = ExportBundleV1 {
            format: "agentsdb.export.v1".to_string(),
            tool: ExportToolInfo {
                name: tool_name.to_string(),
                version: tool_version.to_string(),
            },
            layers: vec![layer],
        };
        let data = serde_json::to_string(&single).context("serialize layer bundle")?;

        let outcome = import_into_layer(
            &abs_path,
            scope,
            &data,
            dry_run,
            dedupe,
            preserve_ids,
            allow_base,
            dim,
            tool_name,
            tool_version,
        )?;

        out.push((abs_path.to_string_lossy().to_string(), outcome));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_chunk(id: u32, content: &str) -> ExportChunkV1 {
        ExportChunkV1 {
            id,
            kind: "test".to_string(),
            content: Some(content.to_string()),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 1,
            sources: Vec::new(),
            embedding: Some(vec![0.0, 0.0, 0.0, 0.0]),
            content_sha256: None,
        }
    }

    #[test]
    fn parse_export_bytes_json_preserves_layers() {
        let bundle = ExportBundleV1 {
            format: "agentsdb.export.v1".to_string(),
            tool: ExportToolInfo {
                name: "test".to_string(),
                version: "0".to_string(),
            },
            layers: vec![
                ExportLayerV1 {
                    path: "AGENTS.delta.db".to_string(),
                    layer: Some("delta".to_string()),
                    schema: ExportLayerSchemaV1 {
                        dim: 4,
                        element_type: "f32".to_string(),
                        quant_scale: 1.0,
                    },
                    layer_metadata_json: None,
                    chunks: vec![minimal_chunk(1, "a")],
                },
                ExportLayerV1 {
                    path: "AGENTS.local.db".to_string(),
                    layer: Some("local".to_string()),
                    schema: ExportLayerSchemaV1 {
                        dim: 4,
                        element_type: "f32".to_string(),
                        quant_scale: 1.0,
                    },
                    layer_metadata_json: None,
                    chunks: vec![minimal_chunk(2, "b")],
                },
            ],
        };

        let bytes = serde_json::to_vec(&bundle).unwrap();
        let parsed = parse_export_bytes(&bytes).unwrap();
        assert_eq!(parsed.layers.len(), 2);
        assert_eq!(parsed.layers[0].path, "AGENTS.delta.db");
        assert_eq!(parsed.layers[1].path, "AGENTS.local.db");
        assert_eq!(parsed.layers[0].chunks.len(), 1);
        assert_eq!(parsed.layers[1].chunks.len(), 1);
    }

    #[test]
    fn parse_export_bytes_ndjson_groups_chunks_by_layer() {
        let header = ExportNdjsonRecordV1::Header {
            format: "agentsdb.export.ndjson.v1".to_string(),
            tool: ExportToolInfo {
                name: "test".to_string(),
                version: "0".to_string(),
            },
        };
        let layer_delta = ExportNdjsonRecordV1::Layer {
            path: "AGENTS.delta.db".to_string(),
            layer: Some("delta".to_string()),
            schema: ExportLayerSchemaV1 {
                dim: 4,
                element_type: "f32".to_string(),
                quant_scale: 1.0,
            },
            layer_metadata_json: None,
        };
        let layer_local = ExportNdjsonRecordV1::Layer {
            path: "AGENTS.local.db".to_string(),
            layer: Some("local".to_string()),
            schema: ExportLayerSchemaV1 {
                dim: 4,
                element_type: "f32".to_string(),
                quant_scale: 1.0,
            },
            layer_metadata_json: None,
        };
        let chunk_delta = ExportNdjsonRecordV1::Chunk {
            layer_path: "AGENTS.delta.db".to_string(),
            chunk: minimal_chunk(1, "a"),
        };
        let chunk_local = ExportNdjsonRecordV1::Chunk {
            layer_path: "AGENTS.local.db".to_string(),
            chunk: minimal_chunk(2, "b"),
        };

        let ndjson = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            serde_json::to_string(&header).unwrap(),
            serde_json::to_string(&layer_delta).unwrap(),
            serde_json::to_string(&layer_local).unwrap(),
            serde_json::to_string(&chunk_delta).unwrap(),
            serde_json::to_string(&chunk_local).unwrap(),
        );

        let parsed = parse_export_bytes(ndjson.as_bytes()).unwrap();
        assert_eq!(parsed.layers.len(), 2);

        let mut by_path: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for l in &parsed.layers {
            by_path.insert(l.path.clone(), l.chunks.len());
        }
        assert_eq!(by_path.get("AGENTS.delta.db").copied().unwrap_or_default(), 1);
        assert_eq!(by_path.get("AGENTS.local.db").copied().unwrap_or_default(), 1);
    }
}
