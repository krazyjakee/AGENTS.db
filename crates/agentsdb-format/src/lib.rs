mod reader;
pub mod writer;

pub use reader::{
    ChunkView, EmbeddingElementType, EmbeddingMatrixHeaderV1, FileHeaderV1, LayerFile,
    RelationshipKind, SectionEntry, SectionKind, SourceRef, StringDictionaryHeaderV1,
};

pub use writer::{
    append_layer_atomic, ensure_writable_layer_path, ensure_writable_layer_path_allow_user,
    read_all_chunks, schema_of, write_layer_atomic, ChunkInput, ChunkSource, LayerSchema,
};
