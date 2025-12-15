use anyhow::Context;
use std::path::Path;

use agentsdb_core::export::{
    ExportBundleV1, ExportChunkV1, ExportLayerSchemaV1, ExportLayerV1, ExportNdjsonRecordV1,
    ExportSourceV1, ExportToolInfo,
};

use crate::util::{apply_redaction, content_sha256_hex, element_type_str, logical_layer_for_path};

/// Export a single layer to either JSON or NDJSON format
///
/// # Arguments
/// * `abs_path` - Absolute path to the layer file
/// * `rel_path` - Relative path/filename for display purposes
/// * `format` - Either "json" or "ndjson"
/// * `redact` - Redaction mode: "none", "content", "embeddings", or "all"
/// * `tool_name` - Name of the tool performing the export (e.g., "agentsdb-cli" or "agentsdb-web")
/// * `tool_version` - Version of the tool
///
/// # Returns
/// A tuple of (content_type, body_bytes)
pub fn export_layer(
    abs_path: &Path,
    rel_path: &str,
    format: &str,
    redact: &str,
    tool_name: &str,
    tool_version: &str,
) -> anyhow::Result<(&'static str, Vec<u8>)> {
    let file = agentsdb_format::LayerFile::open(abs_path)
        .with_context(|| format!("open {}", abs_path.display()))?;
    let layer_schema = agentsdb_format::schema_of(&file);
    let schema = ExportLayerSchemaV1 {
        dim: layer_schema.dim,
        element_type: element_type_str(layer_schema.element_type).to_string(),
        quant_scale: layer_schema.quant_scale,
    };
    let layer_metadata_json = file
        .layer_metadata_bytes()
        .map(|b| String::from_utf8_lossy(b).to_string());

    let chunks = agentsdb_format::read_all_chunks(&file).context("read chunks")?;
    let mut out_chunks = Vec::with_capacity(chunks.len());
    for c in chunks {
        let (content, embedding) = apply_redaction(redact, &c.content, &c.embedding);
        let sources = c
            .sources
            .into_iter()
            .map(|s| match s {
                agentsdb_format::ChunkSource::ChunkId(id) => ExportSourceV1::ChunkId { id },
                agentsdb_format::ChunkSource::SourceString(v) => {
                    ExportSourceV1::SourceString { value: v }
                }
            })
            .collect();
        let content_sha256 = content.as_deref().map(content_sha256_hex);
        out_chunks.push(ExportChunkV1 {
            id: c.id,
            kind: c.kind,
            content,
            author: c.author,
            confidence: c.confidence,
            created_at_unix_ms: c.created_at_unix_ms,
            sources,
            embedding,
            content_sha256,
        });
    }

    match format {
        "json" => {
            let bundle = ExportBundleV1 {
                format: "agentsdb.export.v1".to_string(),
                tool: ExportToolInfo {
                    name: tool_name.to_string(),
                    version: tool_version.to_string(),
                },
                layers: vec![ExportLayerV1 {
                    path: rel_path.to_string(),
                    layer: logical_layer_for_path(rel_path).map(|s| s.to_string()),
                    schema,
                    layer_metadata_json,
                    chunks: out_chunks,
                }],
            };
            Ok((
                "application/json",
                serde_json::to_vec_pretty(&bundle).context("serialize JSON")?,
            ))
        }
        "ndjson" => {
            let mut out = Vec::new();
            let header = ExportNdjsonRecordV1::Header {
                format: "agentsdb.export.ndjson.v1".to_string(),
                tool: ExportToolInfo {
                    name: tool_name.to_string(),
                    version: tool_version.to_string(),
                },
            };
            out.extend_from_slice(serde_json::to_string(&header)?.as_bytes());
            out.push(b'\n');
            let layer_rec = ExportNdjsonRecordV1::Layer {
                path: rel_path.to_string(),
                layer: logical_layer_for_path(rel_path).map(|s| s.to_string()),
                schema,
                layer_metadata_json,
            };
            out.extend_from_slice(serde_json::to_string(&layer_rec)?.as_bytes());
            out.push(b'\n');
            for c in out_chunks {
                let rec = ExportNdjsonRecordV1::Chunk {
                    layer_path: rel_path.to_string(),
                    chunk: c,
                };
                out.extend_from_slice(serde_json::to_string(&rec)?.as_bytes());
                out.push(b'\n');
            }
            Ok(("application/x-ndjson", out))
        }
        _ => anyhow::bail!("format must be json or ndjson"),
    }
}

