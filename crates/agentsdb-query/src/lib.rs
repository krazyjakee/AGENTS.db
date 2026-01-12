use agentsdb_core::error::{Error, FormatError, SchemaError};
use agentsdb_core::types::{
    Author, Chunk, ChunkId, LayerId, ProvenanceRef, SearchFilters, SearchResult,
};
use agentsdb_embeddings::config::KIND_OPTIONS;
use agentsdb_format::{LayerFile, SourceRef};
use std::collections::{HashMap, HashSet};

mod index;
pub use index::{build_layer_index, default_index_path_for_layer, IndexBuildOptions, IndexLookup};

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub embedding: Vec<f32>,
    pub k: usize,
    pub filters: SearchFilters,
    /// Optional raw query text for lexical search
    pub query_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Pure semantic search (vector similarity only)
    Semantic,
    /// Hybrid search with lexical filtering + semantic ranking
    Hybrid,
}

impl Default for SearchMode {
    fn default() -> Self {
        Self::Hybrid
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    /// When enabled, search may use a sidecar index (if present and not stale) to accelerate exact search.
    pub use_index: bool,
    /// Search mode: semantic only or hybrid (lexical + semantic)
    pub mode: SearchMode,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            use_index: false,
            mode: SearchMode::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayerSet {
    pub base: Option<String>,
    pub user: Option<String>,
    pub delta: Option<String>,
    pub local: Option<String>,
}

impl LayerSet {
    pub fn open(&self) -> Result<Vec<(LayerId, LayerFile)>, Error> {
        let mut layers = Vec::new();
        for (layer_id, path) in [
            (LayerId::Local, &self.local),
            (LayerId::User, &self.user),
            (LayerId::Delta, &self.delta),
            (LayerId::Base, &self.base),
        ] {
            if let Some(path) = path {
                layers.push((layer_id, LayerFile::open(path)?));
            }
        }
        validate_schema_compatible(&layers)?;
        Ok(layers)
    }
}

pub fn search_layers(
    layers: &[(LayerId, LayerFile)],
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, Error> {
    search_layers_with_options(layers, query, SearchOptions::default())
}

pub fn search_layers_with_options(
    layers: &[(LayerId, LayerFile)],
    query: &SearchQuery,
    options: SearchOptions,
) -> Result<Vec<SearchResult>, Error> {
    if query.k == 0 {
        return Err(FormatError::InvalidValue {
            field: "k",
            reason: "must be positive",
        }
        .into());
    }
    if layers.is_empty() {
        return Ok(Vec::new());
    }

    let dim = layers[0].1.embedding_dim();
    if query.embedding.len() != dim {
        return Err(SchemaError::Mismatch("query embedding dimension mismatch").into());
    }

    // Precompute which chunk IDs are selected (local > user > delta > base), accounting for
    // append-only updates within a layer.
    // In hybrid mode with query_text, lexical tier comparison allows better matches from
    // lower-precedence layers to surface.
    let selection = compute_selection(layers, query.query_text.as_deref())?;

    let kind_filter: Option<HashSet<&str>> = if query.filters.kinds.is_empty() {
        None
    } else {
        Some(query.filters.kinds.iter().map(|s| s.as_str()).collect())
    };

    let query_norm = l2_norm(&query.embedding);
    let mut tmp = vec![0.0f32; dim];
    let mut hits: Vec<(SearchResult, u32)> = Vec::new(); // (result, priority_tier)

    let layers_by_id: HashMap<LayerId, &LayerFile> =
        layers.iter().map(|(id, f)| (*id, f)).collect();

    let index_lookup = if options.use_index {
        IndexLookup::open_for_layers(layers)?
    } else {
        IndexLookup::empty()
    };

    let use_hybrid = options.mode == SearchMode::Hybrid && query.query_text.is_some();

    for (chunk_id, selected) in selection.selected.iter() {
        let layer = layers_by_id
            .get(&selected.layer)
            .ok_or(SchemaError::Mismatch(
                "selected layer missing from layer set",
            ))?;
        let chunk = selected.chunk;

        if let Some(kinds) = &kind_filter {
            if !kinds.contains(chunk.kind) {
                continue;
            }
        } else if chunk.kind == KIND_OPTIONS || chunk.kind.starts_with("meta.") {
            continue;
        }

        // Compute semantic similarity score
        let semantic_score = if let Some(index) = index_lookup.index_for(selected.layer) {
            let (row_norm, row_opt) = index.row_f32_and_norm(chunk.embedding_row)?;
            match row_opt {
                Some(row) => {
                    cosine_similarity_row_norm(&query.embedding, query_norm, row, row_norm)
                }
                None => {
                    layer.read_embedding_row_f32(chunk.embedding_row, &mut tmp)?;
                    cosine_similarity_row_norm(&query.embedding, query_norm, &tmp, row_norm)
                }
            }
        } else {
            layer.read_embedding_row_f32(chunk.embedding_row, &mut tmp)?;
            cosine_similarity(&query.embedding, query_norm, &tmp)
        };

        let sources = layer
            .sources_for(chunk.rel_start, chunk.rel_count)?
            .into_iter()
            .map(|s| match s {
                SourceRef::ChunkId(id) => ProvenanceRef::ChunkId(ChunkId(id)),
                SourceRef::String(v) => ProvenanceRef::SourceString(v.to_string()),
            })
            .collect();

        let out_chunk = Chunk {
            id: ChunkId(chunk.id),
            kind: chunk.kind.to_string(),
            content: chunk.content.to_string(),
            author: match chunk.author {
                "human" => Author::Human,
                "mcp" => Author::Mcp,
                _other => {
                    return Err(FormatError::InvalidValue {
                        field: "ChunkRecord.author_str_id",
                        reason: "must resolve to 'human' or 'mcp'",
                    }
                    .into());
                }
            },
            confidence: chunk.confidence,
            created_at_unix_ms: chunk.created_at_unix_ms,
            sources,
        };

        // Compute final score based on mode
        let (final_score, priority_tier) = if use_hybrid {
            if let Some(ref query_text) = query.query_text {
                let lexical_match = compute_lexical_match(query_text, &out_chunk.content);
                let (tier, score) = compute_hybrid_score(lexical_match, semantic_score);
                (score, tier)
            } else {
                (semantic_score, 6) // Fallback to pure semantic
            }
        } else {
            (semantic_score, 6) // Pure semantic mode
        };

        hits.push((
            SearchResult {
                layer: selected.layer,
                score: final_score,
                chunk: out_chunk,
                hidden_layers: selection
                    .hidden_by
                    .get(chunk_id)
                    .cloned()
                    .unwrap_or_default(),
            },
            priority_tier,
        ));
    }

    // Sort by priority tier first, then by score within tier
    hits.sort_by(|a, b| {
        a.1.cmp(&b.1) // Priority tier (lower is better)
            .then_with(|| {
                score_for_sort(b.0.score)
                    .total_cmp(&score_for_sort(a.0.score))
            })
            .then_with(|| a.0.chunk.id.cmp(&b.0.chunk.id))
            .then_with(|| a.0.layer.cmp(&b.0.layer))
    });

    // Extract results and truncate
    let results: Vec<SearchResult> = hits.into_iter().map(|(r, _)| r).take(query.k).collect();
    Ok(results)
}

fn validate_schema_compatible(layers: &[(LayerId, LayerFile)]) -> Result<(), Error> {
    if layers.len() <= 1 {
        return Ok(());
    }
    let first = &layers[0].1.embedding_matrix;
    for (_, layer) in &layers[1..] {
        let m = &layer.embedding_matrix;
        if m.dim != first.dim {
            return Err(SchemaError::Mismatch("embedding dim mismatch").into());
        }
        if m.element_type != first.element_type {
            return Err(SchemaError::Mismatch("embedding element type mismatch").into());
        }
        if m.quant_scale.to_bits() != first.quant_scale.to_bits() {
            return Err(SchemaError::Mismatch("embedding quant_scale mismatch").into());
        }
    }
    Ok(())
}

struct Selection<'a> {
    selected: HashMap<ChunkId, SelectedChunk<'a>>,
    hidden_by: HashMap<ChunkId, Vec<LayerId>>,
}

struct SelectedChunk<'a> {
    layer: LayerId,
    chunk: agentsdb_format::ChunkView<'a>,
    lexical_tier: u32,
}

/// Get lexical tier for a chunk (lower = better match)
/// Returns 6 if query_text is None (semantic-only mode)
fn get_lexical_tier(query_text: Option<&str>, content: &str) -> u32 {
    match query_text {
        Some(text) => {
            let lexical_match = compute_lexical_match(text, content);
            let (tier, _) = compute_hybrid_score(lexical_match, 0.0);
            tier
        }
        None => 6, // No query text = semantic-only = use layer precedence
    }
}

fn compute_selection<'a>(
    layers: &'a [(LayerId, LayerFile)],
    query_text: Option<&str>,
) -> Result<Selection<'a>, Error> {
    let mut selected: HashMap<ChunkId, SelectedChunk<'_>> = HashMap::new();
    let mut hidden_by: HashMap<ChunkId, Vec<LayerId>> = HashMap::new();

    for (layer_id, layer) in layers {
        let mut last_by_id: HashMap<ChunkId, agentsdb_format::ChunkView<'_>> = HashMap::new();

        for chunk_res in layer.chunks() {
            let chunk = chunk_res?;
            last_by_id.insert(ChunkId(chunk.id), chunk);
        }

        for (id, chunk) in last_by_id {
            // Read chunk content for lexical tier computation
            let content = chunk.content.to_string();
            let new_tier = get_lexical_tier(query_text, &content);

            if let Some(existing) = selected.get(&id) {
                // Chunk ID already exists in higher-priority layer
                // Compare lexical tiers
                let existing_tier = existing.lexical_tier;

                if new_tier < existing_tier {
                    // This version has BETTER lexical match - replace it
                    // Mark the old layer as hidden instead
                    hidden_by.entry(id).or_default().push(existing.layer);
                    selected.insert(
                        id,
                        SelectedChunk {
                            layer: *layer_id,
                            chunk,
                            lexical_tier: new_tier,
                        },
                    );
                } else {
                    // Keep existing version (better tier or same tier with higher layer precedence)
                    hidden_by.entry(id).or_default().push(*layer_id);
                }
                continue;
            }

            // First occurrence - just add it
            selected.insert(
                id,
                SelectedChunk {
                    layer: *layer_id,
                    chunk,
                    lexical_tier: new_tier,
                },
            );
        }
    }

    Ok(Selection {
        selected,
        hidden_by,
    })
}

