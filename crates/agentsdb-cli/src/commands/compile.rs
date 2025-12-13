use agentsdb_core::embed::hash_embed;
use anyhow::Context;
use serde::Serialize;

use crate::types::{CompileInput, CompileSource};

pub(crate) fn cmd_compile(input: &str, out: &str, json: bool) -> anyhow::Result<()> {
    let s = std::fs::read_to_string(input).with_context(|| format!("read {input}"))?;
    let mut input: CompileInput = serde_json::from_str(&s).context("parse compile input JSON")?;
    let chunks = compile_to_layer(&mut input, out).context("compile")?;

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            out: &'a str,
            chunks: usize,
        }
        let out = Out {
            ok: true,
            out,
            chunks,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("Wrote {out} ({chunks} chunks)");
    }
    Ok(())
}

pub(crate) fn compile_to_layer(input: &mut CompileInput, out: &str) -> anyhow::Result<usize> {
    if input.schema.dim == 0 {
        anyhow::bail!("schema.dim must be non-zero");
    }

    let element_type = match input.schema.element_type.as_str() {
        "f32" => agentsdb_format::EmbeddingElementType::F32,
        "i8" => agentsdb_format::EmbeddingElementType::I8,
        other => anyhow::bail!("schema.element_type must be 'f32' or 'i8' (got {other:?})"),
    };
    let quant_scale = match element_type {
        agentsdb_format::EmbeddingElementType::F32 => 1.0,
        agentsdb_format::EmbeddingElementType::I8 => input.schema.quant_scale.unwrap_or(1.0),
    };
    let schema = agentsdb_format::LayerSchema {
        dim: input.schema.dim,
        element_type,
        quant_scale,
    };

    input.chunks.sort_by_key(|c| c.id);
    let dim = schema.dim as usize;
    let chunks: Vec<agentsdb_format::ChunkInput> = input
        .chunks
        .drain(..)
        .map(|c| {
            let content = c.content;
            let embedding = match c.embedding {
                Some(v) => v,
                None => hash_embed(&content, dim),
            };
            agentsdb_format::ChunkInput {
                id: c.id,
                kind: c.kind,
                content,
                author: c.author,
                confidence: c.confidence,
                created_at_unix_ms: c.created_at_unix_ms,
                embedding,
                sources: c
                    .sources
                    .into_iter()
                    .map(|s| match s {
                        CompileSource::String(v) => agentsdb_format::ChunkSource::SourceString(v),
                        CompileSource::Chunk { chunk_id } => {
                            agentsdb_format::ChunkSource::ChunkId(chunk_id)
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    agentsdb_format::write_layer_atomic(out, &schema, &chunks).context("write layer")?;
    Ok(chunks.len())
}
