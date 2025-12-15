use agentsdb_core::error::{Error, FormatError};
use agentsdb_embeddings::cache::sha256;
use agentsdb_format::{EmbeddingElementType, LayerFile};
use memmap2::Mmap;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAGIC_AGIX: u32 = 0x5849_4741; // 'A' 'G' 'I' 'X'

#[derive(Debug, Clone, Copy)]
pub struct IndexBuildOptions {
    /// Store decoded f32 embeddings even for f32 layers (default false).
    pub store_embeddings_even_if_f32: bool,
}

#[derive(Debug)]
pub struct LayerIndex {
    _path: PathBuf,
    mmap: Mmap,
    dim: u32,
    row_count: u64,
    element_type: EmbeddingElementType,
    quant_scale_bits: u32,
    has_embeddings: bool,
    norms_offset: u64,
    norms_len: u64,
    embeds_offset: u64,
    embeds_len: u64,
}

impl LayerIndex {
    pub fn open(
        path: impl AsRef<Path>,
        expected_layer_sha256: [u8; 32],
    ) -> Result<Option<Self>, Error> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let bytes = mmap.as_ref();

        let hdr = parse_header(bytes)?;
        if hdr.layer_sha256 != expected_layer_sha256 {
            return Ok(None);
        }

        let has_embeddings = (hdr.flags & 1) != 0;
        let element_type = match hdr.element_type {
            1 => EmbeddingElementType::F32,
            2 => EmbeddingElementType::I8,
            _ => {
                return Err(FormatError::InvalidValue {
                    field: "AGIX.header.element_type",
                    reason: "unknown embedding element type",
                }
                .into());
            }
        };

        validate_ranges(bytes, &hdr)?;

        Ok(Some(Self {
            _path: path,
            mmap,
            dim: hdr.dim,
            row_count: hdr.row_count,
            element_type,
            quant_scale_bits: hdr.quant_scale_bits,
            has_embeddings,
            norms_offset: hdr.norms_offset,
            norms_len: hdr.norms_len,
            embeds_offset: hdr.embeds_offset,
            embeds_len: hdr.embeds_len,
        }))
    }

    pub fn row_f32_and_norm(&self, embedding_row: u32) -> Result<(f32, Option<&[f32]>), Error> {
        if embedding_row == 0 || embedding_row as u64 > self.row_count {
            return Err(FormatError::InvalidEmbeddingRow {
                embedding_row,
                row_count: self.row_count,
            }
            .into());
        }
        let idx0 = (embedding_row as usize) - 1;
        let bytes = self.mmap.as_ref();

        let norms = norms_slice(bytes, self.norms_offset, self.norms_len)?;
        let row_norm = norms[idx0];

        if !self.has_embeddings {
            return Ok((row_norm, None));
        }
        let embeds = embeds_slice(bytes, self.embeds_offset, self.embeds_len)?;
        let dim = self.dim as usize;
        let start = idx0.checked_mul(dim).ok_or(FormatError::InvalidRange {
            field: "AGIX.embeddings range",
        })?;
        let end = start.checked_add(dim).ok_or(FormatError::InvalidRange {
            field: "AGIX.embeddings range",
        })?;
        Ok((row_norm, Some(&embeds[start..end])))
    }
}

#[derive(Debug)]
pub struct IndexLookup {
    by_layer: HashMap<agentsdb_core::types::LayerId, LayerIndex>,
}

impl IndexLookup {
    pub fn empty() -> Self {
        Self {
            by_layer: HashMap::new(),
        }
    }