fn score_for_sort(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        f32::NEG_INFINITY
    }
}

fn l2_norm(v: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for x in v {
        sum += x * x;
    }
    sum.sqrt()
}

fn cosine_similarity(query: &[f32], query_norm: f32, row: &[f32]) -> f32 {
    if query_norm == 0.0 || row.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut sum = 0.0f32;
    for (a, b) in query.iter().zip(row.iter()) {
        dot += a * b;
        sum += b * b;
    }
    let row_norm = sum.sqrt();
    if row_norm == 0.0 {
        0.0
    } else {
        dot / (query_norm * row_norm)
    }
}

fn cosine_similarity_row_norm(query: &[f32], query_norm: f32, row: &[f32], row_norm: f32) -> f32 {
    if query_norm == 0.0 || row_norm == 0.0 || row.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    for (a, b) in query.iter().zip(row.iter()) {
        dot += a * b;
    }
    dot / (query_norm * row_norm)
}

/// Extract title from chunk content (first markdown heading or first line)
fn extract_title(content: &str) -> &str {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "";
    }

    // Try to find first markdown heading
    for line in trimmed.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            // Remove markdown heading markers and trim
            let title = line.trim_start_matches('#').trim();
            if !title.is_empty() {
                return title;
            }
        } else if !line.is_empty() {
            // First non-empty, non-heading line
            return line;
        }
    }

    trimmed
}

