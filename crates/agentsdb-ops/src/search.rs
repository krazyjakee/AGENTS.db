use anyhow::Context;
use agentsdb_core::types::{SearchFilters, SearchResult};
use agentsdb_embeddings::config::roll_up_embedding_options;
use agentsdb_embeddings::layer_metadata::ensure_layer_metadata_compatible_with_embedder;
use agentsdb_query::{LayerSet, SearchOptions, SearchQuery};

/// Configuration for a search operation
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Text query to embed (mutually exclusive with query_vec)
    pub query: Option<String>,
    /// Pre-computed embedding vector (mutually exclusive with query)
    pub query_vec: Option<Vec<f32>>,
    /// Number of results to return
    pub k: usize,
    /// Filter by chunk kinds (empty = no filter)
    pub kinds: Vec<String>,
    /// Whether to use ANN index if available
    pub use_index: bool,
}

/// Perform a search across opened layers
///
/// This function:
/// 1. Opens and validates layers
/// 2. Rolls up embedding options from layer hierarchy
/// 3. Creates/resolves embedder
/// 4. Embeds query if needed (or uses provided vector)
/// 5. Validates layer metadata vs embedder
/// 6. Executes search via agentsdb_query
/// 7. Returns ranked results
pub fn search_layers(
    layers: &LayerSet,
    config: SearchConfig,
) -> anyhow::Result<Vec<SearchResult>> {
    // Validate input
    match (&config.query, &config.query_vec) {
        (Some(_), Some(_)) => {
            anyhow::bail!("provide only one of query or query_vec, not both")
        }
        (None, None) => anyhow::bail!("missing query (provide either query or query_vec)"),
        _ => {}
    }

    // Open layers
    let opened = layers.open().context("open layers")?;
    if opened.is_empty() {
        anyhow::bail!("no layers provided");
    }

    // Get dimension from first layer
    let dim = opened[0].1.embedding_dim();

    // Separate layers by type for options roll-up
    let mut local = None;
    let mut user = None;
    let mut delta = None;
    let mut base = None;
    for (layer_id, file) in &opened {
        match layer_id {
            agentsdb_core::types::LayerId::Local => local = Some(file),
            agentsdb_core::types::LayerId::User => user = Some(file),
            agentsdb_core::types::LayerId::Delta => delta = Some(file),
            agentsdb_core::types::LayerId::Base => base = Some(file),
        }
    }

    // Roll up embedding options from layer hierarchy
    let options =
        roll_up_embedding_options(&[local, user, delta, base]).context("roll up options")?;

    // Validate configured dimension matches layer dimension
    if let Some(cfg_dim) = options.dim {
        if cfg_dim != dim {
            anyhow::bail!(
                "embedding dim mismatch (layers are dim={dim}, options specify dim={cfg_dim})"
            );
        }
    }

    // Create embedder from options
    let embedder = options
        .into_embedder(dim)
        .context("resolve embedder from options")?;

    // Get embedding vector
    let embedding = match (&config.query, &config.query_vec) {
        (Some(q), None) => {
            // Embed the query text
            if q.trim().is_empty() {
                anyhow::bail!("query must be non-empty");
            }

            // Validate layer metadata is compatible with embedder
            for (layer_id, file) in &opened {
                if let Err(e) = ensure_layer_metadata_compatible_with_embedder(file, embedder.as_ref()) {
                    anyhow::bail!(
                        "Layer {:?} embedding configuration is incompatible with the configured embedder: {}. \
                        This may happen if the layer was created with different embedding settings. \
                        Try using a pre-computed query vector (--query-vec) instead.",
                        layer_id,
                        e
                    );
                }
            }

            // Embed the query
            let out = embedder.embed(&[q.clone()])?;
            out.into_iter().next().unwrap_or_else(|| vec![0.0; dim])
        }
        (None, Some(vec)) => {
            // Use pre-computed vector
            if vec.len() != dim {
                anyhow::bail!(
                    "query_vec dimension mismatch (expected {}, got {})",
                    dim,
                    vec.len()
                );
            }
            vec.clone()
        }
        _ => unreachable!("validated earlier"),
    };

    // Build search query
    let query = SearchQuery {
        embedding,
        k: config.k,
        filters: SearchFilters {
            kinds: config.kinds,
        },
    };

    // Execute search
    let results = agentsdb_query::search_layers_with_options(
        &opened,
        &query,
        SearchOptions {
            use_index: config.use_index,
        },
    )
    .context("search")?;

    Ok(results)
}

/// Embed a query string using the layer set's embedding configuration
///
/// This is a helper function that just returns the embedding vector
/// without performing a search.
pub fn embed_query(layers: &LayerSet, query: &str) -> anyhow::Result<Vec<f32>> {
    if query.trim().is_empty() {
        anyhow::bail!("query must be non-empty");
    }

    // Open layers
    let opened = layers.open().context("open layers")?;
    if opened.is_empty() {
        anyhow::bail!("no layers provided");
    }

    // Get dimension from first layer
    let dim = opened[0].1.embedding_dim();

    // Separate layers by type for options roll-up
    let mut local = None;
    let mut user = None;
    let mut delta = None;
    let mut base = None;
    for (layer_id, file) in &opened {
        match layer_id {
            agentsdb_core::types::LayerId::Local => local = Some(file),
            agentsdb_core::types::LayerId::User => user = Some(file),
            agentsdb_core::types::LayerId::Delta => delta = Some(file),
            agentsdb_core::types::LayerId::Base => base = Some(file),
        }
    }

    // Roll up embedding options
    let options =
        roll_up_embedding_options(&[local, user, delta, base]).context("roll up options")?;

    // Validate configured dimension
    if let Some(cfg_dim) = options.dim {
        if cfg_dim != dim {
            anyhow::bail!(
                "embedding dim mismatch (layers are dim={dim}, options specify dim={cfg_dim})"
            );
        }
    }

    // Create embedder
    let embedder = options
        .into_embedder(dim)
        .context("resolve embedder from options")?;

    // Validate layer metadata
    for (layer_id, file) in &opened {
        if let Err(e) = ensure_layer_metadata_compatible_with_embedder(file, embedder.as_ref()) {
            anyhow::bail!(
                "Layer {:?} embedding configuration is incompatible: {}",
                layer_id,
                e
            );
        }
    }

    // Embed the query
    let out = embedder.embed(&[query.to_string()])?;
    Ok(out.into_iter().next().unwrap_or_else(|| vec![0.0; dim]))
}
