use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Default, Serialize)]
pub struct PromoteOutcome {
    pub promoted: Vec<u32>,
    pub skipped: Vec<u32>,
}

/// Promote chunks from one layer to another
///
/// # Arguments
/// * `from_path` - Source layer path
/// * `to_path` - Destination layer path
/// * `ids` - Chunk IDs to promote
/// * `_skip_existing` - (Deprecated) No longer used; promoted chunks always receive new auto-assigned IDs
///
/// # Returns
/// A PromoteOutcome containing lists of promoted and skipped IDs
pub fn promote_chunks(
    from_path: &str,
    to_path: &str,
    ids: &[u32],
    _skip_existing: bool,
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
    }

    // Note: We no longer check for ID collisions because promoted chunks
    // will receive auto-assigned IDs in the target layer (id=0 triggers auto-assignment)
    let filtered: Vec<u32> = ids.to_vec();
    let skipped = Vec::new();

    if filtered.is_empty() {
        return Ok(PromoteOutcome {
            promoted: Vec::new(),
            skipped,
        });
    }

    let mut promote = Vec::new();
    for id in &filtered {
        let Some(c) = by_id.get(id) else {
            anyhow::bail!("id {id} not found in {from_path}");
        };
        let mut c = c.clone();
        c.id = 0; // Force auto-assignment of new ID in target layer
        if c.author != "human" {
            c.author = "human".to_string();
        }
        promote.push(c);
    }

    let assigned_ids = if to_p.exists() {
        agentsdb_format::append_layer_atomic(to_path, &mut promote, None).context("append")?
    } else {
        agentsdb_format::write_layer_atomic(
            to_path,
            &from_schema,
            &mut promote,
            from_metadata.as_deref(),
        )
        .context("write")?
    };

    Ok(PromoteOutcome {
        promoted: assigned_ids,
        skipped,
    })
}
