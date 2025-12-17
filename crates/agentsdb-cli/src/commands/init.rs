use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

use agentsdb_embeddings::config::get_immutable_embedding_options;

use crate::commands::compile::compile_to_layer;
use crate::types::{CompileChunk, CompileInput, CompileSchema, CompileSource};
use crate::util::{assign_stable_id, collect_files_wide_docs};

const AGENT_NOTES_BLOCK: &str = r#"## Agent-Specific Notes

This repository includes a compiled documentation database/knowledgebase at `AGENTS.db`.
For context for any task, you MUST use MCP `agents_search` to look up context including architectural, API, and historical changes.
Treat `AGENTS.db` layers as immutable; avoid in-place mutation utilities unless required by the design.
"#;

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

fn agent_notes_present(content: &str) -> bool {
    content.contains("## Agent-Specific Notes")
        && content.contains("compiled documentation database/knowledgebase at `AGENTS.db`")
        && content.contains("MCP `agents_search`")
        && content.contains("layers as immutable")
}

fn ensure_agent_notes(root_path: &Path) -> anyhow::Result<()> {
    for file_name in ["AGENTS.md", "GEMINI.md", "CLAUDE.md"] {
        let path = root_path.join(file_name);
        if !path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        if agent_notes_present(&content) {
            continue;
        }

        let mut updated = content;
        if !updated.is_empty() {
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            if !updated.ends_with("\n\n") {
                updated.push('\n');
            }
        }
        updated.push_str(AGENT_NOTES_BLOCK);

        std::fs::write(&path, updated)
            .with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_init(
    root: &str,
    out: &str,
    kind: &str,
    dim: Option<u32>,
    element_type: &str,
    quant_scale: Option<f32>,
    json: bool,
) -> anyhow::Result<()> {
    let resolved_dim = match dim {
        Some(v) => v,
        None => {
            let out_path = Path::new(out);
            let out_dir = out_path.parent().unwrap_or_else(|| Path::new("."));
            let options = get_immutable_embedding_options(out_dir)
                .context("get immutable embedding options")?;
            options
                .dim
                .map(|v| u32::try_from(v).context("configured dim overflows u32"))
                .transpose()?
                .unwrap_or(128)
        }
    };
    if resolved_dim == 0 {
        anyhow::bail!("--dim must be non-zero");
    }
    if element_type != "f32" && element_type != "i8" {
        anyhow::bail!("--element-type must be 'f32' or 'i8'");
    }

    let root_path = Path::new(root);

    // Ensure .gitignore has AGENTS.local.db
    ensure_gitignore_entry(root_path)?;

    // Ensure agent notes are present in relevant instruction files.
    ensure_agent_notes(root_path)?;

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
            dim: resolved_dim,
            element_type: element_type.to_string(),
            quant_scale: quant_scale.or_else(|| (element_type == "i8").then_some(1.0)),
        },
        chunks,
    };
    let (_action, chunks) = compile_to_layer(&mut input, out, true).context("compile")?;

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

    #[test]
    fn init_does_not_modify_readme() {
        let root = crate::util::make_temp_dir();
        let readme_path = root.join("README.md");
        let original = "# Title\n\nSome content.\n";
        std::fs::write(&readme_path, original).expect("write README");

        // Include a .gitignore to exercise init side effects without touching README.
        std::fs::write(root.join(".gitignore"), "target/\n").expect("write .gitignore");

        let out_path = root.join("AGENTS.test.db");
        let root_s = root.to_string_lossy().to_string();
        let out_s = out_path.to_string_lossy().to_string();
        cmd_init(&root_s, &out_s, "docs", Some(8), "f32", None, true).expect("init should succeed");

        let after = std::fs::read_to_string(&readme_path).expect("read README");
        assert_eq!(after, original);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn init_appends_agent_notes_when_missing() {
        let root = crate::util::make_temp_dir();

        std::fs::write(root.join(".gitignore"), "target/\n").expect("write .gitignore");
        std::fs::write(root.join("README.md"), "# Title\n").expect("write README");

        let agents_path = root.join("AGENTS.md");
        std::fs::write(&agents_path, "# Repo Instructions\n").expect("write AGENTS");

        let claude_path = root.join("CLAUDE.md");
        std::fs::write(&claude_path, AGENT_NOTES_BLOCK).expect("write CLAUDE");

        let out_path = root.join("AGENTS.test.db");
        let root_s = root.to_string_lossy().to_string();
        let out_s = out_path.to_string_lossy().to_string();
        cmd_init(&root_s, &out_s, "docs", Some(8), "f32", None, true).expect("init should succeed");

        let agents_after = std::fs::read_to_string(&agents_path).expect("read AGENTS");
        assert!(agents_after.contains("## Agent-Specific Notes"));
        assert!(agents_after.contains("MCP `agents_search`"));
        assert!(agents_after.ends_with(AGENT_NOTES_BLOCK));

        let claude_after = std::fs::read_to_string(&claude_path).expect("read CLAUDE");
        assert_eq!(
            claude_after.matches("## Agent-Specific Notes").count(),
            1,
            "init should not duplicate notes"
        );

        assert!(
            !root.join("GEMINI.md").exists(),
            "init should not create missing files"
        );

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn init_uses_configured_dim_when_dim_is_omitted() {
        let root = crate::util::make_temp_dir();
        std::fs::write(root.join(".gitignore"), "target/\n").expect("write .gitignore");
        std::fs::write(root.join("README.md"), "# Title\n").expect("write README");

        let root_s = root.to_string_lossy().to_string();
        // Write options to base layer (AGENTS.db) since that's the only layer read for immutable config
        crate::commands::options::cmd_options_set(
            &root_s,
            "base",
            None,
            None,
            None,
            None,
            None,
            Some(8),
            None,
            None,
            None,
            None,
            true,
        )
        .expect("write options");

        let out_path = root.join("AGENTS.test.db");
        let out_s = out_path.to_string_lossy().to_string();
        cmd_init(&root_s, &out_s, "docs", None, "f32", None, true).expect("init should succeed");

        let file = agentsdb_format::LayerFile::open(&out_path).expect("open out layer");
        let schema = agentsdb_format::schema_of(&file);
        assert_eq!(schema.dim, 8);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
