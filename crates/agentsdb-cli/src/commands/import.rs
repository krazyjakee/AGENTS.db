use std::collections::HashSet;

use anyhow::Context;
use serde::Serialize;

use agentsdb_core::export::{ExportBundleV1, ExportChunkV1, ExportNdjsonRecordV1, ExportSourceV1};
use agentsdb_embeddings::config::{
    roll_up_embedding_options_from_paths, standard_layer_paths_for_dir,
};
use agentsdb_embeddings::layer_metadata::LayerMetadataV1;

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

fn flatten_chunks(bundle: ExportBundleV1) -> Vec<ExportChunkV1> {
    let mut out = Vec::new();
    for l in bundle.layers {
        out.extend(l.chunks);
    }
    out
}

fn parse_input_bytes(input: &[u8]) -> anyhow::Result<Vec<ExportChunkV1>> {
    let s = std::str::from_utf8(input).context("input must be valid UTF-8")?;
    let trimmed = s.trim_start();
    if trimmed.starts_with('{') {
        let bundle: ExportBundleV1 = serde_json::from_str(trimmed).context("parse JSON export")?;
        return Ok(flatten_chunks(bundle));
    }

    // NDJSON
    let mut chunks = Vec::new();
    for (i, line) in s.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec: ExportNdjsonRecordV1 =
            serde_json::from_str(line).with_context(|| format!("parse NDJSON line {}", i + 1))?;
        if let ExportNdjsonRecordV1::Chunk { chunk, .. } = rec {
            chunks.push(chunk);
        }
    }
    Ok(chunks)
}

fn resolve_target_path(dir: &str, target: &str, out: Option<&str>) -> anyhow::Result<String> {
    if let Some(p) = out {
        return Ok(p.to_string());
    }
    let siblings = standard_layer_paths_for_dir(std::path::Path::new(dir));
    let p = match target {
        "local" => siblings.local,
        "delta" => siblings.delta,
        "user" => siblings.user,
        "base" => siblings.base,
        _ => anyhow::bail!("--target must be local, delta, user, or base"),
    };
    Ok(p.to_string_lossy().to_string())
}

fn ensure_target_permissions(target: &str, path: &str, allow_base: bool) -> anyhow::Result<()> {
    match target {
        "local" => {
            if std::path::Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n != "AGENTS.local.db")
            {
                anyhow::bail!("target local expects file named AGENTS.local.db");
            }
            agentsdb_format::ensure_writable_layer_path(path).context("permission check")?;
        }
        "delta" => {
            if std::path::Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n != "AGENTS.delta.db")
            {
                anyhow::bail!("target delta expects file named AGENTS.delta.db");
            }
            agentsdb_format::ensure_writable_layer_path(path).context("permission check")?;
        }
        "user" => {
            if std::path::Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n != "AGENTS.user.db")
            {
                anyhow::bail!("target user expects file named AGENTS.user.db");
            }
            agentsdb_format::ensure_writable_layer_path_allow_user(path)
                .context("permission check")?;
        }
        "base" => {
            if !allow_base {
                anyhow::bail!("refusing to write AGENTS.db without --allow-base");
            }
            if std::path::Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n != "AGENTS.db")
            {
                anyhow::bail!("target base expects file named AGENTS.db");
            }
            agentsdb_format::ensure_writable_layer_path_allow_base(path)
                .context("permission check")?;
        }
        _ => anyhow::bail!("--target must be local, delta, or user, or base"),
    }
    Ok(())
}

fn sources_to_chunk_sources(sources: Vec<ExportSourceV1>) -> Vec<agentsdb_format::ChunkSource> {
    sources
        .into_iter()
        .map(|s| match s {
            ExportSourceV1::ChunkId { id } => agentsdb_format::ChunkSource::ChunkId(id),
            ExportSourceV1::SourceString { value } => {
                agentsdb_format::ChunkSource::SourceString(value)
            }
        })
        .collect()
}

