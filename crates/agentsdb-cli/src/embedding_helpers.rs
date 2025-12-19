use anyhow::Context;
use std::path::Path;

use agentsdb_embeddings::config::get_immutable_embedding_options;
use agentsdb_embeddings::embedder::Embedder;
use agentsdb_embeddings::layer_metadata::LayerMetadataV1;

/// Creates an embedder from directory options, validating dimension compatibility.
///
/// This function:
/// - Gets immutable embedding options from the directory
/// - Validates that configured dim matches expected dim (if configured)
/// - Creates and returns the embedder
///
/// # Parameters
/// - `dir`: Directory containing embedding configuration (typically parent of layer file)
/// - `expected_dim`: Expected embedding dimension
///
/// # Returns
/// The configured embedder ready for use
pub(crate) fn create_validated_embedder(
    dir: &Path,
    expected_dim: usize,
) -> anyhow::Result<Box<dyn Embedder + Send + Sync>> {
    let options =
        get_immutable_embedding_options(dir).context("get immutable embedding options")?;
    if let Some(cfg_dim) = options.dim {
        if cfg_dim != expected_dim {
            anyhow::bail!(
                "embedding dim mismatch (schema is dim={expected_dim}, options specify dim={cfg_dim})"
            );
        }
    }
    options
        .into_embedder(expected_dim)
        .context("resolve embedder from options")
}

/// Creates layer metadata with embedder profile and tool information.
///
/// Constructs a LayerMetadataV1 object with:
/// - Embedder profile (backend, model, dim, etc.)
/// - Embedder metadata (provider info, checksums, etc.)
/// - Tool name and version
///
/// # Parameters
/// - `embedder`: The embedder to extract profile/metadata from
///
/// # Returns
/// Serialized JSON bytes ready to write to layer file
pub(crate) fn create_layer_metadata(embedder: &dyn Embedder) -> anyhow::Result<Vec<u8>> {
    let layer_metadata = LayerMetadataV1::new(embedder.profile().clone())
        .with_embedder_metadata(embedder.metadata())
        .with_tool("agentsdb-cli", env!("CARGO_PKG_VERSION"));
    layer_metadata
        .to_json_bytes()
        .context("serialize layer metadata")
}

/// Validates that an embedder's profile matches existing layer metadata.
///
/// This function:
/// - Parses existing layer metadata from JSON bytes
/// - Compares embedding profiles
/// - Returns error if profiles don't match
///
/// # Parameters
/// - `existing_metadata_bytes`: The existing layer metadata as JSON bytes
/// - `embedder`: The embedder to validate against
/// - `layer_path`: Path to layer file (for error messages)
///
/// # Returns
/// Ok(()) if compatible, error with descriptive message otherwise
pub(crate) fn validate_embedder_profile(
    existing_metadata_bytes: &[u8],
    embedder: &dyn Embedder,
    layer_path: &Path,
) -> anyhow::Result<()> {
    let existing = LayerMetadataV1::from_json_bytes(existing_metadata_bytes)
        .context("parse existing layer metadata")?;
    if existing.embedding_profile != *embedder.profile() {
        anyhow::bail!(
            "embedder profile mismatch vs existing layer metadata (existing={:?}, current={:?}) for {}",
            existing.embedding_profile,
            embedder.profile(),
            layer_path.display()
        );
    }
    Ok(())
}

/// Appends chunks to a layer, handling metadata validation and conditional inclusion.
///
/// This function encapsulates the complex logic of:
/// - Checking if existing layer has metadata
/// - If yes: validating profile compatibility, appending without new metadata
/// - If no: appending with new metadata
///
/// # Parameters
/// - `layer_path`: Path to the layer file
/// - `chunks`: Chunks to append
/// - `new_metadata_bytes`: Metadata to include if layer doesn't have any
/// - `embedder`: Embedder to validate against (if existing metadata present)
///
/// # Returns
/// Vector of assigned chunk IDs
pub(crate) fn append_with_validated_metadata(
    layer_path: &Path,
    chunks: &mut [agentsdb_format::ChunkInput],
    new_metadata_bytes: &[u8],
    embedder: &dyn Embedder,
) -> anyhow::Result<Vec<u32>> {
    let file = agentsdb_format::LayerFile::open(layer_path)
        .with_context(|| format!("open existing layer {}", layer_path.display()))?;

    if let Some(existing) = file.layer_metadata_bytes() {
        validate_embedder_profile(existing, embedder, layer_path)?;
        agentsdb_format::append_layer_atomic(layer_path, chunks, None).context("append layer")
    } else {
        agentsdb_format::append_layer_atomic(layer_path, chunks, Some(new_metadata_bytes))
            .context("append layer")
    }
}

/// Validates that a layer's embedding dimension matches configured dimension.
///
/// # Parameters
/// - `layer_schema`: The layer's schema containing the embedding dimension
/// - `configured_dim`: The configured dimension from options (if any)
/// - `layer_path`: Path to layer file (for error messages)
///
/// # Returns
/// Ok(()) if dimensions match or no configured dimension, error otherwise
pub(crate) fn validate_layer_dimension(
    layer_schema: &agentsdb_format::LayerSchema,
    configured_dim: Option<usize>,
    layer_path: &Path,
) -> anyhow::Result<()> {
    if let Some(cfg_dim) = configured_dim {
        if layer_schema.dim as usize != cfg_dim {
            anyhow::bail!(
                "embedding dim mismatch for {} (layer has dim={}, options specify dim={}). \
                Cannot re-embed with different dimension.",
                layer_path.display(),
                layer_schema.dim,
                cfg_dim
            );
        }
    }
    Ok(())
}
