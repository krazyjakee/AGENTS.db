use anyhow::Context;

use agentsdb_core::types::SearchFilters;
use agentsdb_embeddings::config::roll_up_embedding_options;
use agentsdb_embeddings::layer_metadata::ensure_layer_metadata_compatible_with_embedder;
use agentsdb_query::{LayerSet, SearchOptions, SearchQuery};

use crate::types::{SearchJson, SearchResultJson};
use crate::util::{layer_to_str, one_line, parse_vec_json, source_to_string};

pub(crate) fn cmd_search(
    layers: LayerSet,
    query: Option<String>,
    query_vec: Option<String>,
    query_vec_file: Option<String>,
    k: usize,
    kinds: Vec<String>,
    use_index: bool,
    json: bool,
) -> anyhow::Result<()> {
    // Implements the `search` command, which searches one or more layers using vector similarity.
    //
    // This function handles parsing query input (text, vector, or vector file), embedding the query,
    // and performing the search across specified layers with optional filtering and index usage.
    let opened = layers.open().context("open layers")?;
    if opened.is_empty() {
        anyhow::bail!("no layers provided (use --base/--user/--delta/--local)");
    }

    let dim = opened[0].1.embedding_dim();
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
    let options =
        roll_up_embedding_options(&[local, user, delta, base]).context("roll up options")?;
    if let Some(cfg_dim) = options.dim {
        if cfg_dim != dim {
            anyhow::bail!(
                "embedding dim mismatch (layers are dim={dim}, options specify dim={cfg_dim})"
            );
        }
    }
    let embedder = options
        .into_embedder(dim)
        .context("resolve embedder from options")?;
    let embedding = match (query, query_vec, query_vec_file) {
        (Some(q), None, None) => {
            if q.trim().is_empty() {
                anyhow::bail!("--query must be non-empty");
            }
            for (_, file) in &opened {
                ensure_layer_metadata_compatible_with_embedder(file, embedder.as_ref())
                    .context("validate layer metadata vs embedder")?;
            }
            let out = embedder.embed(&[q])?;
            out.into_iter().next().unwrap_or_else(|| vec![0.0; dim])
        }
        (None, Some(v), None) => parse_vec_json(&v)?,
        (None, None, Some(path)) => {
            let s = std::fs::read_to_string(&path).with_context(|| format!("read {path}"))?;
            parse_vec_json(&s)?
        }
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (None, Some(_), Some(_)) => {
            anyhow::bail!("provide only one of --query, --query-vec, or --query-vec-file")
        }
        (None, None, None) => {
            anyhow::bail!("missing query (use --query or --query-vec/--query-vec-file)")
        }
    };

    let query = SearchQuery {
        embedding: embedding.clone(),
        k,
        filters: SearchFilters { kinds },
    };

    let results =
        agentsdb_query::search_layers_with_options(&opened, &query, SearchOptions { use_index })
            .context("search")?;

    if json {
        let out = SearchJson {
            query_dim: embedding.len(),
            k,
            results: results.into_iter().map(to_search_json).collect(),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    for r in results {
        println!(
            "[{:?}] id={} score={:.6} kind={} author={:?} conf={:.3}",
            r.layer,
            r.chunk.id.get(),
            r.score,
            r.chunk.kind,
            r.chunk.author,
            r.chunk.confidence
        );
        if !r.hidden_layers.is_empty() {
            println!("  hidden_layers={:?}", r.hidden_layers);
        }
        println!("  {}", one_line(&r.chunk.content));
    }
    Ok(())
}

fn to_search_json(r: agentsdb_core::types::SearchResult) -> SearchResultJson {
    SearchResultJson {
        layer: layer_to_str(r.layer).to_string(),
        id: r.chunk.id.get(),
        kind: r.chunk.kind,
        score: r.score,
        author: format!("{:?}", r.chunk.author),
        confidence: r.chunk.confidence,
        created_at_unix_ms: r.chunk.created_at_unix_ms,
        sources: r.chunk.sources.into_iter().map(source_to_string).collect(),
        hidden_layers: r
            .hidden_layers
            .into_iter()
            .map(|l| layer_to_str(l).to_string())
            .collect(),
        content: r.chunk.content,
    }
}
