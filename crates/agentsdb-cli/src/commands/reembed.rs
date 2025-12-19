use anyhow::Context;
use serde::Serialize;
use std::path::Path;

use agentsdb_embeddings::config::{get_immutable_embedding_options, standard_layer_paths_for_dir};

use crate::embedding_helpers::validate_layer_dimension;

pub(crate) fn cmd_reembed(
    dir: &str,
    layers_csv: &str,
    allow_base: bool,
    json: bool,
) -> anyhow::Result<()> {
    let dir_path = Path::new(dir);
    let standard_paths = standard_layer_paths_for_dir(dir_path);

    // Parse which layers to re-embed
    let requested_layers: Vec<&str> = layers_csv.split(',').map(|s| s.trim()).collect();

    // Check if base layer is requested
    let reembed_base = requested_layers.contains(&"base");
    if reembed_base && !allow_base {
        anyhow::bail!(
            "refusing to re-embed base layer (AGENTS.db) without --allow-base flag. \
            This operation is potentially destructive and may cause inconsistencies."
        );
    }

    // Validate requested layers
    for layer in &requested_layers {
        if !["base", "user", "delta", "local"].contains(layer) {
            anyhow::bail!(
                "invalid layer name: {layer:?} (valid: base, user, delta, local)"
            );
        }
    }

    // Get embedding options from AGENTS.db
    let options = get_immutable_embedding_options(dir_path)
        .context("get immutable embedding options from AGENTS.db")?;

    let embedder = options
        .clone()
        .into_embedder(options.dim.unwrap_or(128))
        .context("create embedder from options")?;

    let mut reembedded_layers = Vec::new();
    let mut total_chunks = 0usize;

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
                    anyhow::bail!("internal error: attempting to re-embed base without --allow-base");
                }
                agentsdb_format::ensure_writable_layer_path_allow_base(layer_path)
                    .with_context(|| format!("verify {} is writable", layer_path.display()))?;
            }
            "user" | "delta" | "local" => {
                // User layer requires allow_user flag, delta/local are always writable
                agentsdb_format::ensure_writable_layer_path_allow_user(layer_path)
                    .with_context(|| format!("verify {} is writable", layer_path.display()))?;
            }
            _ => unreachable!(),
        }

        // Open the layer file
        let file = agentsdb_format::LayerFile::open(layer_path)
            .with_context(|| format!("open layer {}", layer_path.display()))?;

        let schema = agentsdb_format::schema_of(&file);

        // Read all chunks
        let mut chunks = agentsdb_format::read_all_chunks(&file)
            .with_context(|| format!("read chunks from {}", layer_path.display()))?;

        if chunks.is_empty() {
            if !json {
                eprintln!("Skipping {} (no chunks to re-embed)", layer_path.display());
            }
            continue;
        }

        // Check embedding dimension matches
        validate_layer_dimension(&schema, options.dim, layer_path)?;

        // Prepare content to embed
        let to_embed: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();

        if !json {
            println!(
                "Re-embedding {} chunks in {} using backend={}...",
                to_embed.len(),
                layer_path.display(),
                options.backend
            );
        }

        // Generate new embeddings
        let embeddings = embedder
            .embed(&to_embed)
            .with_context(|| format!("embed chunks for {}", layer_path.display()))?;

        if embeddings.len() != chunks.len() {
            anyhow::bail!(
                "embedder returned {} embeddings for {} chunks",
                embeddings.len(),
                chunks.len()
            );
        }

        // Update chunks with new embeddings
        for (chunk, embedding) in chunks.iter_mut().zip(embeddings.into_iter()) {
            if embedding.len() != schema.dim as usize {
                anyhow::bail!(
                    "embedder returned embedding of dim={} but expected dim={}",
                    embedding.len(),
                    schema.dim
                );
            }
            chunk.embedding = embedding;
        }

        // Preserve existing layer metadata if present
        let layer_metadata = file.layer_metadata_bytes().map(|b| b.to_vec());

        // Write back to the layer file atomically
        agentsdb_format::write_layer_atomic(
            layer_path,
            &schema,
            &mut chunks,
            layer_metadata.as_deref(),
        )
        .with_context(|| format!("write re-embedded layer {}", layer_path.display()))?;

        reembedded_layers.push(layer_path.to_string_lossy().into_owned());
        total_chunks += chunks.len();
    }

    if json {
        #[derive(Serialize)]
        struct Out {
            ok: bool,
            reembedded_layers: Vec<String>,
            total_chunks: usize,
            backend: String,
            model: Option<String>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                ok: true,
                reembedded_layers,
                total_chunks,
                backend: options.backend.clone(),
                model: options.model.clone(),
            })?
        );
    } else {
        if reembedded_layers.is_empty() {
            println!("No layers were re-embedded");
        } else {
            println!(
                "Successfully re-embedded {} chunks across {} layer(s) using backend={}",
                total_chunks,
                reembedded_layers.len(),
                options.backend
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> agentsdb_format::LayerSchema {
        agentsdb_format::LayerSchema {
            dim: 4,
            element_type: agentsdb_format::EmbeddingElementType::F32,
            quant_scale: 1.0,
        }
    }

    fn chunk(id: u32, kind: &str, content: &str) -> agentsdb_format::ChunkInput {
        agentsdb_format::ChunkInput {
            id,
            kind: kind.to_string(),
            content: content.to_string(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: vec![0.1, 0.2, 0.3, 0.4],
            sources: Vec::new(),
        }
    }

    #[test]
    fn reembed_updates_embeddings() {
        let dir = crate::util::make_temp_dir();
        let base_path = dir.join("AGENTS.db");
        let user_path = dir.join("AGENTS.user.db");

        // Create base layer with hash embeddings configured for dim=4
        let options_record = agentsdb_embeddings::config::OptionsRecord {
            embedding: Some(agentsdb_embeddings::config::EmbeddingOptionsPatch {
                backend: Some("hash".to_string()),
                dim: Some(4),
                ..Default::default()
            }),
            checksum_allowlist: None,
        };
        let options_chunk = agentsdb_format::ChunkInput {
            id: 1000,
            kind: agentsdb_embeddings::config::KIND_OPTIONS.to_string(),
            content: serde_json::to_string(&options_record).unwrap(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: vec![0.0; 4],
            sources: Vec::new(),
        };
        let mut base_chunks = [
            options_chunk,
            chunk(1, "canonical", "hello world"),
            chunk(2, "canonical", "test content"),
        ];
        agentsdb_format::write_layer_atomic(&base_path, &schema(), &mut base_chunks, None)
            .unwrap();

        // Create user layer with some chunks
        let mut user_chunks = [
            chunk(100, "note", "user note"),
            chunk(101, "note", "another note"),
        ];
        agentsdb_format::write_layer_atomic(&user_path, &schema(), &mut user_chunks, None)
            .unwrap();

        // Store original embeddings
        let user_file_before = agentsdb_format::LayerFile::open(&user_path).unwrap();
        let user_chunks_before = agentsdb_format::read_all_chunks(&user_file_before).unwrap();
        let original_embedding = user_chunks_before[0].embedding.clone();

        // Re-embed user layer only
        let dir_str = dir.to_string_lossy();
        cmd_reembed(&dir_str, "user", false, false).unwrap();

        // Read back and verify embeddings changed
        let user_file_after = agentsdb_format::LayerFile::open(&user_path).unwrap();
        let user_chunks_after = agentsdb_format::read_all_chunks(&user_file_after).unwrap();

        assert_eq!(user_chunks_after.len(), 2);
        // With hash embeddings, the embedding should be deterministic but different from original
        // (original was [0.1, 0.2, 0.3, 0.4], hash will generate different values)
        assert_ne!(user_chunks_after[0].embedding, original_embedding);

        // Verify base layer was not modified
        let base_file = agentsdb_format::LayerFile::open(&base_path).unwrap();
        let base_chunks_after = agentsdb_format::read_all_chunks(&base_file).unwrap();
        assert_eq!(base_chunks_after.len(), 3); // options chunk + 2 content chunks
        // Check that the first content chunk (index 1) was not modified
        assert_eq!(base_chunks_after[1].embedding, base_chunks[1].embedding);
    }

    #[test]
    fn reembed_refuses_base_without_flag() {
        let dir = crate::util::make_temp_dir();
        let base_path = dir.join("AGENTS.db");

        // Create base layer with hash embeddings configured for dim=4
        let options_record = agentsdb_embeddings::config::OptionsRecord {
            embedding: Some(agentsdb_embeddings::config::EmbeddingOptionsPatch {
                backend: Some("hash".to_string()),
                dim: Some(4),
                ..Default::default()
            }),
            checksum_allowlist: None,
        };
        let options_chunk = agentsdb_format::ChunkInput {
            id: 1000,
            kind: agentsdb_embeddings::config::KIND_OPTIONS.to_string(),
            content: serde_json::to_string(&options_record).unwrap(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: vec![0.0; 4],
            sources: Vec::new(),
        };
        let mut base_chunks = [options_chunk, chunk(1, "canonical", "content")];
        agentsdb_format::write_layer_atomic(&base_path, &schema(), &mut base_chunks, None)
            .unwrap();

        let dir_str = dir.to_string_lossy();
        let result = cmd_reembed(&dir_str, "base", false, false);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--allow-base"));
    }

    #[test]
    fn reembed_allows_base_with_flag() {
        let dir = crate::util::make_temp_dir();
        let base_path = dir.join("AGENTS.db");

        // Create base layer with hash embeddings configured for dim=4
        let options_record = agentsdb_embeddings::config::OptionsRecord {
            embedding: Some(agentsdb_embeddings::config::EmbeddingOptionsPatch {
                backend: Some("hash".to_string()),
                dim: Some(4),
                ..Default::default()
            }),
            checksum_allowlist: None,
        };
        let options_chunk = agentsdb_format::ChunkInput {
            id: 1000,
            kind: agentsdb_embeddings::config::KIND_OPTIONS.to_string(),
            content: serde_json::to_string(&options_record).unwrap(),
            author: "human".to_string(),
            confidence: 1.0,
            created_at_unix_ms: 0,
            embedding: vec![0.0; 4],
            sources: Vec::new(),
        };
        let mut base_chunks = [options_chunk, chunk(1, "canonical", "content")];
        agentsdb_format::write_layer_atomic(&base_path, &schema(), &mut base_chunks, None)
            .unwrap();

        let dir_str = dir.to_string_lossy();
        let result = cmd_reembed(&dir_str, "base", true, false);

        assert!(result.is_ok());
    }
}
