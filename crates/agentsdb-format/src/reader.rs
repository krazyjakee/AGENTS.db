use agentsdb_core::error::FormatError;
use memmap2::Mmap;
use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

const MAGIC_AGDB: u32 = 0x4244_4741; // 'A' 'G' 'D' 'B'

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    StringDictionary,
    ChunkTable,
    EmbeddingMatrix,
    Relationships,
    LayerMetadata,
    Unknown(u32),
}

impl SectionKind {
    fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::StringDictionary,
            2 => Self::ChunkTable,
            3 => Self::EmbeddingMatrix,
            4 => Self::Relationships,
            5 => Self::LayerMetadata,
            other => Self::Unknown(other),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::StringDictionary => "SECTION_STRING_DICTIONARY",
            Self::ChunkTable => "SECTION_CHUNK_TABLE",
            Self::EmbeddingMatrix => "SECTION_EMBEDDING_MATRIX",
            Self::Relationships => "SECTION_RELATIONSHIPS",
            Self::LayerMetadata => "SECTION_LAYER_METADATA",
            Self::Unknown(_) => "SECTION_UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FileHeaderV1 {
    pub magic: u32,
    pub version_major: u16,
    pub version_minor: u16,
    pub file_length_bytes: u64,
    pub section_count: u64,
    pub sections_offset: u64,
    pub flags: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SectionEntry {
    pub kind: SectionKind,
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct StringDictionaryHeaderV1 {
    pub string_count: u64,
    pub entries_offset: u64,
    pub bytes_offset: u64,
    pub bytes_length: u64,
}

#[derive(Debug, Clone, Copy)]
struct StringEntry {
    byte_offset: u64,
    byte_length: u64,
}

#[derive(Debug, Clone, Copy)]
struct ChunkTableHeaderV1 {
    chunk_count: u64,
    records_offset: u64,
}

#[derive(Debug, Clone, Copy)]
struct ChunkRecord {
    id: u32,
    kind_str_id: u32,
    content_str_id: u32,
    author_str_id: u32,
    confidence: f32,
    created_at_unix_ms: u64,
    embedding_row: u32,
    reserved0: u32,
    rel_start: u64,
    rel_count: u32,
    reserved1: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingElementType {
    F32,
    I8,
}

impl EmbeddingElementType {
    fn from_u32(v: u32) -> Result<Self, FormatError> {
        match v {
            1 => Ok(Self::F32),
            2 => Ok(Self::I8),
            _ => Err(FormatError::InvalidValue {
                field: "EmbeddingMatrixHeaderV1.element_type",
                reason: "unknown embedding element type",
            }),
        }
    }

    fn size_bytes(self) -> u64 {
        match self {
            Self::F32 => 4,
            Self::I8 => 1,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EmbeddingMatrixHeaderV1 {
    pub row_count: u64,
    pub dim: u32,
    pub element_type: EmbeddingElementType,
    pub data_offset: u64,
    pub data_length: u64,
    pub quant_scale: f32,
    pub reserved0: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipKind {
    SourceChunkId,
    SourceString,
}

impl RelationshipKind {
    fn from_u32(v: u32) -> Result<Self, FormatError> {
        match v {
            1 => Ok(Self::SourceChunkId),
            2 => Ok(Self::SourceString),
            _ => Err(FormatError::InvalidValue {
                field: "RelationshipRecord.kind",
                reason: "unknown relationship kind",
            }),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RelationshipsHeaderV1 {
    relationship_count: u64,
    records_offset: u64,
}

#[derive(Debug, Clone, Copy)]
struct LayerMetadataHeaderV1 {
    version: u32,
    format: u32,
    blob_offset: u64,
    blob_length: u64,
}

#[derive(Debug)]
pub struct LayerFile {
    path: PathBuf,
    mmap: Mmap,
    pub header: FileHeaderV1,
    pub sections: Vec<SectionEntry>,
    pub string_dictionary: StringDictionaryHeaderV1,
    pub chunk_count: u64,
    chunk_records_offset: u64,
    pub embedding_matrix: EmbeddingMatrixHeaderV1,
    pub relationship_count: Option<u64>,
    relationships_records_offset: Option<u64>,
    layer_metadata: Option<LayerMetadataHeaderV1>,
}

impl LayerFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, agentsdb_core::error::Error> {
        Self::open_with_options(path, false)
    }

    /// Open a layer file without validating chunk ID uniqueness.
    /// This is intended for recovery/repair tools like `agentsdb compact`.
    pub fn open_lenient(path: impl AsRef<Path>) -> Result<Self, agentsdb_core::error::Error> {
        Self::open_with_options(path, true)
    }

    fn open_with_options(
        path: impl AsRef<Path>,
        allow_duplicate_ids: bool,
    ) -> Result<Self, agentsdb_core::error::Error> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let metadata = file.metadata()?;
        let actual_len = metadata.len();
        let mmap = unsafe { Mmap::map(&file)? };

        let bytes: &[u8] = mmap.as_ref();
        let header = parse_file_header(bytes)?;
        if header.file_length_bytes != actual_len {
            return Err(FormatError::FileLengthMismatch {
                header: header.file_length_bytes,
                actual: actual_len,
            }
            .into());
        }
        if header.flags != 0 {
            return Err(FormatError::NonZeroReserved {
                field: "FileHeaderV1.flags",
            }
            .into());
        }
        if header.version_major != 1 {
            return Err(FormatError::UnsupportedVersion {
                major: header.version_major,
                minor: header.version_minor,
            }
            .into());
        }

        let sections = parse_section_table(bytes, &header)?;
        let string_section = required_section(&sections, SectionKind::StringDictionary)?;
        let chunk_section = required_section(&sections, SectionKind::ChunkTable)?;
        let embed_section = required_section(&sections, SectionKind::EmbeddingMatrix)?;
        let rel_section = optional_section(&sections, SectionKind::Relationships)?;
        let metadata_section = optional_section(&sections, SectionKind::LayerMetadata)?;

        let string_dictionary = parse_string_dictionary_header(bytes, string_section)?;
        validate_string_dictionary(bytes, string_section, &string_dictionary)?;

        let chunk_header = parse_chunk_table_header(bytes, chunk_section)?;
        let chunk_count = chunk_header.chunk_count;

        let embedding_matrix = parse_embedding_matrix_header(bytes, embed_section)?;
        validate_embedding_matrix(bytes, embed_section, &embedding_matrix)?;

        let (relationship_count, relationships_records_offset) =
            if let Some(rel_section) = rel_section {
                let rel_header = parse_relationships_header(bytes, rel_section)?;
                validate_relationships(bytes, rel_section, &rel_header, &string_dictionary)?;
                (
                    Some(rel_header.relationship_count),
                    Some(rel_header.records_offset),
                )
            } else {
                (None, None)
            };

        let layer_metadata = if let Some(section) = metadata_section {
            let hdr = parse_layer_metadata_header(bytes, section)?;
            validate_layer_metadata(bytes, section, &hdr)?;
            Some(hdr)
        } else {
            None
        };

        validate_chunk_records(
            bytes,
            chunk_section,
            &chunk_header,
            &string_dictionary,
            &embedding_matrix,
            relationship_count,
            allow_duplicate_ids,
        )?;

        Ok(Self {
            path,
            mmap,
            header,
            sections,
            string_dictionary,
            chunk_count,
            chunk_records_offset: chunk_header.records_offset,
            embedding_matrix,
            relationship_count,
            relationships_records_offset,
            layer_metadata,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_bytes(&self) -> &[u8] {
        self.mmap.as_ref()
    }

    pub fn embedding_dim(&self) -> usize {
        self.embedding_matrix.dim as usize
    }

    pub fn layer_metadata_bytes(&self) -> Option<&[u8]> {
        let hdr = self.layer_metadata?;
        let bytes = self.file_bytes();
        let start = hdr.blob_offset as usize;
        let end = start.saturating_add(hdr.blob_length as usize);
        bytes.get(start..end)
    }

    pub fn layer_metadata_json(&self) -> Result<Option<&str>, agentsdb_core::error::Error> {
        let Some(bytes) = self.layer_metadata_bytes() else {
            return Ok(None);
        };
        Ok(Some(std::str::from_utf8(bytes).map_err(|_| {
            FormatError::InvalidValue {
                field: "LayerMetadataHeaderV1.blob",
                reason: "metadata blob is not valid UTF-8",
            }
        })?))
    }

    pub fn chunks(&self) -> ChunkIter<'_> {
        ChunkIter {
            file: self,
            index: 0,
        }
    }

    pub fn read_embedding_row_f32(
        &self,
        embedding_row: u32,
        out: &mut [f32],
    ) -> Result<(), agentsdb_core::error::Error> {
        if embedding_row == 0 || embedding_row as u64 > self.embedding_matrix.row_count {
            return Err(FormatError::InvalidEmbeddingRow {
                embedding_row,
                row_count: self.embedding_matrix.row_count,
            }
            .into());
        }
        if out.len() != self.embedding_dim() {
            return Err(FormatError::InvalidValue {
                field: "embedding",
                reason: "output buffer length must equal embedding dim",
            }
            .into());
        }

        let bytes = self.file_bytes();
        let dim = self.embedding_matrix.dim as u64;
        let idx0 = (embedding_row as u64) - 1;
        let elem_size = self.embedding_matrix.element_type.size_bytes();
        let row_bytes = dim
            .checked_mul(elem_size)
            .ok_or(FormatError::InvalidRange {
                field: "embedding row size",
            })?;
        let start = self
            .embedding_matrix
            .data_offset
            .checked_add(
                idx0.checked_mul(row_bytes)
                    .ok_or(FormatError::InvalidRange {
                        field: "embedding row offset",
                    })?,
            )
            .ok_or(FormatError::InvalidRange {
                field: "embedding row offset",
            })?;

        match self.embedding_matrix.element_type {
            EmbeddingElementType::F32 => {
                for (i, slot) in out.iter_mut().enumerate() {
                    *slot = read_f32(bytes, start + (i as u64) * 4)?;
                }
            }
            EmbeddingElementType::I8 => {
                let scale = self.embedding_matrix.quant_scale;
                let slice = slice_range(bytes, start, start + row_bytes)?;
                for (i, b) in slice.iter().enumerate() {
                    out[i] = (*b as i8) as f32 * scale;
                }
            }
        }

        Ok(())
    }

    pub fn sources_for(
        &self,
        rel_start: u64,
        rel_count: u32,
    ) -> Result<Vec<SourceRef<'_>>, agentsdb_core::error::Error> {
        if rel_count == 0 {
            return Ok(Vec::new());
        }
        let Some(relationship_count) = self.relationship_count else {
            return Err(FormatError::InvalidValue {
                field: "ChunkRecord.rel_count",
                reason: "relationships section is absent",
            }
            .into());
        };
        let Some(records_offset) = self.relationships_records_offset else {
            return Err(FormatError::InvalidRange {
                field: "RelationshipsHeaderV1.records_offset",
            }
            .into());
        };

        let rel_count_u64 = rel_count as u64;
        let end = rel_start
            .checked_add(rel_count_u64)
            .ok_or(FormatError::InvalidRange {
                field: "ChunkRecord.rel_start/rel_count",
            })?;
        if end > relationship_count {
            return Err(FormatError::InvalidRelationshipsRange {
                rel_start,
                rel_count,
                relationship_count,
            }
            .into());
        }

        let bytes = self.file_bytes();
        let mut out = Vec::with_capacity(rel_count as usize);
        for i in 0..rel_count_u64 {
            let off = records_offset + (rel_start + i) * 8;
            let kind = RelationshipKind::from_u32(read_u32(bytes, off)?)?;
            let value = read_u32(bytes, off + 4)?;
            match kind {
                RelationshipKind::SourceChunkId => out.push(SourceRef::ChunkId(value)),
                RelationshipKind::SourceString => {
                    let s = get_string(bytes, &self.string_dictionary, value as u64)?;
                    out.push(SourceRef::String(s));
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkView<'a> {
    pub id: u32,
    pub kind: &'a str,
    pub content: &'a str,
    pub author: &'a str,
    pub confidence: f32,
    pub created_at_unix_ms: u64,
    pub embedding_row: u32,
    pub rel_start: u64,
    pub rel_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRef<'a> {
    ChunkId(u32),
    String(&'a str),
}

pub struct ChunkIter<'a> {
    file: &'a LayerFile,
    index: u64,
}

impl<'a> Iterator for ChunkIter<'a> {
    type Item = Result<ChunkView<'a>, agentsdb_core::error::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.file.chunk_count {
            return None;
        }
        let idx = self.index;
        self.index += 1;
        Some(self.file.chunk_at(idx))
    }
}

impl LayerFile {
    fn chunk_at<'a>(&'a self, index: u64) -> Result<ChunkView<'a>, agentsdb_core::error::Error> {
        const RECORD_SIZE: u64 = 52;
        if index >= self.chunk_count {
            return Err(FormatError::InvalidRange {
                field: "chunk index",
            }
            .into());
        }

        let bytes = self.file_bytes();
        let off = self
            .chunk_records_offset
            .checked_add(
                index
                    .checked_mul(RECORD_SIZE)
                    .ok_or(FormatError::InvalidRange {
                        field: "chunk index",
                    })?,
            )
            .ok_or(FormatError::InvalidRange {
                field: "chunk index",
            })?;
        let record = parse_chunk_record(bytes, off)?;

        let kind = get_string(bytes, &self.string_dictionary, record.kind_str_id as u64)?;
        let content = get_string(bytes, &self.string_dictionary, record.content_str_id as u64)?;
        let author = get_string(bytes, &self.string_dictionary, record.author_str_id as u64)?;

        Ok(ChunkView {
            id: record.id,
            kind,
            content,
            author,
            confidence: record.confidence,
            created_at_unix_ms: record.created_at_unix_ms,
            embedding_row: record.embedding_row,
            rel_start: record.rel_start,
            rel_count: record.rel_count,
        })
    }
}

fn parse_file_header(bytes: &[u8]) -> Result<FileHeaderV1, FormatError> {
    let magic = read_u32(bytes, 0)?;
    if magic != MAGIC_AGDB {
        return Err(FormatError::BadMagic(magic));
    }
    Ok(FileHeaderV1 {
        magic,
        version_major: read_u16(bytes, 4)?,
        version_minor: read_u16(bytes, 6)?,
        file_length_bytes: read_u64(bytes, 8)?,
        section_count: read_u64(bytes, 16)?,
        sections_offset: read_u64(bytes, 24)?,
        flags: read_u64(bytes, 32)?,
    })
}

fn parse_section_table(
    bytes: &[u8],
    header: &FileHeaderV1,
) -> Result<Vec<SectionEntry>, FormatError> {
    const ENTRY_SIZE: u64 = 24;
    let count = header.section_count;
    let table_offset = header.sections_offset;
    let count_usize = usize::try_from(count).map_err(|_| FormatError::InvalidRange {
        field: "FileHeaderV1.section_count",
    })?;
    let total_len = count
        .checked_mul(ENTRY_SIZE)
        .ok_or(FormatError::InvalidRange {
            field: "FileHeaderV1.section_count",
        })?;
    let table_end = table_offset
        .checked_add(total_len)
        .ok_or(FormatError::InvalidRange {
            field: "FileHeaderV1.sections_offset",
        })?;
    if table_end > bytes.len() as u64 {
        return Err(FormatError::InvalidRange {
            field: "FileHeaderV1.sections_offset",
        });
    }

    let mut sections = Vec::with_capacity(count_usize);
    let mut required_seen = (false, false, false, false, false); // string, chunk, embed, rel, metadata
    for i in 0..count {
        let off = table_offset + i * ENTRY_SIZE;
        let kind_u32 = read_u32(bytes, off)?;
        let reserved = read_u32(bytes, off + 4)?;
        if reserved != 0 {
            return Err(FormatError::NonZeroReserved {
                field: "SectionEntry.reserved",
            });
        }
        let offset = read_u64(bytes, off + 8)?;
        let length = read_u64(bytes, off + 16)?;
        let kind = SectionKind::from_u32(kind_u32);

        let end = offset
            .checked_add(length)
            .ok_or(FormatError::InvalidRange {
                field: "SectionEntry.offset/length",
            })?;
        if end > bytes.len() as u64 {
            return Err(FormatError::InvalidRange { field: kind.name() });
        }

        match kind {
            SectionKind::StringDictionary => {
                if required_seen.0 {
                    return Err(FormatError::DuplicateSection("string_dictionary"));
                }
                required_seen.0 = true;
            }
            SectionKind::ChunkTable => {
                if required_seen.1 {
                    return Err(FormatError::DuplicateSection("chunk_table"));
                }
                required_seen.1 = true;
            }
            SectionKind::EmbeddingMatrix => {
                if required_seen.2 {
                    return Err(FormatError::DuplicateSection("embedding_matrix"));
                }
                required_seen.2 = true;
            }
            SectionKind::Relationships => {
                if required_seen.3 {
                    return Err(FormatError::DuplicateSection("relationships"));
                }
                required_seen.3 = true;
            }
            SectionKind::LayerMetadata => {
                if required_seen.4 {
                    return Err(FormatError::DuplicateSection("layer_metadata"));
                }
                required_seen.4 = true;
            }
            SectionKind::Unknown(_) => {}
        }

        sections.push(SectionEntry {
            kind,
            offset,
            length,
        });
    }

    if !required_seen.0 {
        return Err(FormatError::MissingSection("string_dictionary"));
    }
    if !required_seen.1 {
        return Err(FormatError::MissingSection("chunk_table"));
    }
    if !required_seen.2 {
        return Err(FormatError::MissingSection("embedding_matrix"));
    }

    Ok(sections)
}

fn required_section(
    sections: &[SectionEntry],
    kind: SectionKind,
) -> Result<SectionEntry, FormatError> {
    sections
        .iter()
        .find(|s| s.kind == kind)
        .copied()
        .ok_or(match kind {
            SectionKind::StringDictionary => FormatError::MissingSection("string_dictionary"),
            SectionKind::ChunkTable => FormatError::MissingSection("chunk_table"),
            SectionKind::EmbeddingMatrix => FormatError::MissingSection("embedding_matrix"),
            SectionKind::Relationships => FormatError::MissingSection("relationships"),
            SectionKind::LayerMetadata => FormatError::MissingSection("layer_metadata"),
            SectionKind::Unknown(_) => FormatError::MissingSection("unknown"),
        })
}

fn optional_section(
    sections: &[SectionEntry],
    kind: SectionKind,
) -> Result<Option<SectionEntry>, FormatError> {
    Ok(sections.iter().find(|s| s.kind == kind).copied())
}

fn parse_layer_metadata_header(
    bytes: &[u8],
    section: SectionEntry,
) -> Result<LayerMetadataHeaderV1, FormatError> {
    let base = section.offset;
    Ok(LayerMetadataHeaderV1 {
        version: read_u32(bytes, base)?,
        format: read_u32(bytes, base + 4)?,
        blob_offset: read_u64(bytes, base + 8)?,
        blob_length: read_u64(bytes, base + 16)?,
    })
}

fn validate_layer_metadata(
    bytes: &[u8],
    section: SectionEntry,
    hdr: &LayerMetadataHeaderV1,
) -> Result<(), FormatError> {
    if hdr.version != 1 {
        return Err(FormatError::InvalidValue {
            field: "LayerMetadataHeaderV1.version",
            reason: "unsupported metadata version",
        });
    }
    if hdr.format != 1 {
        return Err(FormatError::InvalidValue {
            field: "LayerMetadataHeaderV1.format",
            reason: "unknown metadata format",
        });
    }
    let header_len = 24u64;
    let blob_start = hdr.blob_offset;
    let blob_end = blob_start
        .checked_add(hdr.blob_length)
        .ok_or(FormatError::InvalidRange {
            field: "LayerMetadataHeaderV1.blob_offset/blob_length",
        })?;
    let section_end =
        section
            .offset
            .checked_add(section.length)
            .ok_or(FormatError::InvalidRange {
                field: "SECTION_LAYER_METADATA offset/length",
            })?;
    if section.length < header_len {
        return Err(FormatError::InvalidRange {
            field: "SECTION_LAYER_METADATA length",
        });
    }
    if blob_start != section.offset + header_len {
        return Err(FormatError::InvalidValue {
            field: "LayerMetadataHeaderV1.blob_offset",
            reason: "must equal section.offset + header_len",
        });
    }
    if blob_end != section_end {
        return Err(FormatError::InvalidValue {
            field: "LayerMetadataHeaderV1.blob_length",
            reason: "must equal section.length - header_len",
        });
    }
    if blob_end > bytes.len() as u64 {
        return Err(FormatError::InvalidRange {
            field: "LayerMetadataHeaderV1.blob_offset/blob_length",
        });
    }
    Ok(())
}

fn parse_string_dictionary_header(
    bytes: &[u8],
    section: SectionEntry,
) -> Result<StringDictionaryHeaderV1, FormatError> {
    let base = section.offset;
    Ok(StringDictionaryHeaderV1 {
        string_count: read_u64(bytes, base)?,
        entries_offset: read_u64(bytes, base + 8)?,
        bytes_offset: read_u64(bytes, base + 16)?,
        bytes_length: read_u64(bytes, base + 24)?,
    })
}

fn validate_string_dictionary(
    bytes: &[u8],
    section: SectionEntry,
    dict: &StringDictionaryHeaderV1,
) -> Result<(), FormatError> {
    const ENTRY_SIZE: u64 = 16;
    let section_start = section.offset;
    let section_end = section.offset + section.length;

    if dict.entries_offset < section_start {
        return Err(FormatError::InvalidRange {
            field: "StringDictionaryHeaderV1.entries_offset",
        });
    }
    let entries_len =
        dict.string_count
            .checked_mul(ENTRY_SIZE)
            .ok_or(FormatError::InvalidRange {
                field: "StringDictionaryHeaderV1.string_count",
            })?;
    let entries_end =
        dict.entries_offset
            .checked_add(entries_len)
            .ok_or(FormatError::InvalidRange {
                field: "StringDictionaryHeaderV1.entries_offset",
            })?;
    if entries_end > section_end {
        return Err(FormatError::InvalidRange {
            field: "StringDictionaryHeaderV1.entries_offset",
        });
    }

    if dict.bytes_offset < section_start {
        return Err(FormatError::InvalidRange {
            field: "StringDictionaryHeaderV1.bytes_offset",
        });
    }
    let bytes_end =
        dict.bytes_offset
            .checked_add(dict.bytes_length)
            .ok_or(FormatError::InvalidRange {
                field: "StringDictionaryHeaderV1.bytes_length",
            })?;
    if bytes_end > section_end {
        return Err(FormatError::InvalidRange {
            field: "StringDictionaryHeaderV1.bytes_offset/bytes_length",
        });
    }

    for i in 0..dict.string_count {
        let off = dict.entries_offset + i * ENTRY_SIZE;
        let entry = StringEntry {
            byte_offset: read_u64(bytes, off)?,
            byte_length: read_u64(bytes, off + 8)?,
        };
        let start =
            dict.bytes_offset
                .checked_add(entry.byte_offset)
                .ok_or(FormatError::InvalidRange {
                    field: "StringEntry.byte_offset",
                })?;
        let end = start
            .checked_add(entry.byte_length)
            .ok_or(FormatError::InvalidRange {
                field: "StringEntry.byte_length",
            })?;
        if end > bytes_end {
            return Err(FormatError::InvalidRange {
                field: "StringEntry.byte_offset/byte_length",
            });
        }
        let slice = slice_range(bytes, start, end)?;
        if std::str::from_utf8(slice).is_err() {
            return Err(FormatError::InvalidUtf8String { id: i + 1 });
        }
    }

    Ok(())
}

fn get_string<'a>(
    bytes: &'a [u8],
    dict: &StringDictionaryHeaderV1,
    id: u64,
) -> Result<&'a str, FormatError> {
    if id == 0 || id > dict.string_count {
        return Err(FormatError::InvalidStringId {
            id,
            count: dict.string_count,
        });
    }
    let idx = id - 1;
    let off = dict.entries_offset + idx * 16;
    let entry = StringEntry {
        byte_offset: read_u64(bytes, off)?,
        byte_length: read_u64(bytes, off + 8)?,
    };
    let start =
        dict.bytes_offset
            .checked_add(entry.byte_offset)
            .ok_or(FormatError::InvalidRange {
                field: "StringEntry.byte_offset",
            })?;
    let end = start
        .checked_add(entry.byte_length)
        .ok_or(FormatError::InvalidRange {
            field: "StringEntry.byte_length",
        })?;
    let slice = slice_range(bytes, start, end)?;
    std::str::from_utf8(slice).map_err(|_| FormatError::InvalidUtf8String { id })
}

fn parse_chunk_table_header(
    bytes: &[u8],
    section: SectionEntry,
) -> Result<ChunkTableHeaderV1, FormatError> {
    let base = section.offset;
    Ok(ChunkTableHeaderV1 {
        chunk_count: read_u64(bytes, base)?,
        records_offset: read_u64(bytes, base + 8)?,
    })
}

fn parse_chunk_record(bytes: &[u8], offset: u64) -> Result<ChunkRecord, FormatError> {
    Ok(ChunkRecord {
        id: read_u32(bytes, offset)?,
        kind_str_id: read_u32(bytes, offset + 4)?,
        content_str_id: read_u32(bytes, offset + 8)?,
        author_str_id: read_u32(bytes, offset + 12)?,
        confidence: read_f32(bytes, offset + 16)?,
        created_at_unix_ms: read_u64(bytes, offset + 20)?,
        embedding_row: read_u32(bytes, offset + 28)?,
        reserved0: read_u32(bytes, offset + 32)?,
        rel_start: read_u64(bytes, offset + 36)?,
        rel_count: read_u32(bytes, offset + 44)?,
        reserved1: read_u32(bytes, offset + 48)?,
    })
}

fn validate_chunk_records(
    bytes: &[u8],
    section: SectionEntry,
    chunk_header: &ChunkTableHeaderV1,
    dict: &StringDictionaryHeaderV1,
    embed: &EmbeddingMatrixHeaderV1,
    relationship_count: Option<u64>,
    allow_duplicate_ids: bool,
) -> Result<(), FormatError> {
    const RECORD_SIZE: u64 = 52;
    let section_start = section.offset;
    let section_end = section.offset + section.length;
    if chunk_header.records_offset < section_start {
        return Err(FormatError::InvalidRange {
            field: "ChunkTableHeaderV1.records_offset",
        });
    }
    let records_len =
        chunk_header
            .chunk_count
            .checked_mul(RECORD_SIZE)
            .ok_or(FormatError::InvalidRange {
                field: "ChunkTableHeaderV1.chunk_count",
            })?;
    let records_end =
        chunk_header
            .records_offset
            .checked_add(records_len)
            .ok_or(FormatError::InvalidRange {
                field: "ChunkTableHeaderV1.records_offset",
            })?;
    if records_end > section_end {
        return Err(FormatError::InvalidRange {
            field: "ChunkTableHeaderV1.records_offset",
        });
    }

    let mut ids = if !allow_duplicate_ids {
        Some(HashSet::with_capacity(chunk_header.chunk_count.min(1024) as usize))
    } else {
        None
    };

    for i in 0..chunk_header.chunk_count {
        let off = chunk_header.records_offset + i * RECORD_SIZE;
        let record = parse_chunk_record(bytes, off)?;

        if record.id == 0 {
            return Err(FormatError::InvalidChunkId(record.id));
        }
        if let Some(ref mut ids) = ids {
            if !ids.insert(record.id) {
                return Err(FormatError::DuplicateChunkId(record.id));
            }
        }

        let kind_id = record.kind_str_id as u64;
        let content_id = record.content_str_id as u64;
        let author_id = record.author_str_id as u64;
        if kind_id == 0 || kind_id > dict.string_count {
            return Err(FormatError::InvalidStringId {
                id: kind_id,
                count: dict.string_count,
            });
        }
        if content_id == 0 || content_id > dict.string_count {
            return Err(FormatError::InvalidStringId {
                id: content_id,
                count: dict.string_count,
            });
        }
        if author_id == 0 || author_id > dict.string_count {
            return Err(FormatError::InvalidStringId {
                id: author_id,
                count: dict.string_count,
            });
        }

        if !record.confidence.is_finite() || !(0.0..=1.0).contains(&record.confidence) {
            return Err(FormatError::InvalidValue {
                field: "ChunkRecord.confidence",
                reason: "must be finite and in range 0.0..=1.0",
            });
        }

        if record.embedding_row == 0 || record.embedding_row as u64 > embed.row_count {
            return Err(FormatError::InvalidEmbeddingRow {
                embedding_row: record.embedding_row,
                row_count: embed.row_count,
            });
        }

        if record.reserved0 != 0 {
            return Err(FormatError::NonZeroReserved {
                field: "ChunkRecord.reserved0",
            });
        }
        if record.reserved1 != 0 {
            return Err(FormatError::NonZeroReserved {
                field: "ChunkRecord.reserved1",
            });
        }

        let author = get_string(bytes, dict, author_id)?;
        if author != "human" && author != "mcp" {
            return Err(FormatError::InvalidAuthor {
                id: author_id,
                value: author.to_owned(),
            });
        }

        match relationship_count {
            None => {
                if record.rel_start != 0 || record.rel_count != 0 {
                    return Err(FormatError::InvalidValue {
                        field: "ChunkRecord.rel_start/rel_count",
                        reason: "must be 0 when relationships section is absent",
                    });
                }
            }
            Some(rel_count_total) => {
                let rel_start = record.rel_start;
                let rel_count = record.rel_count;
                let end = rel_start.checked_add(rel_count as u64).ok_or(
                    FormatError::InvalidRelationshipsRange {
                        rel_start,
                        rel_count,
                        relationship_count: rel_count_total,
                    },
                )?;
                if end > rel_count_total {
                    return Err(FormatError::InvalidRelationshipsRange {
                        rel_start,
                        rel_count,
                        relationship_count: rel_count_total,
                    });
                }
            }
        }
    }

    Ok(())
}

fn parse_embedding_matrix_header(
    bytes: &[u8],
    section: SectionEntry,
) -> Result<EmbeddingMatrixHeaderV1, FormatError> {
    let base = section.offset;
    Ok(EmbeddingMatrixHeaderV1 {
        row_count: read_u64(bytes, base)?,
        dim: read_u32(bytes, base + 8)?,
        element_type: EmbeddingElementType::from_u32(read_u32(bytes, base + 12)?)?,
        data_offset: read_u64(bytes, base + 16)?,
        data_length: read_u64(bytes, base + 24)?,
        quant_scale: read_f32(bytes, base + 32)?,
        reserved0: read_f32(bytes, base + 36)?,
    })
}

fn validate_embedding_matrix(
    bytes: &[u8],
    section: SectionEntry,
    header: &EmbeddingMatrixHeaderV1,
) -> Result<(), FormatError> {
    let section_start = section.offset;
    let section_end = section.offset + section.length;

    if header.dim == 0 {
        return Err(FormatError::InvalidValue {
            field: "EmbeddingMatrixHeaderV1.dim",
            reason: "must be non-zero",
        });
    }

    if header.data_offset < section_start {
        return Err(FormatError::InvalidRange {
            field: "EmbeddingMatrixHeaderV1.data_offset",
        });
    }
    let data_end =
        header
            .data_offset
            .checked_add(header.data_length)
            .ok_or(FormatError::InvalidRange {
                field: "EmbeddingMatrixHeaderV1.data_length",
            })?;
    if data_end > section_end {
        return Err(FormatError::InvalidRange {
            field: "EmbeddingMatrixHeaderV1.data_offset/data_length",
        });
    }

    if header.reserved0 != 0.0 {
        return Err(FormatError::NonZeroReserved {
            field: "EmbeddingMatrixHeaderV1.reserved0",
        });
    }

    match header.element_type {
        EmbeddingElementType::F32 => {
            if header.quant_scale != 1.0 {
                return Err(FormatError::InvalidValue {
                    field: "EmbeddingMatrixHeaderV1.quant_scale",
                    reason: "must be 1.0 for EMBED_F32",
                });
            }
        }
        EmbeddingElementType::I8 => {
            if !header.quant_scale.is_finite() || header.quant_scale == 0.0 {
                return Err(FormatError::InvalidValue {
                    field: "EmbeddingMatrixHeaderV1.quant_scale",
                    reason: "must be finite and non-zero for EMBED_I8",
                });
            }
        }
    }

    let expected = header
        .row_count
        .checked_mul(header.dim as u64)
        .and_then(|v| v.checked_mul(header.element_type.size_bytes()))
        .ok_or(FormatError::InvalidRange {
            field: "EmbeddingMatrixHeaderV1.row_count/dim",
        })?;
    if header.data_length != expected {
        return Err(FormatError::InvalidValue {
            field: "EmbeddingMatrixHeaderV1.data_length",
            reason: "does not match row_count * dim * element_size",
        });
    }

    // Touch the end to ensure bounds are correct.
    let _ = slice_range(
        bytes,
        header.data_offset,
        header.data_offset + header.data_length,
    )?;

    Ok(())
}

fn parse_relationships_header(
    bytes: &[u8],
    section: SectionEntry,
) -> Result<RelationshipsHeaderV1, FormatError> {
    let base = section.offset;
    Ok(RelationshipsHeaderV1 {
        relationship_count: read_u64(bytes, base)?,
        records_offset: read_u64(bytes, base + 8)?,
    })
}

fn validate_relationships(
    bytes: &[u8],
    section: SectionEntry,
    header: &RelationshipsHeaderV1,
    dict: &StringDictionaryHeaderV1,
) -> Result<(), FormatError> {
    const RECORD_SIZE: u64 = 8;
    let section_start = section.offset;
    let section_end = section.offset + section.length;

    if header.records_offset < section_start {
        return Err(FormatError::InvalidRange {
            field: "RelationshipsHeaderV1.records_offset",
        });
    }
    let records_len =
        header
            .relationship_count
            .checked_mul(RECORD_SIZE)
            .ok_or(FormatError::InvalidRange {
                field: "RelationshipsHeaderV1.relationship_count",
            })?;
    let records_end =
        header
            .records_offset
            .checked_add(records_len)
            .ok_or(FormatError::InvalidRange {
                field: "RelationshipsHeaderV1.records_offset",
            })?;
    if records_end > section_end {
        return Err(FormatError::InvalidRange {
            field: "RelationshipsHeaderV1.records_offset",
        });
    }

    for i in 0..header.relationship_count {
        let off = header.records_offset + i * RECORD_SIZE;
        let kind = RelationshipKind::from_u32(read_u32(bytes, off)?)?;
        let value_u32 = read_u32(bytes, off + 4)?;
        match kind {
            RelationshipKind::SourceChunkId => {
                if value_u32 == 0 {
                    return Err(FormatError::InvalidValue {
                        field: "RelationshipRecord.value_u32",
                        reason: "chunk id must be non-zero",
                    });
                }
            }
            RelationshipKind::SourceString => {
                let id = value_u32 as u64;
                if id == 0 || id > dict.string_count {
                    return Err(FormatError::InvalidStringId {
                        id,
                        count: dict.string_count,
                    });
                }
            }
        }
    }

    Ok(())
}

fn slice_range(bytes: &[u8], start: u64, end: u64) -> Result<&[u8], FormatError> {
    if end < start {
        return Err(FormatError::InvalidRange { field: "range" });
    }
    let start_usize =
        usize::try_from(start).map_err(|_| FormatError::InvalidRange { field: "range" })?;
    let end_usize =
        usize::try_from(end).map_err(|_| FormatError::InvalidRange { field: "range" })?;
    if end_usize > bytes.len() {
        return Err(FormatError::InvalidRange { field: "range" });
    }
    Ok(&bytes[start_usize..end_usize])
}

fn read_exact<const N: usize>(bytes: &[u8], offset: u64) -> Result<[u8; N], FormatError> {
    let start =
        usize::try_from(offset).map_err(|_| FormatError::InvalidRange { field: "offset" })?;
    let end = start
        .checked_add(N)
        .ok_or(FormatError::InvalidRange { field: "offset" })?;
    if end > bytes.len() {
        return Err(FormatError::Truncated {
            at: offset,
            needed: N,
        });
    }
    Ok(bytes[start..end].try_into().unwrap())
}

fn read_u16(bytes: &[u8], offset: u64) -> Result<u16, FormatError> {
    Ok(u16::from_le_bytes(read_exact::<2>(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: u64) -> Result<u32, FormatError> {
    Ok(u32::from_le_bytes(read_exact::<4>(bytes, offset)?))
}

fn read_u64(bytes: &[u8], offset: u64) -> Result<u64, FormatError> {
    Ok(u64::from_le_bytes(read_exact::<8>(bytes, offset)?))
}

fn read_f32(bytes: &[u8], offset: u64) -> Result<f32, FormatError> {
    Ok(f32::from_le_bytes(read_exact::<4>(bytes, offset)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u16(buf: &mut [u8], offset: usize, v: u16) {
        buf[offset..offset + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u32(buf: &mut [u8], offset: usize, v: u32) {
        buf[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u64(buf: &mut [u8], offset: usize, v: u64) {
        buf[offset..offset + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn put_f32(buf: &mut [u8], offset: usize, v: f32) {
        buf[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn build_minimal_valid_file() -> Vec<u8> {
        // Layout:
        // header (40 bytes) + section table (3 * 24 = 72) = 112 bytes
        // string section at 112
        // chunk section after string
        // embedding section after chunk

        let strings: [&str; 4] = ["human", "mcp", "note", "hello"];
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
        let chunk_count = 1u64;
        let chunk_section_len = chunk_header_size + chunk_count * chunk_record_size;

        let embed_header_size = 40u64;
        let row_count = 1u64;
        let dim = 2u32;
        let embed_data_len = row_count * dim as u64 * 4;
        let embed_section_len = embed_header_size + embed_data_len;

        let header_len = 40u64;
        let section_table_len = 3u64 * 24;
        let string_section_off = header_len + section_table_len;
        let chunk_section_off = string_section_off + string_section_len;
        let embed_section_off = chunk_section_off + chunk_section_len;
        let file_len = embed_section_off + embed_section_len;

        let mut buf = vec![0u8; file_len as usize];

        // Header
        put_u32(&mut buf, 0, MAGIC_AGDB);
        put_u16(&mut buf, 4, 1);
        put_u16(&mut buf, 6, 0);
        put_u64(&mut buf, 8, file_len);
        put_u64(&mut buf, 16, 3);
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

        // ChunkTableHeaderV1
        put_u64(&mut buf, chunk_section_off as usize, chunk_count);
        let chunk_records_off = chunk_section_off + chunk_header_size;
        put_u64(&mut buf, chunk_section_off as usize + 8, chunk_records_off);
        // ChunkRecord
        let rec_off = chunk_records_off as usize;
        put_u32(&mut buf, rec_off, 1); // id
        put_u32(&mut buf, rec_off + 4, 3); // kind_str_id -> "note"
        put_u32(&mut buf, rec_off + 8, 4); // content_str_id -> "hello"
        put_u32(&mut buf, rec_off + 12, 1); // author_str_id -> "human"
        put_f32(&mut buf, rec_off + 16, 1.0);
        put_u64(&mut buf, rec_off + 20, 0);
        put_u32(&mut buf, rec_off + 28, 1); // embedding_row (1-based)
        put_u32(&mut buf, rec_off + 32, 0);
        put_u64(&mut buf, rec_off + 36, 0);
        put_u32(&mut buf, rec_off + 44, 0);
        put_u32(&mut buf, rec_off + 48, 0);

        // EmbeddingMatrixHeaderV1
        put_u64(&mut buf, embed_section_off as usize, row_count);
        put_u32(&mut buf, embed_section_off as usize + 8, dim);
        put_u32(&mut buf, embed_section_off as usize + 12, 1); // EMBED_F32
        let embed_data_off = embed_section_off + embed_header_size;
        put_u64(&mut buf, embed_section_off as usize + 16, embed_data_off);
        put_u64(&mut buf, embed_section_off as usize + 24, embed_data_len);
        put_f32(&mut buf, embed_section_off as usize + 32, 1.0);
        put_f32(&mut buf, embed_section_off as usize + 36, 0.0);
        // data (2 f32)
        put_f32(&mut buf, embed_data_off as usize, 0.0);
        put_f32(&mut buf, embed_data_off as usize + 4, 1.0);

        buf
    }

    #[test]
    fn opens_minimal_valid_file() {
        let data = build_minimal_valid_file();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AGENTS.db");
        std::fs::write(&path, &data).unwrap();
        let file = LayerFile::open(&path).unwrap();
        assert_eq!(file.header.version_major, 1);
        assert_eq!(file.string_dictionary.string_count, 4);
        assert_eq!(file.chunk_count, 1);
        assert_eq!(file.embedding_matrix.row_count, 1);
        assert_eq!(file.embedding_matrix.dim, 2);
        assert_eq!(file.relationship_count, None);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut data = build_minimal_valid_file();
        data[0..4].copy_from_slice(&0u32.to_le_bytes());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.db");
        std::fs::write(&path, &data).unwrap();
        let err = LayerFile::open(&path).unwrap_err().to_string();
        assert!(err.contains("bad magic"));
    }

    #[test]
    fn rejects_relationships_fields_when_section_missing() {
        let mut data = build_minimal_valid_file();
        // Set rel_count to 1 in chunk record
        // Find chunk record offset: header 40 + sections 72 + string section (computed) + chunk header (16) = records start
        // We'll locate by parsing the file itself for robustness.
        let header = parse_file_header(&data).unwrap();
        let sections = parse_section_table(&data, &header).unwrap();
        let chunk_section = required_section(&sections, SectionKind::ChunkTable).unwrap();
        let chunk_header = parse_chunk_table_header(&data, chunk_section).unwrap();
        let rec_off = chunk_header.records_offset as usize;
        put_u32(&mut data, rec_off + 44, 1);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad_rel.db");
        std::fs::write(&path, &data).unwrap();
        let err = LayerFile::open(&path).unwrap_err().to_string();
        assert!(err.contains("relationships section is absent"));
    }
}
