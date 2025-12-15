use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// Represents all possible errors that can occur within the `agentsdb-core` crate.
    ///
    /// This enum consolidates various error types, making error handling more consistent.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Format(#[from] FormatError),

    #[error(transparent)]
    Schema(#[from] SchemaError),

    #[error(transparent)]
    Permission(#[from] PermissionError),
}

#[derive(Debug, Error)]
pub enum FormatError {
    /// Represents errors specifically related to the AGENTS.db file format.
    ///
    /// These errors typically occur during parsing or validation of a `.db` file.
    #[error("truncated input at byte {at}, need {needed} bytes")]
    Truncated { at: u64, needed: usize },

    #[error("bad magic: expected 0x42444741, got 0x{0:08x}")]
    BadMagic(u32),

    #[error("unsupported version: {major}.{minor}")]
    UnsupportedVersion { major: u16, minor: u16 },

    #[error("non-zero reserved field: {field}")]
    NonZeroReserved { field: &'static str },

    #[error("invalid value for {field}: {reason}")]
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },

    #[error("invalid offset/length for {field}")]
    InvalidRange { field: &'static str },

    #[error("missing required section: {0}")]
    MissingSection(&'static str),

    #[error("duplicate section: {0}")]
    DuplicateSection(&'static str),

    #[error("invalid string id {id} (count {count})")]
    InvalidStringId { id: u64, count: u64 },

    #[error("invalid chunk id: {0}")]
    InvalidChunkId(u32),

    #[error("duplicate chunk id: {0}")]
    DuplicateChunkId(u32),

    #[error("invalid embedding_row {embedding_row} (row_count {row_count})")]
    InvalidEmbeddingRow { embedding_row: u32, row_count: u64 },

    #[error("invalid relationships range start={rel_start} count={rel_count} (relationship_count {relationship_count})")]
    InvalidRelationshipsRange {
        rel_start: u64,
        rel_count: u32,
        relationship_count: u64,
    },

    #[error("invalid utf-8 string (id {id})")]
    InvalidUtf8String { id: u64 },

    #[error("invalid author string (id {id}): {value:?}")]
    InvalidAuthor { id: u64, value: String },

    #[error("file length mismatch: header {header} bytes, actual {actual} bytes")]
    FileLengthMismatch { header: u64, actual: u64 },
}

#[derive(Debug, Error)]
pub enum SchemaError {
    /// Represents errors related to schema mismatches between layers.
    ///
    /// This typically occurs when attempting to combine or operate on layers with incompatible schemas.
    #[error("schema mismatch: {0}")]
    Mismatch(&'static str),
}

#[derive(Debug, Error)]
pub enum PermissionError {
    /// Represents errors related to write permissions for AGENTS.db layers.
    ///
    /// This error occurs when an attempt is made to write to a layer that is not designated as writable.
    #[error("writes are not permitted to {path:?}")]
    WriteNotPermitted { path: PathBuf },
}
