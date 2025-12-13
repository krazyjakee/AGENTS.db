#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ChunkId(pub u32);

impl ChunkId {
    pub fn get(self) -> u32 {
        self.0
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LayerId {
    // Ord is used for deterministic tie-breaks; variants are in precedence order.
    Local,
    User,
    Delta,
    Base,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Author {
    Human,
    Mcp,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProvenanceRef {
    ChunkId(ChunkId),
    SourceString(String),
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: ChunkId,
    pub kind: String,
    pub content: String,
    pub author: Author,
    pub confidence: f32,
    pub created_at_unix_ms: u64,
    pub sources: Vec<ProvenanceRef>,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    pub kinds: Vec<String>,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub layer: LayerId,
    pub score: f32,
    pub chunk: Chunk,
    pub hidden_layers: Vec<LayerId>,
}
