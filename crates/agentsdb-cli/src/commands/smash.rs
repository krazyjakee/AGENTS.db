use anyhow::Context;
use serde::Serialize;
use std::path::Path;
use text_splitter::{ChunkConfig, MarkdownSplitter, TextSplitter};

use agentsdb_embeddings::config::{get_immutable_embedding_options, standard_layer_paths_for_dir};
use agentsdb_format::{LayerFile, read_all_chunks, schema_of};

/// Execute the smash command: break down large chunks into smaller pieces.
/// This command is ALWAYS destructive and replaces the entire layer.
pub(crate) fn cmd_smash(
    dir: &str,
    layers_csv: &str,
    limit: usize,
    allow_base: bool,
    json: bool,
) -> anyhow::Result<()> {
    let dir_path = Path::new(dir);
    let standard_paths = standard_layer_paths_for_dir(dir_path);

    // Parse which layers to smash
    let requested_layers: Vec<&str> = layers_csv.split(',').map(|s| s.trim()).collect();

    // Check if base layer is requested
    let smash_base = requested_layers.contains(&"base");
    if smash_base && !allow_base {
        anyhow::bail!(
            "refusing to smash base layer (AGENTS.db) without --allow-base flag"
        );
    }

    // Validate requested layers
    for layer in &requested_layers {
        if !["base", "user", "delta", "local"].contains(layer) {
            anyhow::bail!(
                "invalid layer name: {:?} (valid: base, user, delta, local)",
                layer
            );
        }
    }

    // Get embedding options
    let options = get_immutable_embedding_options(dir_path)
        .context("get immutable embedding options from AGENTS.db")?;

    let mut smashed_layers = Vec::new();
    let mut total_split_count = 0usize;
    let mut total_chunk_count = 0usize;

    // Process each requested layer
    for layer_name in &requested_layers {
        let layer_path = match *layer_name {
            "base" => &standard_paths.base,
            "user" => &standard_paths.user,
            "delta" => &standard_paths.delta,
            "local" => &standard_paths.local,
            _ => unreachable!(),
        };

        // Skip if layer doesn't exist
        if !layer_path.exists() {
            if !json {
                eprintln!("Skipping {} (file does not exist)", layer_path.display());
            }
            continue;
        }

        // Check writability based on layer type
        match *layer_name {
            "base" => {
                if !allow_base {
                    // This should have been caught earlier, but double-check
                    anyhow::bail!("internal error: attempting to smash base without --allow-base");
                }
                agentsdb_format::ensure_writable_layer_path_allow_base(layer_path)
                    .with_context(|| format!("verify {} is writable", layer_path.display()))?;
            }
            "user" | "delta" | "local" => {
                agentsdb_format::ensure_writable_layer_path_allow_user(layer_path)
                    .with_context(|| format!("verify {} is writable", layer_path.display()))?;
            }
            _ => unreachable!(),
        }

        // Open the layer file
        let file = LayerFile::open(layer_path)
            .with_context(|| format!("open layer {}", layer_path.display()))?;

        let schema = schema_of(&file);

        // Read all chunks
        let chunks = read_all_chunks(&file)
            .with_context(|| format!("read chunks from {}", layer_path.display()))?;

        let embedder = options
            .clone()
            .into_embedder(schema.dim as usize)
            .context("create embedder from options")?;

        // Process chunks and split large ones
        let mut new_chunks = Vec::new();
        let mut split_count = 0;

        // Create text splitters
        let markdown_splitter = MarkdownSplitter::new(ChunkConfig::new(limit));
        let text_splitter = TextSplitter::new(ChunkConfig::new(limit));

        for chunk in chunks {
            if chunk.content.len() > limit {
                split_count += 1;

                // Split the chunk based on file type
                let splits: Vec<String> = if is_markdown(&chunk.content) {
                    markdown_splitter.chunks(&chunk.content).map(|s| s.to_string()).collect()
                } else {
                    text_splitter.chunks(&chunk.content).map(|s| s.to_string()).collect()
                };

                // Add each split as a new chunk
                for (idx, split_content) in splits.into_iter().enumerate() {
                    // Generate embeddings for the split content
                    let embeddings = embedder.embed(&[split_content.clone()])
                        .context("embed chunk content")?;
                    let embedding = embeddings.into_iter().next()
                        .ok_or_else(|| anyhow::anyhow!("embedder returned empty results"))?;

                    // Use original ID for first split, auto-assign for rest
                    let chunk_id = if idx == 0 { chunk.id } else { 0 };

                    new_chunks.push(agentsdb_format::ChunkInput {
                        id: chunk_id,
                        kind: chunk.kind.clone(),
                        content: split_content,
                        author: chunk.author.clone(),
                        confidence: chunk.confidence,
                        created_at_unix_ms: chunk.created_at_unix_ms,
                        embedding,
                        sources: chunk.sources.clone(),
                    });
                }
            } else {
                // Keep chunk as-is but still need to create ChunkInput
                let embeddings = embedder.embed(&[chunk.content.clone()])
                    .context("embed chunk content")?;
                let embedding = embeddings.into_iter().next()
                    .ok_or_else(|| anyhow::anyhow!("embedder returned empty results"))?;

                new_chunks.push(agentsdb_format::ChunkInput {
                    id: chunk.id,
                    kind: chunk.kind,
                    content: chunk.content,
                    author: chunk.author,
                    confidence: chunk.confidence,
                    created_at_unix_ms: chunk.created_at_unix_ms,
                    embedding,
                    sources: chunk.sources,
                });
            }
        }

        // Get layer metadata if it exists
        let metadata_bytes = file.layer_metadata_bytes().map(|b| b.to_vec());

        // Write the new chunks to the layer (ALWAYS replaces the entire layer)
        agentsdb_format::write_layer_atomic(
            layer_path,
            &schema,
            &mut new_chunks,
            metadata_bytes.as_deref(),
        )
        .with_context(|| format!("write layer {}", layer_path.display()))?;

        smashed_layers.push((layer_path.to_string_lossy().into_owned(), split_count, new_chunks.len()));
        total_split_count += split_count;
        total_chunk_count += new_chunks.len();

        if !json {
            println!(
                "Smashed {} large chunks into {} total chunks in {}",
                split_count,
                new_chunks.len(),
                layer_path.display()
            );
        }
    }

    if json {
        #[derive(Serialize)]
        struct LayerResult {
            layer: String,
            split_count: usize,
            total_chunks: usize,
        }
        #[derive(Serialize)]
        struct Out {
            ok: bool,
            layers: Vec<LayerResult>,
            total_split_count: usize,
            total_chunk_count: usize,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                layers: smashed_layers.into_iter().map(|(layer, split_count, total_chunks)| LayerResult {
                    layer,
                    split_count,
                    total_chunks,
                }).collect(),
                total_split_count,
                total_chunk_count,
            })?
        );
    }

    Ok(())
}

/// Check if content is markdown
fn is_markdown(content: &str) -> bool {
    // Simple heuristic: check for markdown headers
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with('#') && trimmed.len() > 1 && trimmed.chars().nth(1) == Some(' ')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_markdown() {
        assert!(is_markdown("# Heading\nSome text"));
        assert!(is_markdown("## Heading 2\nMore text"));
        assert!(!is_markdown("Just text\nNo headers"));
    }
}
