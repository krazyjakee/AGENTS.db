use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeSet;

pub(crate) fn cmd_diff(base: &str, delta: &str, json: bool) -> anyhow::Result<()> {
    let base_file =
        agentsdb_format::LayerFile::open(base).with_context(|| format!("open {base}"))?;
    let delta_file =
        agentsdb_format::LayerFile::open(delta).with_context(|| format!("open {delta}"))?;
    let base_chunks = agentsdb_format::read_all_chunks(&base_file)?;
    let delta_chunks = agentsdb_format::read_all_chunks(&delta_file)?;

    let base_ids: BTreeSet<u32> = base_chunks.iter().map(|c| c.id).collect();
    let mut new_ids = Vec::new();
    let mut overrides = Vec::new();
    for c in &delta_chunks {
        if base_ids.contains(&c.id) {
            overrides.push(c.id);
        } else {
            new_ids.push(c.id);
        }
    }
    new_ids.sort_unstable();
    overrides.sort_unstable();

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            base: &'a str,
            delta: &'a str,
            delta_count: usize,
            new_ids: Vec<u32>,
            overrides: Vec<u32>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                base,
                delta,
                delta_count: delta_chunks.len(),
                new_ids,
                overrides,
            })?
        );
        return Ok(());
    }

    println!("Delta: {delta} ({} chunks)", delta_chunks.len());
    println!("New ids (not present in base): {}", new_ids.len());
    for id in &new_ids {
        println!("  - {id}");
    }
    println!("Overrides (id exists in base): {}", overrides.len());
    for id in &overrides {
        println!("  - {id}");
    }
    Ok(())
}