    pub fn open_for_layers(
        layers: &[(agentsdb_core::types::LayerId, LayerFile)],
    ) -> Result<Self, Error> {
        let mut by_layer = HashMap::new();
        for (id, layer) in layers {
            let idx_path = default_index_path_for_layer(layer.path());
            let layer_sha = sha256(layer.file_bytes());
            if let Some(index) = LayerIndex::open(idx_path, layer_sha)? {
                // Index must match schema; otherwise treat as stale/missing.
                if index.dim != layer.embedding_matrix.dim {
                    continue;
                }
                if index.element_type != layer.embedding_matrix.element_type {
                    continue;
                }
                if index.quant_scale_bits != layer.embedding_matrix.quant_scale.to_bits() {
                    continue;
                }
                if index.row_count != layer.embedding_matrix.row_count {
                    continue;
                }
                by_layer.insert(*id, index);
            }
        }
        Ok(Self { by_layer })
    }

    pub fn index_for(&self, layer: agentsdb_core::types::LayerId) -> Option<&LayerIndex> {
        self.by_layer.get(&layer)
    }
}

pub fn default_index_path_for_layer(layer_path: impl AsRef<Path>) -> PathBuf {
    let layer_path = layer_path.as_ref();
    PathBuf::from(format!("{}.agix", layer_path.display()))
}

pub fn build_layer_index(
    layer: &LayerFile,
    out_path: impl AsRef<Path>,
    opts: IndexBuildOptions,
) -> Result<(), Error> {
    let out_path = out_path.as_ref();

    let dim = layer.embedding_matrix.dim;
    let row_count = layer.embedding_matrix.row_count;
    let element_type = layer.embedding_matrix.element_type;
    let quant_scale_bits = layer.embedding_matrix.quant_scale.to_bits();
    let layer_sha = sha256(layer.file_bytes());

    let store_embeddings =
        matches!(element_type, EmbeddingElementType::I8) || opts.store_embeddings_even_if_f32;

    let mut norms: Vec<f32> = vec![0.0; row_count as usize];
    let mut embeddings: Vec<f32> = if store_embeddings {
        vec![0.0; (row_count as usize) * (dim as usize)]
    } else {
        Vec::new()
    };

    let mut tmp = vec![0.0f32; dim as usize];
    for row in 1..=row_count {
        layer.read_embedding_row_f32(row as u32, &mut tmp)?;
        let mut sum = 0.0f32;
        for v in &tmp {
            sum += v * v;
        }
        norms[(row as usize) - 1] = sum.sqrt();
        if store_embeddings {
            let dst_off = ((row as usize) - 1) * (dim as usize);
            embeddings[dst_off..dst_off + (dim as usize)].copy_from_slice(&tmp);
        }
    }

    let flags: u32 = if store_embeddings { 1 } else { 0 };
    let header_len: u64 = 104;
    let norms_offset = header_len;
    let norms_len = (row_count as u64)
        .checked_mul(4)
        .ok_or(FormatError::InvalidRange {
            field: "AGIX.norms_len",
        })?;
    let embeds_offset = norms_offset
        .checked_add(norms_len)
        .ok_or(FormatError::InvalidRange {
            field: "AGIX.embeds_offset",
        })?;
    let embeds_len = if store_embeddings {
        (row_count as u64)
            .checked_mul(dim as u64)
            .and_then(|v| v.checked_mul(4))
            .ok_or(FormatError::InvalidRange {
                field: "AGIX.embeds_len",
            })?
    } else {
        0
    };

    let mut buf = Vec::with_capacity((header_len + norms_len + embeds_len).try_into().map_err(
        |_| FormatError::InvalidRange {
            field: "AGIX.buffer",
        },
    )?);

    // Header
    push_u32(&mut buf, MAGIC_AGIX);
    push_u16(&mut buf, 1);
    push_u16(&mut buf, 0);
    push_u32(&mut buf, dim);
    push_u32(&mut buf, 0);
    push_u64(&mut buf, row_count);
    push_u32(
        &mut buf,
        match element_type {
            EmbeddingElementType::F32 => 1,
            EmbeddingElementType::I8 => 2,
        },
    );
    push_u32(&mut buf, flags);
    push_u32(&mut buf, quant_scale_bits);
    push_u32(&mut buf, 0);
    buf.extend_from_slice(&layer_sha);
    push_u64(&mut buf, norms_offset);
    push_u64(&mut buf, norms_len);
    push_u64(&mut buf, embeds_offset);
    push_u64(&mut buf, embeds_len);
    debug_assert_eq!(buf.len() as u64, header_len);

    // Norms
    for v in &norms {
        push_f32(&mut buf, *v);
    }

    // Embeddings (optional)
    if store_embeddings {
        for v in &embeddings {
            push_f32(&mut buf, *v);
        }
    }

    write_atomic(out_path, &buf)?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut tmp = parent.to_path_buf();
    tmp.push(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("agentsdb-index"),
        std::process::id(),
    ));
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct IndexHeaderV1 {
    dim: u32,
    row_count: u64,
    element_type: u32,
    flags: u32,
    quant_scale_bits: u32,
    layer_sha256: [u8; 32],
    norms_offset: u64,
    norms_len: u64,
    embeds_offset: u64,
    embeds_len: u64,
}

