use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

use crate::util::parse_ids_csv;

pub(crate) fn cmd_promote(
    from_path: &str,
    to_path: &str,
    ids: &str,
    json: bool,
) -> anyhow::Result<()> {
    let wanted = parse_ids_csv(ids)?;
    if wanted.is_empty() {
        anyhow::bail!("--ids must be non-empty");
    }

    agentsdb_format::ensure_writable_layer_path_allow_user(to_path).context("permission check")?;

    let from_file =
        agentsdb_format::LayerFile::open(from_path).with_context(|| format!("open {from_path}"))?;
    let from_schema = agentsdb_format::schema_of(&from_file);
    let from_metadata = from_file.layer_metadata_bytes().map(|b| b.to_vec());
    let from_chunks = agentsdb_format::read_all_chunks(&from_file)?;

    let by_id: BTreeMap<u32, agentsdb_format::ChunkInput> =
        from_chunks.into_iter().map(|c| (c.id, c)).collect();

    let mut promote = Vec::new();
    for id in &wanted {
        let Some(c) = by_id.get(id) else {
            anyhow::bail!("id {id} not found in {from_path}");
        };
        let mut c = c.clone();
        if c.author != "human" {
            c.author = "human".to_string();
        }
        promote.push(c);
    }

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

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            ok: bool,
            from: &'a str,
            to: &'a str,
            ids: Vec<u32>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                from: from_path,
                to: to_path,
                ids: wanted,
            })?
        );
    } else {
        println!(
            "Promoted {} chunks from {from_path} to {to_path}",
            wanted.len()
        );
    }

    Ok(())
}
