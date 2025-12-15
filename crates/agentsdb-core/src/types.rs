#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ChunkId(pub u32);

impl ChunkId {
    /// Represents a unique identifier for a chunk of data within an AGENTS.db layer.
    pub fn get(self) -> u32 {
        self.0
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LayerId {
    /// Represents the identifier for different types of AGENTS.db layers.
    ///
    /// The variants are ordered by precedence, with `Local` having the highest precedence.
    // Ord is used for deterministic tie-breaks; variants are in precedence order.
    Local,
    User,
    Delta,
    Base,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Author {
    /// Represents the author of a chunk, either a human or an MCP agent.
    Human,
    Mcp,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProvenanceRef {
    /// Represents a reference to the origin or source of a chunk.
    ///
    /// This can be either a reference to another `ChunkId` or a free-form source string.
    ChunkId(ChunkId),
    SourceString(String),
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Represents a single unit of data (a "chunk") stored in an AGENTS.db layer.
    ///
    /// This struct contains the chunk's ID, kind, content, author, confidence,
    /// creation timestamp, and references to its sources.
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
    /// Represents criteria for filtering search results.
    ///
    /// Currently, this includes filtering by chunk `kind`.
    pub kinds: Vec<String>,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Represents a single result from a search operation across AGENTS.db layers.
    ///
    /// This includes the layer where the chunk was found, its similarity score, the chunk itself,
    /// and any layers that were hidden due to precedence.
    pub layer: LayerId,
    pub score: f32,
    pub chunk: Chunk,
    pub hidden_layers: Vec<LayerId>,
}