fn parse_header(bytes: &[u8]) -> Result<IndexHeaderV1, Error> {
    let mut off = 0usize;
    let magic = read_u32(bytes, &mut off)?;
    if magic != MAGIC_AGIX {
        return Err(FormatError::InvalidValue {
            field: "AGIX.magic",
            reason: "bad magic",
        }
        .into());
    }
    let major = read_u16(bytes, &mut off)?;
    let minor = read_u16(bytes, &mut off)?;
    if major != 1 || minor != 0 {
        return Err(FormatError::UnsupportedVersion { major, minor }.into());
    }
    let dim = read_u32(bytes, &mut off)?;
    let _reserved0 = read_u32(bytes, &mut off)?;
    let row_count = read_u64(bytes, &mut off)?;
    let element_type = read_u32(bytes, &mut off)?;
    let flags = read_u32(bytes, &mut off)?;
    let quant_scale_bits = read_u32(bytes, &mut off)?;
    let reserved1 = read_u32(bytes, &mut off)?;
    if reserved1 != 0 {
        return Err(FormatError::NonZeroReserved {
            field: "AGIX.header.reserved1",
        }
        .into());
    }
    let layer_sha256 = read_bytes_32(bytes, &mut off)?;
    let norms_offset = read_u64(bytes, &mut off)?;
    let norms_len = read_u64(bytes, &mut off)?;
    let embeds_offset = read_u64(bytes, &mut off)?;
    let embeds_len = read_u64(bytes, &mut off)?;
    Ok(IndexHeaderV1 {
        dim,
        row_count,
        element_type,
        flags,
        quant_scale_bits,
        layer_sha256,
        norms_offset,
        norms_len,
        embeds_offset,
        embeds_len,
    })
}

fn validate_ranges(bytes: &[u8], hdr: &IndexHeaderV1) -> Result<(), Error> {
    let file_len = bytes.len() as u64;
    // norms
    let norms_end =
        hdr.norms_offset
            .checked_add(hdr.norms_len)
            .ok_or(FormatError::InvalidRange {
                field: "AGIX.norms",
            })?;
    if norms_end > file_len {
        return Err(FormatError::InvalidRange {
            field: "AGIX.norms",
        }
        .into());
    }
    let expected_norms_len = hdr
        .row_count
        .checked_mul(4)
        .ok_or(FormatError::InvalidRange {
            field: "AGIX.expected_norms_len",
        })?;
    if hdr.norms_len != expected_norms_len {
        return Err(FormatError::InvalidValue {
            field: "AGIX.norms_len",
            reason: "unexpected norms length",
        }
        .into());
    }

    let has_embeddings = (hdr.flags & 1) != 0;
    if has_embeddings {
        let embeds_end =
            hdr.embeds_offset
                .checked_add(hdr.embeds_len)
                .ok_or(FormatError::InvalidRange {
                    field: "AGIX.embeddings",
                })?;
        if embeds_end > file_len {
            return Err(FormatError::InvalidRange {
                field: "AGIX.embeddings",
            }
            .into());
        }
        let expected_embeds_len = hdr
            .row_count
            .checked_mul(hdr.dim as u64)
            .and_then(|v| v.checked_mul(4))
            .ok_or(FormatError::InvalidRange {
                field: "AGIX.expected_embeds_len",
            })?;
        if hdr.embeds_len != expected_embeds_len {
            return Err(FormatError::InvalidValue {
                field: "AGIX.embeds_len",
                reason: "unexpected embeddings length",
            }
            .into());
        }
    } else if hdr.embeds_len != 0 {
        return Err(FormatError::InvalidValue {
            field: "AGIX.embeds_len",
            reason: "must be 0 when embeddings are not present",
        }
        .into());
    }

    Ok(())
}