/// Lexical match scoring for hybrid search
#[derive(Debug, Clone, Copy, PartialEq)]
enum LexicalMatch {
    /// Exact phrase match in title
    ExactTitle,
    /// Exact phrase match in body
    ExactBody,
    /// All keywords present in title
    AllKeywordsTitle,
    /// All keywords present in body
    AllKeywordsBody,
    /// Some keywords present
    PartialMatch,
    /// No lexical match
    NoMatch,
}

/// Compute lexical match score for a chunk
fn compute_lexical_match(query_text: &str, content: &str) -> LexicalMatch {
    let query_lower = query_text.to_lowercase();
    let content_lower = content.to_lowercase();
    let title = extract_title(content);
    let title_lower = title.to_lowercase();

    // Check for exact phrase match in title
    if title_lower.contains(&query_lower) {
        return LexicalMatch::ExactTitle;
    }

    // Check for exact phrase match in body
    if content_lower.contains(&query_lower) {
        return LexicalMatch::ExactBody;
    }

    // Extract keywords (split on whitespace and common punctuation)
    let keywords: Vec<String> = query_lower
        .split(|c: char| c.is_whitespace() || c == ',' || c == '.' || c == ';' || c == ':')
        .filter(|s| s.len() > 2) // Filter out very short words
        .map(|s| s.to_string())
        .collect();

    if keywords.is_empty() {
        return if content_lower.contains(&query_lower) {
            LexicalMatch::ExactBody
        } else {
            LexicalMatch::NoMatch
        };
    }

    // Check if all keywords are in title
    let all_in_title = keywords.iter().all(|kw| title_lower.contains(kw));
    if all_in_title {
        return LexicalMatch::AllKeywordsTitle;
    }

    // Check if all keywords are in body
    let all_in_body = keywords.iter().all(|kw| content_lower.contains(kw));
    if all_in_body {
        return LexicalMatch::AllKeywordsBody;
    }

    // Check if some keywords are present
    let any_match = keywords.iter().any(|kw| content_lower.contains(kw));
    if any_match {
        return LexicalMatch::PartialMatch;
    }

    LexicalMatch::NoMatch
}

