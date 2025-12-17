//! Shared utility functions used across multiple embedding backends.

#[cfg(any(
    feature = "openai",
    feature = "voyage",
    feature = "cohere",
    feature = "anthropic",
    feature = "bedrock",
    feature = "gemini",
    feature = "candle",
    feature = "ort"
))]
pub(super) fn ensure_dim(expected: usize, got: usize, backend: &str) -> anyhow::Result<()> {
    if expected != got {
        anyhow::bail!("{backend} embedder dimension mismatch (expected {expected}, got {got})");
    }
    Ok(())
}

#[cfg(any(
    feature = "openai",
    feature = "voyage",
    feature = "cohere",
    feature = "anthropic",
    feature = "bedrock",
    feature = "gemini"
))]
pub(super) fn require_env(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}

#[cfg(any(
    feature = "openai",
    feature = "voyage",
    feature = "cohere",
    feature = "anthropic",
    feature = "bedrock",
    feature = "gemini"
))]
pub(super) fn collect_headers(
    resp: &ureq::Response,
    names: &[&str],
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for &name in names {
        if let Some(v) = resp.header(name) {
            out.insert(name.to_string(), v.to_string());
        }
    }
    out
}

#[cfg(any(feature = "ort", feature = "candle"))]
pub(super) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = vec![0u8; bytes.len() * 2];
    for (i, b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    String::from_utf8(out).expect("valid hex")
}

#[cfg(any(
    feature = "openai",
    feature = "voyage",
    feature = "cohere",
    feature = "anthropic",
    feature = "bedrock",
    feature = "gemini"
))]
use anyhow::Context;
