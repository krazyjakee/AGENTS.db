use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::embedder::EmbeddingProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheKeyAlg {
    Sha256ProfileJsonV1NullContentUtf8,
    Sha256ProfileJsonV2NullContentUtf8,
}

impl CacheKeyAlg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha256ProfileJsonV1NullContentUtf8 => {
                "sha256(profile_json_v1 || 0x00 || content_utf8)"
            }
            Self::Sha256ProfileJsonV2NullContentUtf8 => {
                "sha256(profile_json_v2 || 0x00 || content_utf8)"
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct EmbeddingProfileFingerprintV1<'a> {
    v: u32,
    backend: &'a str,
    model: &'a Option<String>,
    revision: &'a Option<String>,
    dim: usize,
}

fn profile_fingerprint_json_v1(profile: &EmbeddingProfile) -> anyhow::Result<Vec<u8>> {
    let fp = EmbeddingProfileFingerprintV1 {
        v: 1,
        backend: &profile.backend,
        model: &profile.model,
        revision: &profile.revision,
        dim: profile.dim,
    };
    serde_json::to_vec(&fp).context("serialize profile fingerprint")
}

#[derive(Debug, Clone, Serialize)]
struct EmbeddingProfileFingerprintV2<'a> {
    v: u32,
    backend: &'a str,
    model: &'a Option<String>,
    revision: &'a Option<String>,
    dim: usize,
    output_norm: crate::embedder::OutputNorm,
}

fn profile_fingerprint_json_v2(profile: &EmbeddingProfile) -> anyhow::Result<Vec<u8>> {
    let fp = EmbeddingProfileFingerprintV2 {
        v: 2,
        backend: &profile.backend,
        model: &profile.model,
        revision: &profile.revision,
        dim: profile.dim,
        output_norm: profile.output_norm,
    };
    serde_json::to_vec(&fp).context("serialize profile fingerprint")
}

pub fn cache_key_hex(profile: &EmbeddingProfile, content_utf8: &str) -> anyhow::Result<String> {
    // New profiles include additional determinism-relevant fields (e.g. normalization).
    // Keep V1 for backward compatibility with previously populated caches.
    let fp = profile_fingerprint_json_v2(profile)
        .or_else(|_| profile_fingerprint_json_v1(profile))
        .context("profile fingerprint")?;
    let mut buf = Vec::with_capacity(fp.len() + 1 + content_utf8.len());
    buf.extend_from_slice(&fp);
    buf.push(0);
    buf.extend_from_slice(content_utf8.as_bytes());
    let digest = sha256(&buf);
    Ok(hex_lower(&digest))
}

#[derive(Debug, Clone)]
pub struct DiskEmbeddingCache {
    dir: PathBuf,
}