/// Compute hybrid score combining lexical match and semantic similarity
fn compute_hybrid_score(lexical_match: LexicalMatch, semantic_score: f32) -> (u32, f32) {
    // Return (priority_tier, final_score)
    // Lower tier number = higher priority
    match lexical_match {
        LexicalMatch::ExactTitle => (1, 1.0), // Highest priority
        LexicalMatch::ExactBody => (2, 0.98),
        LexicalMatch::AllKeywordsTitle => {
            // Boost semantic score for title matches
            (3, 0.90 + semantic_score.max(0.0).min(1.0) * 0.08)
        }
        LexicalMatch::AllKeywordsBody => {
            // Boost semantic score for body matches
            (4, 0.80 + semantic_score.max(0.0).min(1.0) * 0.10)
        }
        LexicalMatch::PartialMatch => {
            // Slight boost to semantic score
            (5, 0.50 + semantic_score.max(0.0).min(1.0) * 0.40)
        }
        LexicalMatch::NoMatch => {
            // Pure semantic fallback
            (6, semantic_score)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentsdb_format::EmbeddingElementType;
    use std::path::PathBuf;

    fn build_layer_two_chunks_f32(one_chunk: bool) -> Vec<u8> {
        // Strings: human, mcp, kind_a, kind_b, content_a, content_b
        let strings = [
            "human",
            "mcp",
            "kind_a",
            "kind_b",
            "content_a",
            "content_b",
            "file:1",
        ];

        let mut string_blob = Vec::new();
        let mut string_entries = Vec::new();
        for s in strings {
            let off = string_blob.len() as u64;
            string_blob.extend_from_slice(s.as_bytes());
            string_entries.push((off, s.len() as u64));
        }

        let string_header_size = 32u64;
        let string_entries_size = (string_entries.len() as u64) * 16;
        let string_section_len =
            string_header_size + string_entries_size + (string_blob.len() as u64);

        let chunk_header_size = 16u64;
        let chunk_record_size = 52u64;
        let chunk_count = if one_chunk { 1u64 } else { 2u64 };
        let chunk_section_len = chunk_header_size + chunk_count * chunk_record_size;

        let rel_header_size = 16u64;
        let rel_record_size = 8u64;
        let rel_count = 1u64;
        let rel_section_len = rel_header_size + rel_count * rel_record_size;

        let embed_header_size = 40u64;
        let row_count = if one_chunk { 1u64 } else { 2u64 };
        let dim = 2u32;
        let embed_data_len = row_count * dim as u64 * 4;
        let embed_section_len = embed_header_size + embed_data_len;

        let header_len = 40u64;
        let section_table_len = 4u64 * 24;
        let string_section_off = header_len + section_table_len;
        let chunk_section_off = string_section_off + string_section_len;
        let rel_section_off = chunk_section_off + chunk_section_len;
        let embed_section_off = rel_section_off + rel_section_len;
        let file_len = embed_section_off + embed_section_len;

        let mut buf = vec![0u8; file_len as usize];

        // Header
        put_u32(&mut buf, 0, 0x4244_4741);
        put_u16(&mut buf, 4, 1);
        put_u16(&mut buf, 6, 0);
        put_u64(&mut buf, 8, file_len);
        put_u64(&mut buf, 16, 4);
        put_u64(&mut buf, 24, header_len);
        put_u64(&mut buf, 32, 0);

        // Section table
        let mut sec = header_len as usize;
        // string dict
        put_u32(&mut buf, sec, 1);
        put_u32(&mut buf, sec + 4, 0);
        put_u64(&mut buf, sec + 8, string_section_off);
        put_u64(&mut buf, sec + 16, string_section_len);
        sec += 24;
        // chunk table
        put_u32(&mut buf, sec, 2);
        put_u32(&mut buf, sec + 4, 0);
        put_u64(&mut buf, sec + 8, chunk_section_off);
        put_u64(&mut buf, sec + 16, chunk_section_len);
        sec += 24;
        // relationships
        put_u32(&mut buf, sec, 4);
        put_u32(&mut buf, sec + 4, 0);
        put_u64(&mut buf, sec + 8, rel_section_off);
        put_u64(&mut buf, sec + 16, rel_section_len);
        sec += 24;
        // embedding matrix
        put_u32(&mut buf, sec, 3);
        put_u32(&mut buf, sec + 4, 0);
        put_u64(&mut buf, sec + 8, embed_section_off);
        put_u64(&mut buf, sec + 16, embed_section_len);

        // StringDictionaryHeaderV1
        let string_entries_off = string_section_off + string_header_size;
        let string_bytes_off = string_entries_off + string_entries_size;
        put_u64(
            &mut buf,
            string_section_off as usize,
            string_entries.len() as u64,
        );
        put_u64(
            &mut buf,
            string_section_off as usize + 8,
            string_entries_off,
        );
        put_u64(&mut buf, string_section_off as usize + 16, string_bytes_off);
        put_u64(
            &mut buf,
            string_section_off as usize + 24,
            string_blob.len() as u64,
        );
        // StringEntry[]
        for (i, (off, len)) in string_entries.iter().enumerate() {
            let entry_off = string_entries_off as usize + i * 16;
            put_u64(&mut buf, entry_off, *off);
            put_u64(&mut buf, entry_off + 8, *len);
        }
        // bytes blob
        buf[string_bytes_off as usize..(string_bytes_off as usize + string_blob.len())]
            .copy_from_slice(&string_blob);

        // RelationshipsHeaderV1 + one record: REL_SOURCE_STRING, value_u32=7 ("file:1")
        put_u64(&mut buf, rel_section_off as usize, rel_count);
        let rel_records_off = rel_section_off + rel_header_size;
        put_u64(&mut buf, rel_section_off as usize + 8, rel_records_off);
        put_u32(&mut buf, rel_records_off as usize, 2);
        put_u32(&mut buf, rel_records_off as usize + 4, 7);

        // ChunkTableHeaderV1
        put_u64(&mut buf, chunk_section_off as usize, chunk_count);
        let chunk_records_off = chunk_section_off + chunk_header_size;
        put_u64(&mut buf, chunk_section_off as usize + 8, chunk_records_off);

        // ChunkRecord #1
        let rec1 = chunk_records_off as usize;
        put_u32(&mut buf, rec1, 1);
        put_u32(&mut buf, rec1 + 4, 3); // kind_a
        put_u32(&mut buf, rec1 + 8, 5); // content_a
        put_u32(&mut buf, rec1 + 12, 1); // human
        put_f32(&mut buf, rec1 + 16, 1.0);
        put_u64(&mut buf, rec1 + 20, 0);
        put_u32(&mut buf, rec1 + 28, 1);
        put_u32(&mut buf, rec1 + 32, 0);
        put_u64(&mut buf, rec1 + 36, 0);
        put_u32(&mut buf, rec1 + 44, 0);
        put_u32(&mut buf, rec1 + 48, 0);

        if !one_chunk {
            // ChunkRecord #2
            let rec2 = (chunk_records_off + chunk_record_size) as usize;
            put_u32(&mut buf, rec2, 2);
            put_u32(&mut buf, rec2 + 4, 4); // kind_b
            put_u32(&mut buf, rec2 + 8, 6); // content_b
            put_u32(&mut buf, rec2 + 12, 2); // mcp
            put_f32(&mut buf, rec2 + 16, 0.5);
            put_u64(&mut buf, rec2 + 20, 0);
            put_u32(&mut buf, rec2 + 28, 2);
            put_u32(&mut buf, rec2 + 32, 0);
            put_u64(&mut buf, rec2 + 36, 0);
            put_u32(&mut buf, rec2 + 44, 0);
            put_u32(&mut buf, rec2 + 48, 0);
        }

        // EmbeddingMatrixHeaderV1
        put_u64(&mut buf, embed_section_off as usize, row_count);
        put_u32(&mut buf, embed_section_off as usize + 8, dim);
        put_u32(&mut buf, embed_section_off as usize + 12, 1);
        let embed_data_off = embed_section_off + embed_header_size;
        put_u64(&mut buf, embed_section_off as usize + 16, embed_data_off);
        put_u64(&mut buf, embed_section_off as usize + 24, embed_data_len);
        put_f32(&mut buf, embed_section_off as usize + 32, 1.0);
        put_f32(&mut buf, embed_section_off as usize + 36, 0.0);
        // row1: [1,0], row2: [0,1]
        put_f32(&mut buf, embed_data_off as usize, 1.0);
        put_f32(&mut buf, embed_data_off as usize + 4, 0.0);
        if !one_chunk {
            put_f32(&mut buf, embed_data_off as usize + 8, 0.0);
            put_f32(&mut buf, embed_data_off as usize + 12, 1.0);
        }

        buf
    }

    fn put_u16(buf: &mut [u8], off: usize, v: u16) {
        buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u32(buf: &mut [u8], off: usize, v: u32) {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u64(buf: &mut [u8], off: usize, v: u64) {
        buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn put_f32(buf: &mut [u8], off: usize, v: f32) {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn single_layer_search_orders_by_score() {
        let data = build_layer_two_chunks_f32(false);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AGENTS.db");
        std::fs::write(&path, &data).unwrap();

        let layer = LayerFile::open(&path).unwrap();
        assert_eq!(
            layer.embedding_matrix.element_type,
            EmbeddingElementType::F32
        );

        let layers = vec![(LayerId::Base, layer)];
        let q = SearchQuery {
            embedding: vec![1.0, 0.0],
            k: 10,
            filters: SearchFilters::default(),
            query_text: None,
        };
        let res = search_layers(&layers, &q).unwrap();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].chunk.id.get(), 1);
        assert_eq!(res[1].chunk.id.get(), 2);
    }

    #[test]
    fn union_hides_lower_precedence_duplicates() {
        let base = build_layer_two_chunks_f32(false);
        let local = build_layer_two_chunks_f32(true); // only id=1

        let dir = tempfile::tempdir().unwrap();
        let base_path = dir.path().join("AGENTS.db");
        let local_path = dir.path().join("AGENTS.local.db");
        std::fs::write(&base_path, &base).unwrap();
        std::fs::write(&local_path, &local).unwrap();

        let layers = vec![
            (LayerId::Local, LayerFile::open(&local_path).unwrap()),
            (LayerId::Base, LayerFile::open(&base_path).unwrap()),
        ];
        validate_schema_compatible(&layers).unwrap();

        let q = SearchQuery {
            embedding: vec![1.0, 0.0],
            k: 10,
            filters: SearchFilters::default(),
            query_text: None,
        };
        let res = search_layers(&layers, &q).unwrap();

        // Expect only 2 visible chunks: local id=1, base id=2.
        let ids: Vec<u32> = res.iter().map(|r| r.chunk.id.get()).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert_eq!(ids.len(), 2);

        let local_1 = res.iter().find(|r| r.chunk.id.get() == 1).unwrap();
        assert_eq!(local_1.layer, LayerId::Local);
        assert_eq!(local_1.hidden_layers, vec![LayerId::Base]);
    }

    #[test]
    fn search_with_index_matches_bruteforce() {
        let data = build_layer_two_chunks_f32(false);
        let dir = tempfile::tempdir().unwrap();
        let layer_path = dir.path().join("AGENTS.db");
        std::fs::write(&layer_path, &data).unwrap();

        let layer = LayerFile::open(&layer_path).unwrap();
        assert_eq!(
            layer.embedding_matrix.element_type,
            EmbeddingElementType::F32
        );

        let index_path = PathBuf::from(format!("{}.agix", layer_path.display()));
        build_layer_index(
            &layer,
            &index_path,
            IndexBuildOptions {
                store_embeddings_even_if_f32: false,
            },
        )
        .unwrap();

        let layers = vec![(LayerId::Base, layer)];
        let q = SearchQuery {
            embedding: vec![1.0, 0.0],
            k: 10,
            filters: SearchFilters::default(),
            query_text: None,
        };

        let brute =
            search_layers_with_options(&layers, &q, SearchOptions { use_index: false, mode: SearchMode::Semantic }).unwrap();
        let indexed =
            search_layers_with_options(&layers, &q, SearchOptions { use_index: true, mode: SearchMode::Semantic }).unwrap();

        assert_eq!(brute.len(), indexed.len());
        for (a, b) in brute.iter().zip(indexed.iter()) {
            assert_eq!(a.layer, b.layer);
            assert_eq!(a.chunk.id, b.chunk.id);
            assert_eq!(a.chunk.kind, b.chunk.kind);
            assert_eq!(a.chunk.content, b.chunk.content);
        }
    }
}
