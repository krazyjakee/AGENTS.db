use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

use crate::types::{CollectChunk, CollectOutput, CollectSource, CompileSchemaOut};
use crate::util::{assign_stable_id, collect_files};

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_collect(
    root: &str,
    includes: &[String],
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
    let files = collect_files(root_path, includes)?;

    let mut used_ids = BTreeSet::new();
    let mut chunks = Vec::with_capacity(files.len());
    for rel in files {
        let abs = root_path.join(&rel);
        let content =
            std::fs::read_to_string(&abs).with_context(|| format!("read {}", abs.display()))?;
        let id = assign_stable_id(&rel, &content, &mut used_ids);
        chunks.push(CollectChunk {
            id,
            kind: kind.to_string(),
            content,
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            sources: vec![CollectSource::String(format!("{}:1", rel.display()))],
        });
    }

    let output = CollectOutput {
        schema: CompileSchemaOut {
            dim,
            element_type: element_type.to_string(),
            quant_scale: quant_scale.or_else(|| (element_type == "i8").then_some(1.0)),
        },
        chunks,
    };

    let s = serde_json::to_string_pretty(&output)?;
    let out_path = Path::new(out);
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
    }
    std::fs::write(out_path, s.as_bytes()).with_context(|| format!("write {out}"))?;

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
                chunks: output.chunks.len(),
            })?
        );
    } else {
        println!("Wrote {out} ({} chunks)", output.chunks.len());
    }

    Ok(())
}
