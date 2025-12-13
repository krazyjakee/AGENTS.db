use agentsdb_core::embed::hash_embed;
use anyhow::Context;
use serde::Serialize;

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
    let assigned = if p.exists() {
        if embedding.is_empty() {
            let file = agentsdb_format::LayerFile::open(path).context("open layer")?;
            chunk.embedding = hash_embed(&chunk.content, file.embedding_dim());
        }
        let mut chunks = vec![chunk];
        let ids = agentsdb_format::append_layer_atomic(path, &mut chunks).context("append")?;
        ids[0]
    } else {
        let dim = match (dim, embedding.is_empty()) {
            (Some(d), _) => d as usize,
            (None, true) => {
                anyhow::bail!("creating a new layer without --embedding requires --dim")
            }
            (None, false) => embedding.len(),
        };
        if chunk.embedding.is_empty() {
            chunk.embedding = hash_embed(&chunk.content, dim);
        }
        if chunk.id == 0 {
            chunk.id = 1;
        }
        let schema = agentsdb_format::LayerSchema {
            dim: dim as u32,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        agentsdb_format::write_layer_atomic(path, &schema, &[chunk]).context("create layer")?;
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
