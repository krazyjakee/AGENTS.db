use anyhow::Context;

use agentsdb_ops::{search_layers, SearchConfig};
use agentsdb_query::LayerSet;

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

    // Parse query_vec from JSON string or file if provided
    let query_vec_parsed = match (query_vec, query_vec_file) {
        (Some(v), None) => Some(parse_vec_json(&v)?),
        (None, Some(path)) => {
            let s = std::fs::read_to_string(&path).with_context(|| format!("read {path}"))?;
            Some(parse_vec_json(&s)?)
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("provide only one of --query-vec or --query-vec-file")
        }
        (None, None) => None,
    };

    // Use shared search operation
    let config = SearchConfig {
        query,
        query_vec: query_vec_parsed,
        k,
        kinds,
        use_index,
    };

    let results = search_layers(&layers, config).context("search")?;

    if json {
        // Get dimension from layers for JSON output
        let opened = layers.open().context("open layers for dimension")?;
        let query_dim = if !opened.is_empty() {
            opened[0].1.embedding_dim()
        } else {
            0
        };

        let out = SearchJson {
            query_dim,
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