fn norms_slice<'a>(bytes: &'a [u8], off: u64, len: u64) -> Result<&'a [f32], Error> {
    if off % 4 != 0 || len % 4 != 0 {
        return Err(FormatError::InvalidRange {
            field: "AGIX.norms alignment",
        }
        .into());
    }
    let start = off as usize;
    let end = start
        .checked_add(len as usize)
        .ok_or(FormatError::InvalidRange {
            field: "AGIX.norms slice",
        })?;
    let bytes = bytes.get(start..end).ok_or(FormatError::InvalidRange {
        field: "AGIX.norms slice",
    })?;
    let (prefix, body, suffix) = unsafe { bytes.align_to::<f32>() };
    if !prefix.is_empty() || !suffix.is_empty() {
        return Err(FormatError::InvalidRange {
            field: "AGIX.norms slice alignment",
        }
        .into());
    }
    Ok(body)
}

fn embeds_slice<'a>(bytes: &'a [u8], off: u64, len: u64) -> Result<&'a [f32], Error> {
    if off % 4 != 0 || len % 4 != 0 {
        return Err(FormatError::InvalidRange {
            field: "AGIX.embeddings alignment",
        }
        .into());
    }
    let start = off as usize;
    let end = start
        .checked_add(len as usize)
        .ok_or(FormatError::InvalidRange {
            field: "AGIX.embeddings slice",
        })?;
    let bytes = bytes.get(start..end).ok_or(FormatError::InvalidRange {
        field: "AGIX.embeddings slice",
    })?;
    let (prefix, body, suffix) = unsafe { bytes.align_to::<f32>() };
    if !prefix.is_empty() || !suffix.is_empty() {
        return Err(FormatError::InvalidRange {
            field: "AGIX.embeddings slice alignment",
        }
        .into());
    }
    Ok(body)
}

fn read_u16(bytes: &[u8], off: &mut usize) -> Result<u16, Error> {
    let start = *off;
    let end = start + 2;
    let slice = bytes.get(start..end).ok_or(FormatError::Truncated {
        at: start as u64,
        needed: 2,
    })?;
    *off = end;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], off: &mut usize) -> Result<u32, Error> {
    let start = *off;
    let end = start + 4;
    let slice = bytes.get(start..end).ok_or(FormatError::Truncated {
        at: start as u64,
        needed: 4,
    })?;
    *off = end;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], off: &mut usize) -> Result<u64, Error> {
    let start = *off;
    let end = start + 8;
    let slice = bytes.get(start..end).ok_or(FormatError::Truncated {
        at: start as u64,
        needed: 8,
    })?;
    *off = end;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

fn read_bytes_32(bytes: &[u8], off: &mut usize) -> Result<[u8; 32], Error> {
    let start = *off;
    let end = start + 32;
    let slice = bytes.get(start..end).ok_or(FormatError::Truncated {
        at: start as u64,
        needed: 32,
    })?;
    *off = end;
    let mut out = [0u8; 32];
    out.copy_from_slice(slice);
    Ok(out)
}

fn push_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_f32(buf: &mut Vec<u8>, v: f32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
