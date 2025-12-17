use crate::{EmbeddingElementType, LayerFile};
use agentsdb_core::error::{Error, FormatError, PermissionError};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

const MAGIC_AGDB: u32 = 0x4244_4741; // 'A' 'G' 'D' 'B'

const SECTION_STRING_DICTIONARY: u32 = 1;
const SECTION_CHUNK_TABLE: u32 = 2;
const SECTION_EMBEDDING_MATRIX: u32 = 3;
const SECTION_RELATIONSHIPS: u32 = 4;
const SECTION_LAYER_METADATA: u32 = 5;

const LAYER_METADATA_FORMAT_JSON: u32 = 1;

const REL_SOURCE_CHUNK_ID: u32 = 1;
const REL_SOURCE_STRING: u32 = 2;

#[derive(Debug, Clone)]
pub struct LayerSchema {
    pub dim: u32,
    pub element_type: EmbeddingElementType,
    pub quant_scale: f32,
}

#[derive(Debug, Clone)]
pub enum ChunkSource {
    ChunkId(u32),
    SourceString(String),
}

#[derive(Debug, Clone)]
pub struct ChunkInput {
    pub id: u32, // 0 = auto-assign
    pub kind: String,
    pub content: String,
    pub author: String, // "human" | "mcp"
    pub confidence: f32,
    pub created_at_unix_ms: u64,
    pub embedding: Vec<f32>, // dim f32, regardless of on-disk element type
    pub sources: Vec<ChunkSource>,
}

pub fn schema_of(file: &LayerFile) -> LayerSchema {
    LayerSchema {
        dim: file.embedding_matrix.dim,
        element_type: file.embedding_matrix.element_type,
        quant_scale: file.embedding_matrix.quant_scale,
    }
}

pub fn write_layer_atomic(
    path: impl AsRef<Path>,
    schema: &LayerSchema,
    chunks: &mut [ChunkInput],
    layer_metadata_json: Option<&[u8]>,
) -> Result<Vec<u32>, Error> {
    // Auto-assign IDs for chunks with id=0
    let mut used_ids: HashSet<u32> = chunks.iter().filter(|c| c.id != 0).map(|c| c.id).collect();
    let mut next_id = used_ids
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1);

    let mut assigned = Vec::with_capacity(chunks.len());
    for c in chunks.iter_mut() {
        if c.id == 0 {
            while used_ids.contains(&next_id) {
                next_id = next_id.saturating_add(1);
            }
            c.id = next_id;
            used_ids.insert(c.id);
            next_id = next_id.saturating_add(1);
        }
        assigned.push(c.id);
    }

    let bytes = encode_layer(schema, chunks, layer_metadata_json)?;
    atomic_write(path.as_ref(), &bytes)?;
    Ok(assigned)
}

pub fn append_layer_atomic(
    path: impl AsRef<Path>,
    new_chunks: &mut [ChunkInput],
    layer_metadata_json: Option<&[u8]>,
) -> Result<Vec<u32>, Error> {
    let path = path.as_ref();
    let file = LayerFile::open(path)?;
    let schema = schema_of(&file);
    let mut all_chunks = decode_all_chunks(&file)?;
    let existing_metadata = file.layer_metadata_bytes().map(|b| b.to_vec());
    let metadata_to_write = layer_metadata_json
        .map(|b| b.to_vec())
        .or(existing_metadata);

    let mut used_ids: HashSet<u32> = all_chunks.iter().map(|c| c.id).collect();
    let mut next_id = used_ids
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .max(1);

    let mut assigned = Vec::with_capacity(new_chunks.len());
    for c in new_chunks.iter_mut() {
        if c.id == 0 {
            while used_ids.contains(&next_id) {
                next_id = next_id.saturating_add(1);
            }
            c.id = next_id;
            used_ids.insert(c.id);
            next_id = next_id.saturating_add(1);
        } else {
            used_ids.insert(c.id);
        }
        if c.id == 0 {
            return Err(FormatError::InvalidValue {
                field: "ChunkRecord.id",
                reason: "must be non-zero",
            }
            .into());
        }
        assigned.push(c.id);
        all_chunks.push(c.clone());
    }

    let bytes = encode_layer(&schema, &all_chunks, metadata_to_write.as_deref())?;
    atomic_write(path, &bytes)?;
    Ok(assigned)
}

pub fn ensure_writable_layer_path(path: impl AsRef<Path>) -> Result<(), Error> {
    ensure_writable_layer_path_inner(path.as_ref(), false, false)
}

pub fn ensure_writable_layer_path_allow_user(path: impl AsRef<Path>) -> Result<(), Error> {
    ensure_writable_layer_path_inner(path.as_ref(), true, false)
}

