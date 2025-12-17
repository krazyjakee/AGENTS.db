use anyhow::Result;

use crate::embedder::{Embedder, EmbeddingProfile, OutputNorm};

pub struct HashEmbedder {
    profile: EmbeddingProfile,
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        Self {
            profile: EmbeddingProfile {
                backend: "hash".to_string(),
                model: None,
                revision: None,
                dim,
                output_norm: OutputNorm::None,
            },
        }
    }
}

impl Embedder for HashEmbedder {
    fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(inputs
            .iter()
            .map(|s| agentsdb_core::embed::hash_embed(s, self.profile.dim))
            .collect())
    }
}
