use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::util::now_unix_ms;

/// Default decay TTL: 30 days in milliseconds.
const DEFAULT_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Sidecar file name for decay state.
const DECAY_FILE: &str = "AGENTS.decay.json";

/// Persisted decay state: maps chunk keys to their last-accessed time.
///
/// Chunk keys are `"{layer}:{chunk_id}"` so that IDs from different layers don't collide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayState {
    /// Decay TTL in milliseconds.  Chunks not accessed within this window are considered decayed.
    pub ttl_ms: u64,
    /// Map of `"layer:chunk_id"` → last-accessed unix-ms timestamp.
    pub accessed: HashMap<String, u64>,
}

impl Default for DecayState {
    fn default() -> Self {
        Self {
            ttl_ms: DEFAULT_TTL_MS,
            accessed: HashMap::new(),
        }
    }
}

impl DecayState {
    /// Build the sidecar file path given the project root directory.
    pub fn path_for(root: &Path) -> PathBuf {
        root.join(DECAY_FILE)
    }

    /// Load from disk, returning default if the file doesn't exist.
    pub fn load(root: &Path) -> Self {
        let path = Self::path_for(root);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist to disk.
    pub fn save(&self, root: &Path) -> anyhow::Result<()> {
        let path = Self::path_for(root);
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Record an access for a chunk, refreshing its decay timer.
    pub fn touch(&mut self, layer: &str, chunk_id: u32) {
        let key = format!("{layer}:{chunk_id}");
        self.accessed.insert(key, now_unix_ms());
    }

    /// Record accesses for multiple chunks at once.
    pub fn touch_many(&mut self, items: &[(String, u32)]) {
        let now = now_unix_ms();
        for (layer, chunk_id) in items {
            let key = format!("{layer}:{chunk_id}");
            self.accessed.insert(key, now);
        }
    }

    /// Check whether a chunk has decayed (not accessed within the TTL window).
    ///
    /// Chunks that have never been accessed are evaluated against their `created_at_unix_ms`.
    pub fn is_decayed(&self, layer: &str, chunk_id: u32, created_at_unix_ms: u64) -> bool {
        let key = format!("{layer}:{chunk_id}");
        let last_access = self
            .accessed
            .get(&key)
            .copied()
            .unwrap_or(created_at_unix_ms);
        let now = now_unix_ms();
        now.saturating_sub(last_access) > self.ttl_ms
    }

    /// Update the TTL (in milliseconds).
    pub fn set_ttl_ms(&mut self, ttl_ms: u64) {
        self.ttl_ms = ttl_ms;
    }

    /// Remove entries for chunks that no longer exist (garbage collection).
    pub fn gc(&mut self, valid_keys: &std::collections::HashSet<String>) {
        self.accessed.retain(|k, _| valid_keys.contains(k));
    }
}
