use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::Path;

use crate::util::parse_ids_csv;

#[derive(Debug, Default)]
pub(crate) struct PromoteOutcome {
    pub(crate) promoted: Vec<u32>,
    pub(crate) skipped: Vec<u32>,
}

pub(crate) fn promote_chunks(
    from_path: &str,
    to_path: &str,
    ids: &[u32],
    skip_existing: bool,
    tombstone_source: bool,
    yes: bool,
) -> anyhow::Result<PromoteOutcome> {
    if ids.is_empty() {
        anyhow::bail!("ids must be non-empty");
    }

    agentsdb_format::ensure_writable_layer_path_allow_user(to_path).context("permission check")?;

    let from_file =
        agentsdb_format::LayerFile::open(from_path).with_context(|| format!("open {from_path}"))?;
    let from_schema = agentsdb_format::schema_of(&from_file);
    let from_metadata = from_file.layer_metadata_bytes().map(|b| b.to_vec());
    let from_chunks = agentsdb_format::read_all_chunks(&from_file)?;

    let by_id: BTreeMap<u32, agentsdb_format::ChunkInput> =
        from_chunks.into_iter().map(|c| (c.id, c)).collect();

    let to_p = Path::new(to_path);
    let mut to_existing_ids: BTreeSet<u32> = BTreeSet::new();
    if to_p.exists() {
        let to_file =
            agentsdb_format::LayerFile::open(to_path).with_context(|| format!("open {to_path}"))?;
        let to_schema = agentsdb_format::schema_of(&to_file);
        if to_schema.dim != from_schema.dim
            || to_schema.element_type != from_schema.element_type
            || to_schema.quant_scale.to_bits() != from_schema.quant_scale.to_bits()
        {
            anyhow::bail!("schema mismatch between {from_path} and {to_path}");
        }
        to_existing_ids = agentsdb_format::read_all_chunks(&to_file)?
            .into_iter()
            .map(|c| c.id)
            .collect();
    }

    let mut filtered = Vec::new();
    let mut skipped = Vec::new();
    for id in ids {
        if to_existing_ids.contains(id) {
            if skip_existing {
                skipped.push(*id);
                continue;
            }
            anyhow::bail!(
                "destination already contains id {id} (use --skip-existing to skip duplicates)"
            );
        }
        filtered.push(*id);
    }

    if filtered.is_empty() {
        return Ok(PromoteOutcome {
            promoted: Vec::new(),
            skipped,
        });
    }

    if !yes
        && Path::new(to_path).file_name().and_then(|s| s.to_str()) == Some("AGENTS.user.db")
        && std::io::stdin().is_terminal()
    {
        eprint!(
            "Promote {} chunks into {to_path}? This is a durable, append-only layer. [y/N] ",
            filtered.len()
        );
        use std::io::Write;
        std::io::stderr().flush().ok();
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).ok();
        let s = s.trim().to_ascii_lowercase();
        if s != "y" && s != "yes" {
            anyhow::bail!("aborted");
        }
    }

    let mut promote = Vec::new();
    for id in &filtered {
        let Some(c) = by_id.get(id) else {
            anyhow::bail!("id {id} not found in {from_path}");
        };
        let mut c = c.clone();
        if c.author != "human" {
            c.author = "human".to_string();
        }
        promote.push(c);
    }

    if to_p.exists() {
        agentsdb_format::append_layer_atomic(to_path, &mut promote, None).context("append")?;
    } else {
        agentsdb_format::write_layer_atomic(
            to_path,
            &from_schema,
            &promote,
            from_metadata.as_deref(),
        )
        .context("write")?;
    }

    // Tombstone promoted chunks in source layer if requested
    if tombstone_source && !filtered.is_empty() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut tombstones = Vec::new();
        for id in &filtered {
            // Read the chunk to get its embedding
            let chunk = by_id.get(id).ok_or_else(|| {
                anyhow::anyhow!("chunk id {id} not found in {from_path}")
            })?;

            let tombstone = agentsdb_format::ChunkInput {
                id: 0, // Will be auto-assigned
                kind: "tombstone".to_string(),
                content: format!("Promoted chunk {} to {}", id, to_path),
                author: "human".to_string(),
                confidence: 1.0,
                created_at_unix_ms: now_ms,
                embedding: chunk.embedding.clone(),
                sources: vec![agentsdb_format::ChunkSource::ChunkId(*id)],
            };
            tombstones.push(tombstone);
        }

        agentsdb_format::append_layer_atomic(from_path, &mut tombstones, None)
            .context("append tombstones to source layer")?;
    }

    Ok(PromoteOutcome {
        promoted: filtered,
        skipped,
    })
}

pub(crate) fn cmd_promote(
    from_path: &str,
    to_path: &str,
    ids: &str,
    skip_existing: bool,
    tombstone_source: bool,
    yes: bool,
    json: bool,
) -> anyhow::Result<()> {
    let wanted = parse_ids_csv(ids)?;
    if wanted.is_empty() {
        anyhow::bail!("--ids must be non-empty");
    }
    let out = promote_chunks(from_path, to_path, &wanted, skip_existing, tombstone_source, yes || json)?;
    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            from: &'a str,
            to: &'a str,
            promoted: Vec<u32>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            skipped: Vec<u32>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                from: from_path,
                to: to_path,
                promoted: out.promoted,
                skipped: out.skipped,
            })?
        );
    } else {
        if out.promoted.is_empty() {
            println!("No chunks to promote (all requested ids already exist in {to_path})");
        } else {
            println!(
                "Promoted {} chunks from {from_path} to {to_path}",
                out.promoted.len()
            );
        }
        if !out.skipped.is_empty() {
            println!(
                "Skipped {} ids already present in destination",
                out.skipped.len()
            );
        }
    }

    Ok(())
}
