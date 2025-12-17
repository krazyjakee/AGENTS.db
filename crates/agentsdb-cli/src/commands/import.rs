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
    target: Option<&str>,
    out: Option<&str>,
    dry_run: bool,
    dedupe: bool,
    preserve_ids: bool,
    allow_base: bool,
    dim: Option<u32>,
    json: bool,
) -> anyhow::Result<()> {
    // Read input file
    let bytes = std::fs::read(input).with_context(|| format!("read {}", input))?;
    let data = std::str::from_utf8(&bytes).context("input must be valid UTF-8")?;

    if let Some(target) = target {
        let target_path = resolve_target_path(dir, target, out)?;

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

        return Ok(());
    }

    if out.is_some() {
        anyhow::bail!("--out is only valid when --target is provided");
    }

    let results = agentsdb_ops::import::import_export_bundle_into_dir(
        std::path::Path::new(dir),
        &bytes,
        dry_run,
        dedupe,
        preserve_ids,
        allow_base,
        dim,
        "agentsdb-cli",
        env!("CARGO_PKG_VERSION"),
    )?;
    if results.is_empty() {
        anyhow::bail!("no layers found in import");
    }

    let total_imported: usize = results.iter().map(|(_, o)| o.imported).sum();
    let total_skipped: usize = results.iter().map(|(_, o)| o.skipped).sum();

    #[derive(Serialize)]
    struct LayerOut<'a> {
        path: &'a str,
        imported: usize,
        skipped: usize,
        dry_run: bool,
    }
    #[derive(Serialize)]
    struct OutAll<'a> {
        ok: bool,
        dir: &'a str,
        imported: usize,
        skipped: usize,
        dry_run: bool,
        layers: Vec<LayerOut<'a>>,
    }

    if json {
        let layers = results
            .iter()
            .map(|(p, o)| LayerOut {
                path: p.as_str(),
                imported: o.imported,
                skipped: o.skipped,
                dry_run: o.dry_run,
            })
            .collect();
        let out_struct = OutAll {
            ok: true,
            dir,
            imported: total_imported,
            skipped: total_skipped,
            dry_run,
            layers,
        };
        println!("{}", serde_json::to_string_pretty(&out_struct)?);
    } else if dry_run {
        println!(
            "Dry-run: would import {} chunks across {} layers (skipped={})",
            total_imported,
            results.len(),
            total_skipped
        );
    } else if total_imported == 0 {
        println!("No chunks imported (skipped={})", total_skipped);
    } else {
        println!(
            "Imported {} chunks across {} layers (skipped={})",
            total_imported,
            results.len(),
            total_skipped
        );
    }

    Ok(())
}