/// Export multiple layers to a single JSON or NDJSON bundle
///
/// # Arguments
/// * `layers_and_paths` - Vector of (abs_path, rel_path, logical_layer) tuples
/// * `format` - Either "json" or "ndjson"
/// * `redact` - Redaction mode: "none", "content", "embeddings", or "all"
/// * `tool_name` - Name of the tool performing the export
/// * `tool_version` - Version of the tool
///
/// # Returns
/// A tuple of (content_type, body_bytes)
pub fn export_layers(
    layers_and_paths: Vec<(&Path, &str, Option<&str>)>,
    format: &str,
    redact: &str,
    tool_name: &str,
    tool_version: &str,
) -> anyhow::Result<(&'static str, Vec<u8>)> {
    let mut export_layers = Vec::new();

    for (abs_path, rel_path, logical_layer) in layers_and_paths {
        if !abs_path.exists() {
            continue;
        }

        let file = agentsdb_format::LayerFile::open(abs_path)
            .with_context(|| format!("open {}", abs_path.display()))?;
        let layer_schema = agentsdb_format::schema_of(&file);
        let schema = ExportLayerSchemaV1 {
            dim: layer_schema.dim,
            element_type: element_type_str(layer_schema.element_type).to_string(),
            quant_scale: layer_schema.quant_scale,
        };
        let layer_metadata_json = file
            .layer_metadata_bytes()
            .map(|b| String::from_utf8_lossy(b).to_string());

        let chunks = agentsdb_format::read_all_chunks(&file).context("read chunks")?;
        let mut out_chunks = Vec::with_capacity(chunks.len());
        for c in chunks {
            let (content, embedding) = apply_redaction(redact, &c.content, &c.embedding);
            let sources = c
                .sources
                .into_iter()
                .map(|s| match s {
                    agentsdb_format::ChunkSource::ChunkId(id) => ExportSourceV1::ChunkId { id },
                    agentsdb_format::ChunkSource::SourceString(v) => {
                        ExportSourceV1::SourceString { value: v }
                    }
                })
                .collect();
            let content_sha256 = content.as_deref().map(content_sha256_hex);
            out_chunks.push(ExportChunkV1 {
                id: c.id,
                kind: c.kind,
                content,
                author: c.author,
                confidence: c.confidence,
                created_at_unix_ms: c.created_at_unix_ms,
                sources,
                embedding,
                content_sha256,
            });
        }

        export_layers.push(ExportLayerV1 {
            path: rel_path.to_string(),
            layer: logical_layer.map(|s| s.to_string()),
            schema,
            layer_metadata_json,
            chunks: out_chunks,
        });
    }

    let bundle = ExportBundleV1 {
        format: "agentsdb.export.v1".to_string(),
        tool: ExportToolInfo {
            name: tool_name.to_string(),
            version: tool_version.to_string(),
        },
        layers: export_layers,
    };

    match format {
        "json" => {
            let bytes = serde_json::to_vec_pretty(&bundle).context("serialize JSON")?;
            Ok(("application/json", bytes))
        }
        "ndjson" => {
            let mut out = Vec::new();
            let header = ExportNdjsonRecordV1::Header {
                format: "agentsdb.export.ndjson.v1".to_string(),
                tool: bundle.tool.clone(),
            };
            out.extend_from_slice(serde_json::to_string(&header)?.as_bytes());
            out.push(b'\n');
            for l in &bundle.layers {
                let rec = ExportNdjsonRecordV1::Layer {
                    path: l.path.clone(),
                    layer: l.layer.clone(),
                    schema: l.schema.clone(),
                    layer_metadata_json: l.layer_metadata_json.clone(),
                };
                out.extend_from_slice(serde_json::to_string(&rec)?.as_bytes());
                out.push(b'\n');
                for c in &l.chunks {
                    let rec = ExportNdjsonRecordV1::Chunk {
                        layer_path: l.path.clone(),
                        chunk: c.clone(),
                    };
                    out.extend_from_slice(serde_json::to_string(&rec)?.as_bytes());
                    out.push(b'\n');
                }
            }
            Ok(("application/x-ndjson", out))
        }
        _ => anyhow::bail!("format must be json or ndjson"),
    }
}
