use anyhow::Context;
use serde::Serialize;

use agentsdb_embeddings::config::standard_layer_paths_for_dir;

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

    // Read input file
    let bytes = std::fs::read(input).with_context(|| format!("read {}", input))?;
    let data = std::str::from_utf8(&bytes).context("input must be valid UTF-8")?;

    // Use shared import operation
    let outcome = agentsdb_ops::import::import_into_layer(
        std::path::Path::new(&target_path),
        target,
        data,
        dry_run,
        dedupe,
        preserve_ids,
        allow_base,
        dim,
        "agentsdb-cli",
        env!("CARGO_PKG_VERSION"),
    )?;

    // Format output
    #[derive(Serialize)]
    struct Out<'a> {
        ok: bool,
        path: &'a str,
        imported: usize,
        skipped: usize,
        dry_run: bool,
    }
    let out_struct = Out {
        ok: true,
        path: &target_path,
        imported: outcome.imported,
        skipped: outcome.skipped,
        dry_run: outcome.dry_run,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&out_struct)?);
    } else if dry_run {
        println!(
            "Dry-run: would import {} chunks to {} (skipped={})",
            outcome.imported, target_path, outcome.skipped
        );
    } else if outcome.imported == 0 {
        println!("No chunks imported (skipped={})", outcome.skipped);
    } else {
        println!(
            "Imported {} chunks to {} (skipped={})",
            outcome.imported, target_path, outcome.skipped
        );
    }

    Ok(())
}
