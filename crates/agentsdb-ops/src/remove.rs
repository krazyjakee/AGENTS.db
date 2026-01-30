use anyhow::Context;
use std::path::Path;

/// Remove a chunk from a layer by rewriting the file without it
///
/// # Arguments
/// * `path` - Path to the layer file
/// * `id` - Chunk ID to remove
///
/// # Returns
/// Ok(true) if chunk was found and removed, Ok(false) if chunk was not found
pub fn remove_chunk(path: &Path, id: u32) -> anyhow::Result<bool> {
    agentsdb_format::ensure_writable_layer_path_allow_user(path.to_str().unwrap_or_default())
        .context("permission check")?;

    // Open the file
    let file = agentsdb_format::LayerFile::open_lenient(path)
        .with_context(|| format!("open {}", path.display()))?;

    let schema = agentsdb_format::schema_of(&file);
    let all_chunks = agentsdb_format::read_all_chunks(&file)
        .with_context(|| format!("read chunks from {}", path.display()))?;

    // Check if chunk exists
    let chunk_exists = all_chunks.iter().any(|c| c.id == id);
    if !chunk_exists {
        return Ok(false);
    }

    // Filter out the chunk to remove
    let mut filtered_chunks: Vec<agentsdb_format::ChunkInput> = all_chunks
        .into_iter()
        .filter(|c| c.id != id)
        .collect();

    // Rewrite the file atomically without the removed chunk
    agentsdb_format::write_layer_atomic(path, &schema, &mut filtered_chunks, None)
        .with_context(|| format!("rewrite {}", path.display()))?;

    Ok(true)
}