pub fn ensure_writable_layer_path_allow_base(path: impl AsRef<Path>) -> Result<(), Error> {
    // Escape hatch: allow writing to AGENTS.db when explicitly requested by the caller.
    ensure_writable_layer_path_inner(path.as_ref(), true, true)
}

pub fn read_all_chunks(file: &LayerFile) -> Result<Vec<ChunkInput>, Error> {
    decode_all_chunks(file)
}

fn ensure_writable_layer_path_inner(
    path: &Path,
    allow_user: bool,
    allow_base: bool,
) -> Result<(), Error> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let forbidden = match (allow_user, allow_base) {
        // Default: only local/delta allowed.
        (false, _) => ["AGENTS.db", "AGENTS.user.db"].as_slice(),
        // User allowed, but base still protected.
        (true, false) => ["AGENTS.db"].as_slice(),
        // Escape hatch: allow base + user.
        (true, true) => [].as_slice(),
    };
    if forbidden.contains(&name) {
        return Err(PermissionError::WriteNotPermitted {
            path: path.to_path_buf(),
        }
        .into());
    }
    Ok(())
}

fn decode_all_chunks(file: &LayerFile) -> Result<Vec<ChunkInput>, Error> {
    let dim = file.embedding_dim();
    let mut tmp = vec![0.0f32; dim];
    let mut out = Vec::with_capacity(file.chunk_count as usize);
    for c in file.chunks() {
        let c = c?;
        file.read_embedding_row_f32(c.embedding_row, &mut tmp)?;
        let sources = file
            .sources_for(c.rel_start, c.rel_count)?
            .into_iter()
            .map(|s| match s {
                crate::SourceRef::ChunkId(id) => ChunkSource::ChunkId(id),
                crate::SourceRef::String(v) => ChunkSource::SourceString(v.to_string()),
            })
            .collect();

        out.push(ChunkInput {
            id: c.id,
            kind: c.kind.to_string(),
            content: c.content.to_string(),
            author: c.author.to_string(),
            confidence: c.confidence,
            created_at_unix_ms: c.created_at_unix_ms,
            embedding: tmp.clone(),
            sources,
        });
    }
    Ok(out)
}

