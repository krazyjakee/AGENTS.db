use anyhow::Context;
use std::io::Write;

use agentsdb_embeddings::config::standard_layer_paths_for_dir;

fn parse_layers_csv(s: &str) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    for raw in s.split(',') {
        let v = raw.trim();
        if v.is_empty() {
            continue;
        }
        match v {
            "base" | "user" | "delta" | "local" => out.push(v.to_string()),
            _ => anyhow::bail!("invalid layer {v:?} (expected base,user,delta,local)"),
        }
    }
    if out.is_empty() {
        anyhow::bail!("--layers must include at least one of base,user,delta,local");
    }
    Ok(out)
}

pub(crate) fn cmd_export(
    dir: &str,
    format: &str,
    layers_csv: &str,
    out_path: Option<&str>,
    redact: &str,
    json: bool,
) -> anyhow::Result<()> {
    if json {
        anyhow::bail!("--json is not supported for export (export output is already JSON/NDJSON)");
    }

    let layers = parse_layers_csv(layers_csv)?;
    let siblings = standard_layer_paths_for_dir(std::path::Path::new(dir));

    // Build list of paths to export
    let mut paths_to_export = Vec::new();
    for layer in layers {
        let path = match layer.as_str() {
            "base" => siblings.base.clone(),
            "user" => siblings.user.clone(),
            "delta" => siblings.delta.clone(),
            "local" => siblings.local.clone(),
            _ => continue,
        };
        if path.exists() {
            paths_to_export.push(path);
        }
    }

    // Build list of (abs_path, rel_path, logical_layer) tuples with proper lifetimes
    let layers_and_paths: Vec<_> = paths_to_export
        .iter()
        .map(|path| {
            let rel_path = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_else(|| path.to_str().unwrap_or("unknown"));
            let logical = agentsdb_ops::util::logical_layer_for_path(rel_path);
            (path.as_path(), rel_path, logical)
        })
        .collect();

    // Use shared export operation
    let (_content_type, body) = agentsdb_ops::export::export_layers(
        layers_and_paths,
        format,
        redact,
        "agentsdb-cli",
        env!("CARGO_PKG_VERSION"),
    )?;

    // Write output
    let mut out: Box<dyn std::io::Write> = match out_path {
        Some(p) => Box::new(std::fs::File::create(p).with_context(|| format!("create {}", p))?),
        None => Box::new(std::io::stdout()),
    };
    out.write_all(&body)?;

    Ok(())
}
