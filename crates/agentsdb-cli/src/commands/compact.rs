use anyhow::Context;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) fn cmd_compact(
    base: Option<&str>,
    user: Option<&str>,
    out: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().context("resolve current directory")?;

    if base.is_none() && user.is_none() && out.is_none() {
        let compacted = compact_all_in_dir(&cwd).context("compact all")?;
        if json {
            #[derive(Serialize)]
            struct Out<'a> {
                ok: bool,
                dir: &'a str,
                compacted: Vec<String>,
            }
            let rendered = compacted
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&Out {
                    ok: true,
                    dir: &cwd.to_string_lossy(),
                    compacted: rendered,
                })?
            );
        } else {
            println!("Compacted {} layer file(s)", compacted.len());
        }
        return Ok(());
    }

    let (base, user) = apply_default_layer_paths(base, user, &cwd);
    let out = match out {
        Some(v) => v.to_string(),
        None => default_out_path(base.as_deref(), user.as_deref())
            .context("--out is required when no input layers are provided")?,
    };

    if base.is_none() && user.is_none() {
        anyhow::bail!(
            "no layers provided (use --base/--user, or run from a directory containing AGENTS.db/AGENTS.user.db)"
        );
    }

    agentsdb_format::ensure_writable_layer_path_allow_user(&out)
        .context("refuse to write compacted output to a non-writable layer path")?;

    let (schema, chunks) = compact_layers(base.as_deref(), user.as_deref()).context("compact")?;
    agentsdb_format::write_layer_atomic(&out, &schema, &chunks, None)
        .context("write compacted layer")?;

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            base: Option<&'a str>,
            user: Option<&'a str>,
            out: &'a str,
            chunks: usize,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                base: base.as_deref(),
                user: user.as_deref(),
                out: &out,
                chunks: chunks.len(),
            })?
        );
    } else {
        println!("Wrote {out} ({} chunks)", chunks.len());
    }

    Ok(())
}

fn compact_all_in_dir(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut compacted = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry.context("read_dir entry")?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| name == "AGENTS.db")
        {
            continue;
        }
        let is_db = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("db"))
            .unwrap_or(false);
        if !is_db {
            continue;
        }

        if agentsdb_format::ensure_writable_layer_path_allow_user(&path).is_err() {
            continue;
        }

        let Ok(file) = agentsdb_format::LayerFile::open_lenient(&path) else {
            continue;
        };

        let schema = agentsdb_format::schema_of(&file);
        let chunks = agentsdb_format::read_all_chunks(&file)
            .with_context(|| format!("read chunks from {}", path.display()))?;
        agentsdb_format::write_layer_atomic(&path, &schema, &chunks, None)
            .with_context(|| format!("rewrite {}", path.display()))?;
        compacted.push(path);
    }
    Ok(compacted)
}

fn default_out_path(base: Option<&str>, user: Option<&str>) -> Option<String> {
    let base_dir = base
        .and_then(|p| Path::new(p).parent())
        .map(ToOwned::to_owned);
    let user_dir = user
        .and_then(|p| Path::new(p).parent())
        .map(ToOwned::to_owned);
    let dir = base_dir.or(user_dir)?;
    Some(
        dir.join("AGENTS.compacted.db")
            .to_string_lossy()
            .into_owned(),
    )
}

fn compact_layers(
    base: Option<&str>,
    user: Option<&str>,
) -> anyhow::Result<(
    agentsdb_format::LayerSchema,
    Vec<agentsdb_format::ChunkInput>,
)> {
    let mut schema: Option<agentsdb_format::LayerSchema> = None;
    let mut by_id: BTreeMap<u32, agentsdb_format::ChunkInput> = BTreeMap::new();

    for (layer_name, path) in [("base", base), ("user", user)] {
        let Some(path) = path else { continue };
        let file = agentsdb_format::LayerFile::open_lenient(path)
            .with_context(|| format!("open {layer_name} layer {path}"))?;
        let layer_schema = agentsdb_format::schema_of(&file);
        if let Some(s) = &schema {
            if s.dim != layer_schema.dim
                || s.element_type != layer_schema.element_type
                || s.quant_scale.to_bits() != layer_schema.quant_scale.to_bits()
            {
                anyhow::bail!(
                    "schema mismatch between layers (expected dim={} type={:?} scale={}, got dim={} type={:?} scale={})",
                    s.dim,
                    s.element_type,
                    s.quant_scale,
                    layer_schema.dim,
                    layer_schema.element_type,
                    layer_schema.quant_scale
                );
            }
        } else {
            schema = Some(layer_schema);
        }

        for c in agentsdb_format::read_all_chunks(&file)? {
            // When duplicates exist (either within a file or across layers),
            // always keep the newest entry (last occurrence).
            // This allows compact to fix corrupted files with duplicate IDs.
            by_id.insert(c.id, c);
        }
    }

    let schema = schema.context("no schema (no input layers opened)")?;
    let mut chunks: Vec<agentsdb_format::ChunkInput> = by_id.into_values().collect();
    chunks.sort_by_key(|c| c.id);
    ensure_nonzero_unique_ids(&chunks)?;
    Ok((schema, chunks))
}