fn encode_layer(
    schema: &LayerSchema,
    chunks: &[ChunkInput],
    layer_metadata_json: Option<&[u8]>,
) -> Result<Vec<u8>, Error> {
    if schema.dim == 0 {
        return Err(FormatError::InvalidValue {
            field: "EmbeddingMatrixHeaderV1.dim",
            reason: "must be non-zero",
        }
        .into());
    }
    let dim = schema.dim as usize;

    for c in chunks {
        if c.id == 0 {
            return Err(FormatError::InvalidValue {
                field: "ChunkRecord.id",
                reason: "must be non-zero",
            }
            .into());
        }
        if c.author != "human" && c.author != "mcp" {
            return Err(FormatError::InvalidValue {
                field: "ChunkRecord.author_str_id",
                reason: "author must be 'human' or 'mcp'",
            }
            .into());
        }
        if !c.confidence.is_finite() || !(0.0..=1.0).contains(&c.confidence) {
            return Err(FormatError::InvalidValue {
                field: "ChunkRecord.confidence",
                reason: "must be finite and in range 0.0..=1.0",
            }
            .into());
        }
        if c.embedding.len() != dim {
            return Err(FormatError::InvalidValue {
                field: "embedding",
                reason: "must match schema dim",
            }
            .into());
        }
    }

    // Determine whether to include relationships.
    let include_relationships = chunks.iter().any(|c| !c.sources.is_empty());
    let include_layer_metadata = layer_metadata_json.is_some();

    // Intern strings in deterministic first-seen order.
    let mut strings: Vec<String> = Vec::new();
    let mut string_ids: HashMap<String, u32> = HashMap::new();
    let mut intern = |s: &str| -> u32 {
        if let Some(&id) = string_ids.get(s) {
            return id;
        }
        let id = (strings.len() as u32) + 1;
        strings.push(s.to_string());
        string_ids.insert(s.to_string(), id);
        id
    };

    for c in chunks {
        let _ = intern(&c.kind);
        let _ = intern(&c.content);
        let _ = intern(&c.author);
        if include_relationships {
            for src in &c.sources {
                if let ChunkSource::SourceString(s) = src {
                    let _ = intern(s);
                }
            }
        }
    }

    // Build string blob and entries.
    let mut string_blob = Vec::new();
    let mut string_entries: Vec<(u64, u64)> = Vec::with_capacity(strings.len());
    for s in &strings {
        let off = string_blob.len() as u64;
        string_blob.extend_from_slice(s.as_bytes());
        string_entries.push((off, s.len() as u64));
    }

    // Relationships: packed in chunk order.
    let mut rel_records: Vec<(u32, u32)> = Vec::new();
    let mut chunk_rel: Vec<(u64, u32)> = Vec::with_capacity(chunks.len());
    if include_relationships {
        for c in chunks {
            let start = rel_records.len() as u64;
            for src in &c.sources {
                match src {
                    ChunkSource::ChunkId(id) => rel_records.push((REL_SOURCE_CHUNK_ID, *id)),
                    ChunkSource::SourceString(s) => {
                        let sid = *string_ids.get(s).expect("interned");
                        rel_records.push((REL_SOURCE_STRING, sid));
                    }
                }
            }
            let count = (rel_records.len() as u64 - start) as u32;
            chunk_rel.push((start, count));
        }
    } else {
        for _ in chunks {
            chunk_rel.push((0, 0));
        }
    }

    // Layout.
    let header_len = 40u64;
    let mut section_count = 3u64;
    if include_relationships {
        section_count += 1;
    }
    if include_layer_metadata {
        section_count += 1;
    }
    let section_table_len = section_count * 24u64;

    let string_header_size = 32u64;
    let string_entries_size = (strings.len() as u64) * 16u64;
    let string_section_len = string_header_size + string_entries_size + (string_blob.len() as u64);

    let chunk_header_size = 16u64;
    let chunk_records_size = (chunks.len() as u64) * 52u64;
    let chunk_section_len = chunk_header_size + chunk_records_size;

    let embed_header_size = 40u64;
    let elem_size = match schema.element_type {
        EmbeddingElementType::F32 => 4u64,
        EmbeddingElementType::I8 => 1u64,
    };
    let row_count = chunks.len() as u64;
    let embed_data_len = row_count
        .checked_mul(schema.dim as u64)
        .and_then(|v| v.checked_mul(elem_size))
        .ok_or(FormatError::InvalidRange {
            field: "EmbeddingMatrixHeaderV1.row_count/dim",
        })?;
    let embed_section_len = embed_header_size + embed_data_len;

    let rel_header_size = 16u64;
    let rel_records_size = (rel_records.len() as u64) * 8u64;
    let rel_section_len = rel_header_size + rel_records_size;

    let layer_metadata_header_size = 24u64;
    let layer_metadata_len = layer_metadata_json.map(|b| b.len() as u64).unwrap_or(0);
    let layer_metadata_section_len = layer_metadata_header_size + layer_metadata_len;

    let string_section_off = header_len + section_table_len;
    let chunk_section_off = string_section_off + string_section_len;
    let layer_metadata_section_off = if include_layer_metadata {
        Some(chunk_section_off + chunk_section_len)
    } else {
        None
    };
    let after_meta = layer_metadata_section_off
        .map(|off| off + layer_metadata_section_len)
        .unwrap_or(chunk_section_off + chunk_section_len);
    let rel_section_off = if include_relationships {
        Some(after_meta)
    } else {
        None
    };
    let after_rel = rel_section_off
        .map(|off| off + rel_section_len)
        .unwrap_or(after_meta);
    let embed_section_off = after_rel;
    let file_len = embed_section_off + embed_section_len;

    let mut buf = vec![0u8; file_len as usize];

    // Header
    put_u32(&mut buf, 0, MAGIC_AGDB);
    put_u16(&mut buf, 4, 1);
    put_u16(&mut buf, 6, 0);
    put_u64(&mut buf, 8, file_len);
    put_u64(&mut buf, 16, section_count);
    put_u64(&mut buf, 24, header_len);
    put_u64(&mut buf, 32, 0);

    // Section table
    let mut sec = header_len as usize;
    // string dict
    put_u32(&mut buf, sec, SECTION_STRING_DICTIONARY);
    put_u32(&mut buf, sec + 4, 0);
    put_u64(&mut buf, sec + 8, string_section_off);
    put_u64(&mut buf, sec + 16, string_section_len);
    sec += 24;
    // chunk table
    put_u32(&mut buf, sec, SECTION_CHUNK_TABLE);
    put_u32(&mut buf, sec + 4, 0);
    put_u64(&mut buf, sec + 8, chunk_section_off);
    put_u64(&mut buf, sec + 16, chunk_section_len);
    sec += 24;
    if let Some(meta_off) = layer_metadata_section_off {
        put_u32(&mut buf, sec, SECTION_LAYER_METADATA);
        put_u32(&mut buf, sec + 4, 0);
        put_u64(&mut buf, sec + 8, meta_off);
        put_u64(&mut buf, sec + 16, layer_metadata_section_len);
        sec += 24;
    }
    if let Some(rel_off) = rel_section_off {
        put_u32(&mut buf, sec, SECTION_RELATIONSHIPS);
        put_u32(&mut buf, sec + 4, 0);
        put_u64(&mut buf, sec + 8, rel_off);
        put_u64(&mut buf, sec + 16, rel_section_len);
        sec += 24;
    }
    // embedding matrix
    put_u32(&mut buf, sec, SECTION_EMBEDDING_MATRIX);
    put_u32(&mut buf, sec + 4, 0);
    put_u64(&mut buf, sec + 8, embed_section_off);
    put_u64(&mut buf, sec + 16, embed_section_len);

    // StringDictionary section
    let string_entries_off = string_section_off + string_header_size;
    let string_bytes_off = string_entries_off + string_entries_size;
    put_u64(&mut buf, string_section_off as usize, strings.len() as u64);
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
    for (i, (off, len)) in string_entries.iter().enumerate() {
        let entry_off = string_entries_off as usize + i * 16;
        put_u64(&mut buf, entry_off, *off);
        put_u64(&mut buf, entry_off + 8, *len);
    }
    buf[string_bytes_off as usize..(string_bytes_off as usize + string_blob.len())]
        .copy_from_slice(&string_blob);

    // Relationships section (optional)
    if let Some(rel_off) = rel_section_off {
        put_u64(&mut buf, rel_off as usize, rel_records.len() as u64);
        let rel_records_off = rel_off + rel_header_size;
        put_u64(&mut buf, rel_off as usize + 8, rel_records_off);
        for (i, (kind, value)) in rel_records.iter().enumerate() {
            let off = rel_records_off as usize + i * 8;
            put_u32(&mut buf, off, *kind);
            put_u32(&mut buf, off + 4, *value);
        }
    }

    // Layer metadata (optional)
    if let (Some(meta_off), Some(meta_bytes)) = (layer_metadata_section_off, layer_metadata_json) {
        let blob_off = meta_off + layer_metadata_header_size;
        put_u32(&mut buf, meta_off as usize, 1);
        put_u32(&mut buf, meta_off as usize + 4, LAYER_METADATA_FORMAT_JSON);
        put_u64(&mut buf, meta_off as usize + 8, blob_off);
        put_u64(&mut buf, meta_off as usize + 16, meta_bytes.len() as u64);
        buf[blob_off as usize..(blob_off as usize + meta_bytes.len())].copy_from_slice(meta_bytes);
    }

    // Chunk table
    put_u64(&mut buf, chunk_section_off as usize, chunks.len() as u64);
    let chunk_records_off = chunk_section_off + chunk_header_size;
    put_u64(&mut buf, chunk_section_off as usize + 8, chunk_records_off);
    for (i, c) in chunks.iter().enumerate() {
        let rec_off = chunk_records_off as usize + i * 52;
        let (rel_start, rel_count) = chunk_rel[i];
        put_u32(&mut buf, rec_off, c.id);
        put_u32(
            &mut buf,
            rec_off + 4,
            *string_ids.get(&c.kind).expect("interned"),
        );
        put_u32(
            &mut buf,
            rec_off + 8,
            *string_ids.get(&c.content).expect("interned"),
        );
        put_u32(
            &mut buf,
            rec_off + 12,
            *string_ids.get(&c.author).expect("interned"),
        );
        put_f32(&mut buf, rec_off + 16, c.confidence);
        put_u64(&mut buf, rec_off + 20, c.created_at_unix_ms);
        put_u32(&mut buf, rec_off + 28, (i as u32) + 1); // embedding_row (1-based)
        put_u32(&mut buf, rec_off + 32, 0);
        put_u64(&mut buf, rec_off + 36, rel_start);
        put_u32(&mut buf, rec_off + 44, rel_count);
        put_u32(&mut buf, rec_off + 48, 0);
    }

    // Embedding matrix
    put_u64(&mut buf, embed_section_off as usize, row_count);
    put_u32(&mut buf, embed_section_off as usize + 8, schema.dim);
    put_u32(
        &mut buf,
        embed_section_off as usize + 12,
        match schema.element_type {
            EmbeddingElementType::F32 => 1,
            EmbeddingElementType::I8 => 2,
        },
    );
    let embed_data_off = embed_section_off + embed_header_size;
    put_u64(&mut buf, embed_section_off as usize + 16, embed_data_off);
    put_u64(&mut buf, embed_section_off as usize + 24, embed_data_len);
    put_f32(
        &mut buf,
        embed_section_off as usize + 32,
        match schema.element_type {
            EmbeddingElementType::F32 => 1.0,
            EmbeddingElementType::I8 => schema.quant_scale,
        },
    );
    put_f32(&mut buf, embed_section_off as usize + 36, 0.0);

    match schema.element_type {
        EmbeddingElementType::F32 => {
            let mut at = embed_data_off as usize;
            for c in chunks {
                for x in &c.embedding {
                    put_f32(&mut buf, at, *x);
                    at += 4;
                }
            }
        }
        EmbeddingElementType::I8 => {
            let scale = schema.quant_scale;
            if !scale.is_finite() || scale == 0.0 {
                return Err(FormatError::InvalidValue {
                    field: "EmbeddingMatrixHeaderV1.quant_scale",
                    reason: "must be finite and non-zero for EMBED_I8",
                }
                .into());
            }
            let mut at = embed_data_off as usize;
            for c in chunks {
                for x in &c.embedding {
                    let q = (*x / scale).round();
                    let clamped = q.clamp(-128.0, 127.0) as i32;
                    buf[at] = (clamped as i8) as u8;
                    at += 1;
                }
            }
        }
    }

    Ok(buf)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let base = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("AGENTS.db");

    let mut i = 0u32;
    loop {
        let tmp_name = if i == 0 {
            format!("{base}.tmp")
        } else {
            format!("{base}.tmp.{i}")
        };
        let tmp_path = dir.join(tmp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(mut f) => {
                f.write_all(bytes)?;
                f.sync_all()?;
                std::fs::rename(&tmp_path, path)?;
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                i = i.saturating_add(1);
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LayerFile;

    #[test]
    fn writer_produces_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AGENTS.delta.db");

        let schema = LayerSchema {
            dim: 2,
            element_type: EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        let mut chunks = vec![ChunkInput {
            id: 1,
            kind: "note".to_string(),
            content: "hello".to_string(),
            author: "mcp".to_string(),
            confidence: 0.9,
            created_at_unix_ms: 0,
            embedding: vec![0.0, 1.0],
            sources: vec![ChunkSource::SourceString("file:1".to_string())],
        }];

        write_layer_atomic(&path, &schema, &mut chunks, None).unwrap();
        let opened = LayerFile::open(&path).unwrap();
        assert_eq!(opened.chunk_count, 1);
        assert_eq!(opened.embedding_matrix.dim, 2);
        assert_eq!(opened.relationship_count, Some(1));
    }

    #[test]
    fn layer_metadata_roundtrips_and_is_preserved_on_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AGENTS.delta.db");

        let schema = LayerSchema {
            dim: 2,
            element_type: EmbeddingElementType::F32,
            quant_scale: 1.0,
        };
        let mut chunks = vec![ChunkInput {
            id: 1,
            kind: "note".to_string(),
            content: "hello".to_string(),
            author: "mcp".to_string(),
            confidence: 0.9,
            created_at_unix_ms: 0,
            embedding: vec![0.0, 1.0],
            sources: vec![],
        }];

        let meta1 = br#"{"v":1,"x":"y"}"#;
        write_layer_atomic(&path, &schema, &mut chunks, Some(meta1)).unwrap();
        let opened = LayerFile::open(&path).unwrap();
        assert_eq!(
            opened.layer_metadata_json().unwrap().unwrap(),
            r#"{"v":1,"x":"y"}"#
        );

        let mut new_chunks = vec![ChunkInput {
            id: 0,
            kind: "note".to_string(),
            content: "world".to_string(),
            author: "mcp".to_string(),
            confidence: 0.9,
            created_at_unix_ms: 0,
            embedding: vec![1.0, 0.0],
            sources: vec![],
        }];
        append_layer_atomic(&path, &mut new_chunks, None).unwrap();
        let reopened = LayerFile::open(&path).unwrap();
        assert_eq!(
            reopened.layer_metadata_json().unwrap().unwrap(),
            r#"{"v":1,"x":"y"}"#
        );

        let meta2 = br#"{"v":1,"x":"z"}"#;
        let mut another = vec![ChunkInput {
            id: 0,
            kind: "note".to_string(),
            content: "again".to_string(),
            author: "mcp".to_string(),
            confidence: 0.9,
            created_at_unix_ms: 0,
            embedding: vec![0.5, 0.5],
            sources: vec![],
        }];
        append_layer_atomic(&path, &mut another, Some(meta2)).unwrap();
        let reopened = LayerFile::open(&path).unwrap();
        assert_eq!(
            reopened.layer_metadata_json().unwrap().unwrap(),
            r#"{"v":1,"x":"z"}"#
        );
    }
}
