use agentsdb_core::embed::hash_embed;
use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::types::{CompileChunk, CompileInput, CompileSchema, CompileSource};
use crate::util::{assign_stable_id, collect_files};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayerWriteAction {
    Created,
    Replaced,
    Appended,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_compile(
    input_json: Option<&str>,
    out: &str,
    replace: bool,
    root: &str,
    includes: &[String],
    paths: &[String],
    texts: &[String],
    kind: &str,
    dim: u32,
    element_type: &str,
    quant_scale: Option<f32>,
    json: bool,
) -> anyhow::Result<()> {
    let mut input = if let Some(input_json) = input_json {
        if !paths.is_empty() || !texts.is_empty() {
            anyhow::bail!("--in cannot be combined with PATHs or --text");
        }
        let s =
            std::fs::read_to_string(input_json).with_context(|| format!("read {}", input_json))?;
        serde_json::from_str(&s).context("parse compile input JSON")?
    } else {
        compile_input_from_sources(
            root,
            includes,
            paths,
            texts,
            kind,
            dim,
            element_type,
            quant_scale,
        )?
    };

    let (action, chunks) = compile_to_layer(&mut input, out, replace).context("compile")?;

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
        match action {
            LayerWriteAction::Appended => println!("Updated {out} (+{chunks} chunks)"),
            LayerWriteAction::Created | LayerWriteAction::Replaced => {
                println!("Wrote {out} ({chunks} chunks)")
            }
        }
    }
    Ok(())
}

fn compile_input_from_sources(
    root: &str,
    includes: &[String],
    paths: &[String],
    texts: &[String],
    kind: &str,
    dim: u32,
    element_type: &str,
    quant_scale: Option<f32>,
) -> anyhow::Result<CompileInput> {
    if dim == 0 {
        anyhow::bail!("--dim must be non-zero");
    }
    if element_type != "f32" && element_type != "i8" {
        anyhow::bail!("--element-type must be 'f32' or 'i8'");
    }

    let schema = CompileSchema {
        dim,
        element_type: element_type.to_string(),
        quant_scale: quant_scale.or_else(|| (element_type == "i8").then_some(1.0)),
    };

    let cwd = std::env::current_dir().ok();
    let mut used_ids = BTreeSet::new();
    let mut chunks = Vec::new();

    for (i, content) in texts.iter().enumerate() {
        let label = format!("inline:{}", i + 1);
        let label_path = Path::new(&label);
        let id = assign_stable_id(label_path, content, &mut used_ids);
        chunks.push(CompileChunk {
            id,
            kind: kind.to_string(),
            content: content.clone(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: None,
            sources: vec![CompileSource::String(format!("{label}:1"))],
        });
    }

    let file_paths: Vec<(PathBuf, PathBuf)> = if paths.is_empty() {
        let root_path = Path::new(root);
        collect_files(root_path, includes)?
            .into_iter()
            .map(|rel| (root_path.join(&rel), rel))
            .collect()
    } else {
        paths
            .iter()
            .map(|p| {
                let p = PathBuf::from(p);
                let abs = match (&cwd, p.is_absolute()) {
                    (_, true) => p.clone(),
                    (Some(cwd), false) => cwd.join(&p),
                    (None, false) => p.clone(),
                };
                let rel = match (&cwd, p.is_absolute()) {
                    (Some(cwd), true) => p.strip_prefix(cwd).unwrap_or(&p).to_path_buf(),
                    _ => p,
                };
                (abs, rel)
            })
            .collect()
    };

    for (abs, rel) in file_paths {
        let bytes = std::fs::read(&abs).with_context(|| format!("read bytes {}", abs.display()))?;
        let content = String::from_utf8_lossy(&bytes).to_string();
        let id = assign_stable_id(&rel, &content, &mut used_ids);
        chunks.push(CompileChunk {
            id,
            kind: kind.to_string(),
            content,
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: None,
            sources: vec![CompileSource::String(format!("{}:1", rel.display()))],
        });
    }

    if chunks.is_empty() {
        anyhow::bail!(
            "no chunks to compile (provide PATHs and/or --text, or use --root/--include)"
        );
    }

    Ok(CompileInput { schema, chunks })
}

pub(crate) fn compile_to_layer(
    input: &mut CompileInput,
    out: &str,
    replace: bool,
) -> anyhow::Result<(LayerWriteAction, usize)> {
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
    let mut chunks: Vec<agentsdb_format::ChunkInput> = input
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

    let out_path = Path::new(out);
    let existed = out_path.exists();
    let action = if !replace && existed {
        let file = agentsdb_format::LayerFile::open(out_path)
            .with_context(|| format!("open existing layer {}", out_path.display()))?;
        let existing_schema = agentsdb_format::schema_of(&file);
        if existing_schema.dim != schema.dim
            || existing_schema.element_type != schema.element_type
            || (existing_schema.quant_scale - schema.quant_scale).abs() > f32::EPSILON
        {
            anyhow::bail!(
                "output layer schema mismatch (existing: dim={}, element_type={:?}, quant_scale={}; new: dim={}, element_type={:?}, quant_scale={}); use --replace or choose a new --out path",
                existing_schema.dim,
                existing_schema.element_type,
                existing_schema.quant_scale,
                schema.dim,
                schema.element_type,
                schema.quant_scale
            );
        }

        agentsdb_format::append_layer_atomic(out_path, &mut chunks).context("append layer")?;
        LayerWriteAction::Appended
    } else {
        agentsdb_format::write_layer_atomic(out_path, &schema, &chunks).context("write layer")?;
        if existed && replace {
            LayerWriteAction::Replaced
        } else {
            LayerWriteAction::Created
        }
    };

    Ok((action, chunks.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_appends_when_out_exists() {
        let dir = crate::util::make_temp_dir();
        let out = dir.join("AGENTS.db");

        let mut input1 = CompileInput {
            schema: CompileSchema {
                dim: 8,
                element_type: "f32".to_string(),
                quant_scale: None,
            },
            chunks: vec![CompileChunk {
                id: 1,
                kind: "canonical".to_string(),
                content: "first".to_string(),
                author: "human".to_string(),
                confidence: 1.0,
                created_at_unix_ms: 0,
                embedding: None,
                sources: vec![],
            }],
        };
        let (action1, chunks1) =
            compile_to_layer(&mut input1, out.to_str().unwrap(), false).expect("initial compile");
        assert_eq!(action1, LayerWriteAction::Created);
        assert_eq!(chunks1, 1);

        let mut input2 = CompileInput {
            schema: CompileSchema {
                dim: 8,
                element_type: "f32".to_string(),
                quant_scale: None,
            },
            chunks: vec![CompileChunk {
                id: 2,
                kind: "canonical".to_string(),
                content: "second".to_string(),
                author: "human".to_string(),
                confidence: 1.0,
                created_at_unix_ms: 0,
                embedding: None,
                sources: vec![],
            }],
        };
        let (action2, chunks2) =
            compile_to_layer(&mut input2, out.to_str().unwrap(), false).expect("append compile");
        assert_eq!(action2, LayerWriteAction::Appended);
        assert_eq!(chunks2, 1);

        let file = agentsdb_format::LayerFile::open(&out).expect("open output");
        let all = agentsdb_format::read_all_chunks(&file).expect("read chunks");
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|c| c.content == "first"));
        assert!(all.iter().any(|c| c.content == "second"));
    }
}
