use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

use crate::commands::compile::compile_to_layer;
use crate::types::{CompileChunk, CompileInput, CompileSchema, CompileSource};
use crate::util::{assign_stable_id, collect_files_wide_docs};

/// Ensures that AGENTS.local.db is in .gitignore
fn ensure_gitignore_entry(root_path: &Path) -> anyhow::Result<()> {
    let gitignore_path = root_path.join(".gitignore");

    if !gitignore_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&gitignore_path).context("Failed to read .gitignore")?;

    if content.lines().any(|line| line.trim() == "AGENTS.local.db") {
        return Ok(());
    }

    let updated_content = if content.ends_with('\n') {
        format!("{}AGENTS.local.db\n", content)
    } else {
        format!("{}\nAGENTS.local.db\n", content)
    };

    std::fs::write(&gitignore_path, updated_content).context("Failed to write .gitignore")?;

    println!("Added AGENTS.local.db to .gitignore");
    Ok(())
}

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

    // Ensure .gitignore has AGENTS.local.db
    ensure_gitignore_entry(root_path)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_temp_dir() -> PathBuf {
        static CTR: AtomicUsize = AtomicUsize::new(0);
        let n = CTR.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "agentsdb_cli_init_test_{}_{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        p
    }

    #[test]
    fn init_does_not_modify_readme() {
        let root = make_temp_dir();
        let readme_path = root.join("README.md");
        let original = "# Title\n\nSome content.\n";
        std::fs::write(&readme_path, original).expect("write README");

        // Include a .gitignore to exercise init side effects without touching README.
        std::fs::write(root.join(".gitignore"), "target/\n").expect("write .gitignore");

        let out_path = root.join("AGENTS.test.db");
        let root_s = root.to_string_lossy().to_string();
        let out_s = out_path.to_string_lossy().to_string();
        cmd_init(&root_s, &out_s, "docs", 8, "f32", None, true).expect("init should succeed");

        let after = std::fs::read_to_string(&readme_path).expect("read README");
        assert_eq!(after, original);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
