use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

use crate::commands::compile::compile_to_layer;
use crate::types::{CompileChunk, CompileInput, CompileSchema, CompileSource};
use crate::util::{assign_stable_id, collect_files_wide_docs};

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_init(
    root: &str,
    out: &str,
    kind: &str,
    dim: u32,
    element_type: &str,
    quant_scale: Option<f32>,
    json: bool,
) -> anyhow::Result<()> {
    if dim == 0 {
        anyhow::bail!("--dim must be non-zero");
    }
    if element_type != "f32" && element_type != "i8" {
        anyhow::bail!("--element-type must be 'f32' or 'i8'");
    }

    let root_path = Path::new(root);
    let files = collect_files_wide_docs(root_path)?;

    let mut used_ids = BTreeSet::new();
    let mut chunks = Vec::with_capacity(files.len());
    for rel in files {
        let abs = root_path.join(&rel);
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

    let mut input = CompileInput {
        schema: CompileSchema {
            dim,
            element_type: element_type.to_string(),
            quant_scale: quant_scale.or_else(|| (element_type == "i8").then_some(1.0)),
        },
        chunks,
    };
    let chunks = compile_to_layer(&mut input, out).context("compile")?;

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            out: &'a str,
            chunks: usize,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                out,
                chunks,
            })?
        );
    } else {
        println!("Wrote {out} ({chunks} chunks)");
    }
    Ok(())
}