fn ensure_nonzero_unique_ids(chunks: &[agentsdb_format::ChunkInput]) -> anyhow::Result<()> {
    let mut seen = BTreeSet::new();
    for c in chunks {
        if c.id == 0 {
            anyhow::bail!("invalid chunk id 0 in input layer");
        }
        if !seen.insert(c.id) {
            anyhow::bail!("duplicate chunk id {} in compacted output", c.id);
        }
    }
    Ok(())
}

fn apply_default_layer_paths(
    base: Option<&str>,
    user: Option<&str>,
    cwd: &Path,
) -> (Option<String>, Option<String>) {
    let mut base = base.map(ToString::to_string);
    let mut user = user.map(ToString::to_string);

    let base_default = cwd.join("AGENTS.db");
    let user_default = cwd.join("AGENTS.user.db");

    if base.is_none() && base_default.exists() {
        base = Some(path_to_string(base_default));
    }
    if user.is_none() && user_default.exists() {
        user = Some(path_to_string(user_default));
    }

    (base, user)
}

fn path_to_string(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn schema() -> agentsdb_format::LayerSchema {
        agentsdb_format::LayerSchema {
            dim: 4,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        }
    }

    fn chunk(id: u32, kind: &str, content: &str) -> agentsdb_format::ChunkInput {
        agentsdb_format::ChunkInput {
            id,
            kind: kind.to_string(),
            content: content.to_string(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: vec![0.0, 0.0, 0.0, 0.0],
            sources: Vec::new(),
        }
    }

    #[test]
    fn compacts_base_plus_user() {
        let dir = crate::util::make_temp_dir();
        let base_path = dir.join("AGENTS.db");
        let user_path = dir.join("AGENTS.user.db");
        let out_path = dir.join("AGENTS.compacted.db");

        agentsdb_format::write_layer_atomic(
            &base_path,
            &schema(),
            &[
                chunk(1, "canonical", "base a"),
                chunk(2, "canonical", "base b"),
            ],
            None,
        )
        .unwrap();
        agentsdb_format::write_layer_atomic(
            &user_path,
            &schema(),
            &[chunk(100, "note", "user x")],
            None,
        )
        .unwrap();

        let base_s = base_path.to_string_lossy().into_owned();
        let user_s = user_path.to_string_lossy().into_owned();
        cmd_compact(Some(&base_s), Some(&user_s), None, true).unwrap();

        let out_file = agentsdb_format::LayerFile::open(&out_path).unwrap();
        let chunks = agentsdb_format::read_all_chunks(&out_file).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].id, 1);
        assert_eq!(chunks[1].id, 2);
        assert_eq!(chunks[2].id, 100);

        let base_file = agentsdb_format::LayerFile::open(&base_path).unwrap();
        let base_chunks = agentsdb_format::read_all_chunks(&base_file).unwrap();
        assert_eq!(base_chunks.len(), 2);
    }

    #[test]
    fn keeps_newest_when_ids_conflict() {
        let dir = crate::util::make_temp_dir();
        let base_path = dir.join("AGENTS.db");
        let user_path = dir.join("AGENTS.user.db");

        agentsdb_format::write_layer_atomic(
            &base_path,
            &schema(),
            &[chunk(1, "canonical", "old content")],
            None,
        )
        .unwrap();
        agentsdb_format::write_layer_atomic(
            &user_path,
            &schema(),
            &[chunk(1, "canonical", "new content")],
            None,
        )
        .unwrap();

        let base_s = base_path.to_string_lossy().into_owned();
        let user_s = user_path.to_string_lossy().into_owned();
        let (_, chunks) = compact_layers(Some(&base_s), Some(&user_s)).unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].id, 1);
        assert_eq!(chunks[0].content, "new content");
    }

    #[test]
    fn compact_all_in_dir_rewrites_all_valid_db_files() {
        let dir = crate::util::make_temp_dir();
        let a_path = dir.join("AGENTS.db");
        let b_path = dir.join("AGENTS.user.db");
        let junk_path = dir.join("junk.db");
        let other_path = dir.join("notes.txt");

        agentsdb_format::write_layer_atomic(
            &a_path,
            &schema(),
            &[chunk(1, "canonical", "a")],
            None,
        )
        .unwrap();
        agentsdb_format::write_layer_atomic(&b_path, &schema(), &[chunk(2, "note", "b")], None)
            .unwrap();
        std::fs::write(&junk_path, b"not an agentsdb layer").unwrap();
        std::fs::write(&other_path, b"ignore").unwrap();

        let compacted = compact_all_in_dir(&dir).unwrap();
        let rendered: HashSet<String> = compacted
            .into_iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert_eq!(rendered, HashSet::from(["AGENTS.user.db".to_string()]));
    }
}
