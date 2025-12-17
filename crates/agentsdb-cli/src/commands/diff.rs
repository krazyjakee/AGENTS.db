use anyhow::Context;
use serde::Serialize;
use std::collections::BTreeSet;

pub(crate) fn cmd_diff(
    base: &str,
    delta: &str,
    target: Option<&str>,
    user: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
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

    let (target_name, target_conflicts) = match target {
        None => (None, Vec::new()),
        Some("user") => {
            let user =
                user.ok_or_else(|| anyhow::anyhow!("--user is required when --target user"))?;
            let user_path = std::path::Path::new(user);
            if !user_path.exists() {
                (Some("user"), Vec::new())
            } else {
                let user_file = agentsdb_format::LayerFile::open(user)
                    .with_context(|| format!("open {user}"))?;
                let user_chunks = agentsdb_format::read_all_chunks(&user_file)?;
                let user_ids: BTreeSet<u32> = user_chunks.iter().map(|c| c.id).collect();
                let mut conflicts: Vec<u32> = delta_chunks
                    .iter()
                    .map(|c| c.id)
                    .filter(|id| user_ids.contains(id))
                    .collect();
                conflicts.sort_unstable();
                conflicts.dedup();
                (Some("user"), conflicts)
            }
        }
        Some(other) => anyhow::bail!("unsupported --target {other:?}"),
    };

    if json {
        #[derive(Serialize)]
        struct Out<'a> {
            base: &'a str,
            delta: &'a str,
            delta_count: usize,
            new_ids: Vec<u32>,
            overrides: Vec<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            target: Option<&'a str>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            target_conflicts: Vec<u32>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                base,
                delta,
                delta_count: delta_chunks.len(),
                new_ids,
                overrides,
                target: target_name,
                target_conflicts,
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
    if let Some(target) = target_name {
        println!(
            "Conflicts (id exists in {target}): {}",
            target_conflicts.len()
        );
        for id in &target_conflicts {
            println!("  - {id}");
        }
    }
    Ok(())
}
