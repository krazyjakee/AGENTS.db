use agentsdb_core::error::{Error, FormatError, SchemaError};
use agentsdb_core::types::{
    Author, Chunk, ChunkId, LayerId, ProvenanceRef, SearchFilters, SearchResult,
};
use agentsdb_format::{LayerFile, SourceRef};
use std::collections::{HashMap, HashSet};

type Visibility = (HashSet<(LayerId, ChunkId)>, HashMap<ChunkId, Vec<LayerId>>);

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub embedding: Vec<f32>,
    pub k: usize,
    pub filters: SearchFilters,
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
        if let Some(path) = &self.local {
            layers.push((LayerId::Local, LayerFile::open(path)?));
        }
        if let Some(path) = &self.user {
            layers.push((LayerId::User, LayerFile::open(path)?));
        }
        if let Some(path) = &self.delta {
            layers.push((LayerId::Delta, LayerFile::open(path)?));
        }
        if let Some(path) = &self.base {
            layers.push((LayerId::Base, LayerFile::open(path)?));
        }
        validate_schema_compatible(&layers)?;
        Ok(layers)
    }
}

pub fn search_layers(
    layers: &[(LayerId, LayerFile)],
    query: &SearchQuery,
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

    // Precompute which chunk IDs are hidden by higher-precedence layers.
    let (visible, hidden_by) = compute_visibility(layers)?;

    let kind_filter: Option<HashSet<&str>> = if query.filters.kinds.is_empty() {
        None
    } else {
        Some(query.filters.kinds.iter().map(|s| s.as_str()).collect())
    };

    let query_norm = l2_norm(&query.embedding);
    let mut tmp = vec![0.0f32; dim];
    let mut hits: Vec<SearchResult> = Vec::new();

    for (layer_id, layer) in layers {
        for chunk_res in layer.chunks() {
            let chunk = chunk_res?;
            let chunk_id = ChunkId(chunk.id);
            if !visible.contains(&(*layer_id, chunk_id)) {
                continue;
            }
            if let Some(kinds) = &kind_filter {
                if !kinds.contains(chunk.kind) {
                    continue;
                }
            }

            layer.read_embedding_row_f32(chunk.embedding_row, &mut tmp)?;
            let score = cosine_similarity(&query.embedding, query_norm, &tmp);

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

            hits.push(SearchResult {
                layer: *layer_id,
                score,
                chunk: out_chunk,
                hidden_layers: hidden_by
                    .get(&ChunkId(chunk.id))
                    .cloned()
                    .unwrap_or_default(),
            });
        }
    }

    hits.sort_by(|a, b| {
        score_for_sort(b.score)
            .total_cmp(&score_for_sort(a.score))
            .then_with(|| a.chunk.id.cmp(&b.chunk.id))
            .then_with(|| a.layer.cmp(&b.layer))
    });
    hits.truncate(query.k);
    Ok(hits)
}

fn validate_schema_compatible(layers: &[(LayerId, LayerFile)]) -> Result<(), Error> {
    if layers.len() <= 1 {
        return Ok(());
    }
    let first = &layers[0].1.embedding_matrix;
    for (_, layer) in &layers[1..] {
        let other = &layer.embedding_matrix;
        if other.dim != first.dim {
            return Err(SchemaError::Mismatch("embedding dim mismatch").into());
        }
        if other.element_type != first.element_type {
            return Err(SchemaError::Mismatch("embedding element type mismatch").into());
        }
        // For v0.1, treat quant_scale as schema (especially for EMBED_I8).
        if other.quant_scale.to_bits() != first.quant_scale.to_bits() {
            return Err(SchemaError::Mismatch("embedding quant_scale mismatch").into());
        }
    }
    Ok(())
}

fn compute_visibility(layers: &[(LayerId, LayerFile)]) -> Result<Visibility, Error> {
    let mut seen: HashMap<ChunkId, LayerId> = HashMap::new();
    let mut visible: HashSet<(LayerId, ChunkId)> = HashSet::new();
    let mut hidden_by: HashMap<ChunkId, Vec<LayerId>> = HashMap::new();

    for (layer_id, layer) in layers {
        for chunk in layer.chunks() {
            let chunk = chunk?;
            let id = ChunkId(chunk.id);
            match seen.get(&id).copied() {
                None => {
                    seen.insert(id, *layer_id);
                    visible.insert((*layer_id, id));
                }
                Some(higher) => {
                    hidden_by.entry(id).or_default().push(*layer_id);
                    // Ensure the "higher" layer's chunk is marked visible (it will be).
                    let _ = higher;
                }
            }
        }
    }

    Ok((visible, hidden_by))
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

#[cfg(test)]
mod tests {
    use super::*;
    use agentsdb_format::EmbeddingElementType;

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
}