pub(crate) fn cmd_import(
    dir: &str,
    input: &str,
    target: &str,
    out: Option<&str>,
    dry_run: bool,
    dedupe: bool,
    preserve_ids: bool,
    allow_base: bool,
    dim: Option<u32>,
    json: bool,
) -> anyhow::Result<()> {
    let target_path = resolve_target_path(dir, target, out)?;
    ensure_target_permissions(target, &target_path, allow_base)?;

    let bytes = std::fs::read(input).with_context(|| format!("read {}", input))?;
    let mut imported = parse_input_bytes(&bytes).context("parse import file")?;
    if imported.is_empty() {
        anyhow::bail!("no chunks found in import");
    }

    // Validate required fields and normalize hashes (do not trust exported hash blindly).
    for c in &mut imported {
        if c.content.is_none() {
            anyhow::bail!("import contains redacted/missing content; cannot import");
        }
        let h = content_sha256_hex(c.content.as_deref().unwrap_or_default());
        c.content_sha256 = Some(h);
    }

    let target_pathbuf = std::path::PathBuf::from(&target_path);
    let dir_path = target_pathbuf
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let siblings = standard_layer_paths_for_dir(dir_path);

    let mut existing_hashes: HashSet<String> = HashSet::new();
    let mut existing_ids: HashSet<u32> = HashSet::new();
    let (exists, dim_usize, existing_meta) = if target_pathbuf.exists() {
        let file = agentsdb_format::LayerFile::open(&target_path).context("open target layer")?;
        let chunks = agentsdb_format::read_all_chunks(&file).context("read target chunks")?;
        if dedupe {
            for c in &chunks {
                existing_hashes.insert(content_sha256_hex(&c.content));
            }
        }
        for c in &chunks {
            existing_ids.insert(c.id);
        }
        (
            true,
            file.embedding_dim(),
            file.layer_metadata_bytes().map(|b| b.to_vec()),
        )
    } else {
        (false, 0usize, None)
    };

    let inferred_dim = if exists {
        dim_usize
    } else if let Some(d) = dim {
        d as usize
    } else {
        let mut inferred: Option<usize> = None;
        for c in &imported {
            if let Some(emb) = c.embedding.as_ref() {
                inferred = Some(emb.len());
                break;
            }
        }
        inferred.context("creating a new layer requires --dim or input embeddings")?
    };

    let embedder_for_dim = |dim_usize: usize| -> anyhow::Result<
        Box<dyn agentsdb_embeddings::embedder::Embedder + Send + Sync>,
    > {
        let options = roll_up_embedding_options_from_paths(
            Some(siblings.local.as_path()),
            Some(siblings.user.as_path()),
            Some(siblings.delta.as_path()),
            Some(siblings.base.as_path()),
        )
        .context("roll up options")?;
        if let Some(cfg_dim) = options.dim {
            if cfg_dim != dim_usize {
                anyhow::bail!(
                    "embedding dim mismatch (target dim={dim_usize}, options specify dim={cfg_dim})"
                );
            }
        }
        options
            .into_embedder(dim_usize)
            .context("resolve embedder from options")
    };

    let mut layer_metadata_json: Option<Vec<u8>> = None;
    let mut embedder: Option<Box<dyn agentsdb_embeddings::embedder::Embedder + Send + Sync>> = None;

    // Prepare chunks.
    let mut prepared: Vec<agentsdb_format::ChunkInput> = Vec::new();
    let mut skipped = 0usize;
    let mut next_new_id = 1u32;

    if !exists && preserve_ids {
        for c in &imported {
            let id = c.id;
            if id == 0 {
                anyhow::bail!("--preserve-ids requires non-zero ids in input");
            }
            if existing_ids.contains(&id) {
                anyhow::bail!("id {id} already exists in target");
            }
            existing_ids.insert(id);
        }
    }

    for c in imported {
        let content = c.content.as_ref().expect("validated");
        let hash = c.content_sha256.as_deref().unwrap_or_default();
        if dedupe && existing_hashes.contains(hash) {
            skipped += 1;
            continue;
        }
        if dedupe {
            existing_hashes.insert(hash.to_string());
        }

        let embedding = match c.embedding {
            Some(v) => v,
            None => {
                if embedder.is_none() {
                    let e = embedder_for_dim(inferred_dim)?;
                    let meta = LayerMetadataV1::new(e.profile().clone())
                        .with_embedder_metadata(e.metadata())
                        .with_tool("agentsdb-cli", env!("CARGO_PKG_VERSION"));
                    layer_metadata_json =
                        Some(meta.to_json_bytes().context("serialize layer metadata")?);
                    embedder = Some(e);
                }
                let e = embedder.as_ref().expect("embedder");
                e.embed(&[content.clone()])?
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| vec![0.0; inferred_dim])
            }
        };
        if embedding.len() != inferred_dim {
            anyhow::bail!(
                "embedding dim mismatch in import chunk id={} (got {}, expected {})",
                c.id,
                embedding.len(),
                inferred_dim
            );
        }

        let id = if exists {
            if preserve_ids {
                if existing_ids.contains(&c.id) {
                    anyhow::bail!(
                        "id {} already exists in target (use --preserve-ids=false)",
                        c.id
                    );
                }
                existing_ids.insert(c.id);
                c.id
            } else {
                0
            }
        } else if preserve_ids {
            c.id
        } else {
            while existing_ids.contains(&next_new_id) {
                next_new_id = next_new_id.saturating_add(1);
            }
            existing_ids.insert(next_new_id);
            let assigned = next_new_id;
            next_new_id = next_new_id.saturating_add(1);
            assigned
        };

        prepared.push(agentsdb_format::ChunkInput {
            id,
            kind: c.kind,
            content: content.clone(),
            author: c.author,
            confidence: c.confidence,
            created_at_unix_ms: c.created_at_unix_ms,
            embedding,
            sources: sources_to_chunk_sources(c.sources),
        });
    }

    if prepared.is_empty() {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            path: &'a str,
            imported: usize,
            skipped: usize,
            dry_run: bool,
        }
        let out = Out {
            ok: true,
            path: &target_path,
            imported: 0,
            skipped,
            dry_run,
        };
        if json {
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!("No chunks imported (skipped={skipped})");
        }
        return Ok(());
    }

    // If computing embeddings into an existing layer, enforce embedder profile compatibility.
    if let (Some(existing_meta), Some(layer_metadata_json)) =
        (existing_meta.as_ref(), layer_metadata_json.as_ref())
    {
        let existing = LayerMetadataV1::from_json_bytes(existing_meta)
            .context("parse existing layer metadata")?;
        let desired = LayerMetadataV1::from_json_bytes(layer_metadata_json)
            .context("parse desired layer metadata")?;
        if existing.embedding_profile != desired.embedding_profile {
            anyhow::bail!(
                "embedder profile mismatch vs target layer metadata (existing={:?}, current={:?})",
                existing.embedding_profile,
                desired.embedding_profile
            );
        }
    }

    if dry_run {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            path: &'a str,
            imported: usize,
            skipped: usize,
            dry_run: bool,
        }
        let out = Out {
            ok: true,
            path: &target_path,
            imported: prepared.len(),
            skipped,
            dry_run: true,
        };
        if json {
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!(
                "Dry-run: would import {} chunks to {} (skipped={})",
                prepared.len(),
                target_path,
                skipped
            );
        }
        return Ok(());
    }

    let imported_count = prepared.len();
    if exists {
        let mut new_chunks = prepared;
        let _ = agentsdb_format::append_layer_atomic(
            &target_path,
            &mut new_chunks,
            layer_metadata_json.as_deref(),
        )
        .context("append")?;
    } else {
        let schema = agentsdb_format::LayerSchema {
            dim: inferred_dim as u32,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        agentsdb_format::write_layer_atomic(
            &target_path,
            &schema,
            &prepared,
            layer_metadata_json.as_deref(),
        )
        .context("create layer")?;
    }

    #[derive(Serialize)]
    struct Out<'a> {
        ok: bool,
        path: &'a str,
        imported: usize,
        skipped: usize,
        dry_run: bool,
    }
    let out = Out {
        ok: true,
        path: &target_path,
        imported: imported_count,
        skipped,
        dry_run: false,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "Imported {} chunks to {} (skipped={})",
            imported_count, target_path, skipped
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::cmd_import;

    use crate::commands::export::cmd_export;

    fn write_test_layer(path: &std::path::Path, dim: usize) {
        let schema = agentsdb_format::LayerSchema {
            dim: dim as u32,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        let chunk = agentsdb_format::ChunkInput {
            id: 1,
            kind: "note".to_string(),
            content: "hello".to_string(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 1,
            embedding: vec![0.0; dim],
            sources: vec![agentsdb_format::ChunkSource::SourceString(
                "README.md:1".to_string(),
            )],
        };
        agentsdb_format::write_layer_atomic(path, &schema, &[chunk], None).expect("write layer");
    }

    #[test]
    fn export_then_import_creates_target_layer() {
        let td = tempfile::tempdir().expect("tempdir");
        let dir = td.path();
        write_test_layer(&dir.join("AGENTS.local.db"), 8);

        let export_path = dir.join("out.json");
        cmd_export(
            dir.to_str().unwrap(),
            "json",
            "local",
            Some(export_path.to_str().unwrap()),
            "none",
            false,
        )
        .expect("export");

        cmd_import(
            dir.to_str().unwrap(),
            export_path.to_str().unwrap(),
            "delta",
            None,
            false,
            false,
            false,
            false,
            None,
            true,
        )
        .expect("import");

        let delta =
            agentsdb_format::LayerFile::open(dir.join("AGENTS.delta.db")).expect("open delta");
        let chunks = agentsdb_format::read_all_chunks(&delta).expect("read chunks");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "hello");
        assert_eq!(chunks[0].embedding.len(), 8);
    }

    #[test]
    fn import_with_dedupe_skips_duplicates() {
        let td = tempfile::tempdir().expect("tempdir");
        let dir = td.path();
        write_test_layer(&dir.join("AGENTS.local.db"), 8);

        let export_path = dir.join("out.json");
        cmd_export(
            dir.to_str().unwrap(),
            "json",
            "local",
            Some(export_path.to_str().unwrap()),
            "none",
            false,
        )
        .expect("export");

        cmd_import(
            dir.to_str().unwrap(),
            export_path.to_str().unwrap(),
            "delta",
            None,
            false,
            true,
            false,
            false,
            None,
            false,
        )
        .expect("import 1");
        cmd_import(
            dir.to_str().unwrap(),
            export_path.to_str().unwrap(),
            "delta",
            None,
            false,
            true,
            false,
            false,
            None,
            false,
        )
        .expect("import 2");

        let delta =
            agentsdb_format::LayerFile::open(dir.join("AGENTS.delta.db")).expect("open delta");
        let chunks = agentsdb_format::read_all_chunks(&delta).expect("read chunks");
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn import_rejects_redacted_content() {
        let td = tempfile::tempdir().expect("tempdir");
        let dir = td.path();
        write_test_layer(&dir.join("AGENTS.local.db"), 8);

        let export_path = dir.join("out.json");
        cmd_export(
            dir.to_str().unwrap(),
            "json",
            "local",
            Some(export_path.to_str().unwrap()),
            "content",
            false,
        )
        .expect("export");

        let err = cmd_import(
            dir.to_str().unwrap(),
            export_path.to_str().unwrap(),
            "delta",
            None,
            true,
            false,
            false,
            false,
            None,
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("redacted") || err.contains("missing content"));
    }
}
