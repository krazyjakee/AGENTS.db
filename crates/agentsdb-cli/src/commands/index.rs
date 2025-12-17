use anyhow::Context;
use serde::Serialize;
use std::path::PathBuf;

use agentsdb_query::{
    build_layer_index, default_index_path_for_layer, IndexBuildOptions, LayerSet,
};

#[derive(Debug, Serialize)]
/// Represents a single indexed layer in the JSON output for the `index` command.
struct IndexEntryJson {
    layer: String,
    layer_path: String,
    index_path: String,
}

#[derive(Debug, Serialize)]
/// Represents the JSON output structure for the `index` command.
struct IndexJson {
    built: Vec<IndexEntryJson>,
}

pub(crate) fn cmd_index(
    layers: LayerSet,
    out_dir: Option<&str>,
    store_embeddings_f32: bool,
    json: bool,
) -> anyhow::Result<()> {
    let opened = layers.open().context("open layers")?;
    if opened.is_empty() {
        anyhow::bail!("no layers provided (use --base/--user/--delta/--local)");
    }

    let out_dir = out_dir.map(PathBuf::from);
    let mut built = Vec::new();

    for (layer_id, layer) in &opened {
        let index_path = match &out_dir {
            Some(dir) => {
                let name = layer
                    .path()
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| anyhow::anyhow!("layer path is not valid UTF-8"))?;
                dir.join(format!("{name}.agix"))
            }
            None => default_index_path_for_layer(layer.path()),
        };

        build_layer_index(
            layer,
            &index_path,
            IndexBuildOptions {
                store_embeddings_even_if_f32: store_embeddings_f32,
            },
        )
        .with_context(|| format!("build index for {:?}", layer.path()))?;

        built.push(IndexEntryJson {
            layer: format!("{layer_id:?}"),
            layer_path: layer.path().display().to_string(),
            index_path: index_path.display().to_string(),
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&IndexJson { built })?);
        return Ok(());
    }

    for e in built {
        println!(
            "OK: indexed [{layer}] {layer_path} -> {index_path}",
            layer = e.layer,
            layer_path = e.layer_path,
            index_path = e.index_path
        );
    }
    Ok(())
}