impl DiskEmbeddingCache {
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("create cache dir {}", dir.display()))?;
        Ok(Self { dir })
    }

    pub fn default_dir() -> anyhow::Result<PathBuf> {
        if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
            return Ok(PathBuf::from(dir).join("agentsdb").join("embeddings"));
        }
        if cfg!(windows) {
            if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
                return Ok(PathBuf::from(dir).join("agentsdb").join("embeddings"));
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Ok(PathBuf::from(home)
                .join(".cache")
                .join("agentsdb")
                .join("embeddings"));
        }
        anyhow::bail!("unable to determine cache dir (set XDG_CACHE_HOME or HOME)")
    }

    fn path_for_key(&self, key_hex: &str) -> PathBuf {
        let (a, b) = if key_hex.len() >= 4 {
            (&key_hex[0..2], &key_hex[2..4])
        } else {
            ("xx", "yy")
        };
        self.dir.join(a).join(b).join(format!("{key_hex}.json"))
    }

    pub fn load_f32(&self, key_hex: &str) -> anyhow::Result<Option<Vec<f32>>> {
        let path = self.path_for_key(key_hex);
        let mut f = match std::fs::File::open(&path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("open {}", path.display())),
        };
        let mut s = String::new();
        f.read_to_string(&mut s)
            .with_context(|| format!("read {}", path.display()))?;
        let entry: CacheEntryV1 = serde_json::from_str(&s).context("parse cache entry")?;
        if entry.key != key_hex {
            return Ok(None);
        }
        Ok(Some(entry.embedding))
    }

    pub fn store_f32(
        &self,
        key_hex: &str,
        profile: &EmbeddingProfile,
        embedding: &[f32],
    ) -> anyhow::Result<()> {
        let path = self.path_for_key(key_hex);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }

        let entry = CacheEntryV1::new(key_hex, profile, embedding);
        let json = serde_json::to_vec(&entry).context("serialize cache entry")?;
        atomic_write(&path, &json)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntryV1 {
    v: u32,
    key: String,
    cache_key_alg: CacheKeyAlg,
    profile: EmbeddingProfileStored,
    dim: usize,
    embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmbeddingProfileStored {
    backend: String,
    model: Option<String>,
    revision: Option<String>,
    dim: usize,
    #[serde(default)]
    output_norm: crate::embedder::OutputNorm,
}

impl CacheEntryV1 {
    fn new(key_hex: &str, profile: &EmbeddingProfile, embedding: &[f32]) -> Self {
        Self {
            v: 1,
            key: key_hex.to_string(),
            cache_key_alg: CacheKeyAlg::Sha256ProfileJsonV2NullContentUtf8,
            profile: EmbeddingProfileStored {
                backend: profile.backend.clone(),
                model: profile.model.clone(),
                revision: profile.revision.clone(),
                dim: profile.dim,
                output_norm: profile.output_norm,
            },
            dim: profile.dim,
            embedding: embedding.to_vec(),
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let base = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("entry.json");

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

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = vec![0u8; bytes.len() * 2];
    for (i, b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    String::from_utf8(out).expect("valid hex")
}

pub fn sha256(input: &[u8]) -> [u8; 32] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut block = [0u8; 64];
    let mut len = 0usize;
    let mut total_len: u64 = 0;

    let push_block = |state: &mut [u32; 8], block: &[u8; 64]| {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let j = i * 4;
            w[i] = u32::from_be_bytes([block[j], block[j + 1], block[j + 2], block[j + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    };

    for &b in input {
        block[len] = b;
        len += 1;
        total_len += 1;
        if len == 64 {
            push_block(&mut state, &block);
            len = 0;
        }
    }

    block[len] = 0x80;
    len += 1;
    if len > 56 {
        for b in block[len..].iter_mut() {
            *b = 0;
        }
        push_block(&mut state, &block);
        len = 0;
    }
    for b in block[len..56].iter_mut() {
        *b = 0;
    }
    let bit_len = total_len * 8;
    block[56..64].copy_from_slice(&bit_len.to_be_bytes());
    push_block(&mut state, &block);

    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector_empty() {
        let d = sha256(b"");
        assert_eq!(
            hex_lower(&d),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn cache_key_is_stable() {
        let p = EmbeddingProfile {
            backend: "hash".to_string(),
            model: None,
            revision: None,
            dim: 8,
            output_norm: crate::embedder::OutputNorm::None,
        };
        let k1 = cache_key_hex(&p, "hello world").unwrap();
        let k2 = cache_key_hex(&p, "hello world").unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn disk_cache_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskEmbeddingCache::new(dir.path().to_path_buf()).unwrap();

        let p = EmbeddingProfile {
            backend: "hash".to_string(),
            model: None,
            revision: None,
            dim: 3,
            output_norm: crate::embedder::OutputNorm::None,
        };
        let key = cache_key_hex(&p, "x").unwrap();
        assert!(cache.load_f32(&key).unwrap().is_none());
        cache.store_f32(&key, &p, &[1.0, 2.0, 3.0]).unwrap();
        assert_eq!(cache.load_f32(&key).unwrap().unwrap(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn disk_cache_entry_bytes_are_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskEmbeddingCache::new(dir.path().to_path_buf()).unwrap();
        let profile = EmbeddingProfile {
            backend: "hash".to_string(),
            model: None,
            revision: None,
            dim: 4,
            output_norm: crate::embedder::OutputNorm::None,
        };
        let key = cache_key_hex(&profile, "hello").unwrap();
        let emb = vec![0.25_f32, 0.5, -1.0, 2.0];

        cache.store_f32(&key, &profile, &emb).unwrap();
        let path = dir
            .path()
            .join(&key[0..2])
            .join(&key[2..4])
            .join(format!("{key}.json"));
        let bytes1 = std::fs::read(&path).unwrap();

        cache.store_f32(&key, &profile, &emb).unwrap();
        let bytes2 = std::fs::read(&path).unwrap();

        assert_eq!(bytes1, bytes2);
    }
}
