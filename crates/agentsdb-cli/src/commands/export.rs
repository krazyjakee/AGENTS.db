use anyhow::Context;

use agentsdb_core::export::{
    ExportBundleV1, ExportChunkV1, ExportLayerSchemaV1, ExportLayerV1, ExportSourceV1,
    ExportToolInfo,
};
use agentsdb_embeddings::config::standard_layer_paths_for_dir;

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = vec![0u8; bytes.len() * 2];
    for (i, b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    String::from_utf8(out).expect("valid hex")
}

fn content_sha256_hex(content: &str) -> String {
    let digest = agentsdb_embeddings::cache::sha256(content.as_bytes());
    hex_lower(&digest)
}

fn parse_layers_csv(s: &str) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    for raw in s.split(',') {
        let v = raw.trim();
        if v.is_empty() {
            continue;
        }
        match v {
            "base" | "user" | "delta" | "local" => out.push(v.to_string()),
            _ => anyhow::bail!("invalid layer {v:?} (expected base,user,delta,local)"),
        }
    }
    if out.is_empty() {
        anyhow::bail!("--layers must include at least one of base,user,delta,local");
    }
    Ok(out)
}

fn element_type_str(t: agentsdb_format::EmbeddingElementType) -> &'static str {
    match t {
        agentsdb_format::EmbeddingElementType::F32 => "f32",
        agentsdb_format::EmbeddingElementType::I8 => "i8",
    }
}

fn apply_redaction(
    redact: &str,
    content: &str,
    embedding: &[f32],
) -> (Option<String>, Option<Vec<f32>>) {
    match redact {
        "none" => (Some(content.to_string()), Some(embedding.to_vec())),
        "content" => (None, Some(embedding.to_vec())),
        "embeddings" => (Some(content.to_string()), None),
        "all" => (None, None),
        _ => (Some(content.to_string()), Some(embedding.to_vec())),
    }
}

pub(crate) fn cmd_export(
    dir: &str,
    format: &str,
    layers_csv: &str,
    out_path: Option<&str>,
    redact: &str,
    json: bool,
) -> anyhow::Result<()> {
    if json {
        anyhow::bail!("--json is not supported for export (export output is already JSON/NDJSON)");
    }

    let layers = parse_layers_csv(layers_csv)?;
    let siblings = standard_layer_paths_for_dir(std::path::Path::new(dir));
    let mut export_layers: Vec<ExportLayerV1> = Vec::new();

    for layer in layers {
        let (path, logical) = match layer.as_str() {
            "base" => (siblings.base.clone(), Some("base".to_string())),
            "user" => (siblings.user.clone(), Some("user".to_string())),
            "delta" => (siblings.delta.clone(), Some("delta".to_string())),
            "local" => (siblings.local.clone(), Some("local".to_string())),
            _ => continue,
        };
        if !path.exists() {
            continue;
        }

        let file = agentsdb_format::LayerFile::open(&path)
            .with_context(|| format!("open {}", path.display()))?;
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
            path: path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string()),
            layer: logical,
            schema,
            layer_metadata_json,
            chunks: out_chunks,
        });
    }

    let bundle = ExportBundleV1 {
        format: "agentsdb.export.v1".to_string(),
        tool: ExportToolInfo {
            name: "agentsdb-cli".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        layers: export_layers,
    };

    let mut out: Box<dyn std::io::Write> = match out_path {
        Some(p) => Box::new(std::fs::File::create(p).with_context(|| format!("create {}", p))?),
        None => Box::new(std::io::stdout()),
    };

    match format {
        "json" => {
            let bytes = serde_json::to_vec_pretty(&bundle).context("serialize JSON")?;
            out.write_all(&bytes)?;
            out.write_all(b"\n")?;
        }
        "ndjson" => {
            use agentsdb_core::export::ExportNdjsonRecordV1;
            let header = ExportNdjsonRecordV1::Header {
                format: "agentsdb.export.ndjson.v1".to_string(),
                tool: bundle.tool.clone(),
            };
            writeln!(out, "{}", serde_json::to_string(&header)?)?;
            for l in &bundle.layers {
                let rec = ExportNdjsonRecordV1::Layer {
                    path: l.path.clone(),
                    layer: l.layer.clone(),
                    schema: l.schema.clone(),
                    layer_metadata_json: l.layer_metadata_json.clone(),
                };
                writeln!(out, "{}", serde_json::to_string(&rec)?)?;
                for c in &l.chunks {
                    let rec = ExportNdjsonRecordV1::Chunk {
                        layer_path: l.path.clone(),
                        chunk: c.clone(),
                    };
                    writeln!(out, "{}", serde_json::to_string(&rec)?)?;
                }
            }
        }
        _ => anyhow::bail!("--format must be json or ndjson"),
    }

    Ok(())
}
