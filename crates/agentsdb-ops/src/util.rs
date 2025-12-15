/// Convert bytes to lowercase hex string
pub fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = vec![0u8; bytes.len() * 2];
    for (i, b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    String::from_utf8(out).expect("valid hex")
}

/// Compute SHA-256 hash of content and return as hex string
pub fn content_sha256_hex(content: &str) -> String {
    let digest = agentsdb_embeddings::cache::sha256(content.as_bytes());
    hex_lower(&digest)
}

/// Apply redaction rules to content and embeddings
/// Returns (content, embedding) where either or both may be None based on redaction mode
pub fn apply_redaction(
    redact: &str,
    content: &str,
    embedding: &[f32],
) -> (Option<String>, Option<Vec<f32>>) {
    match redact {
        "none" => (Some(content.to_string()), Some(embedding.to_vec())),
        "content" => (None, Some(embedding.to_vec())),
        "embeddings" => (Some(content.to_string()), None),
        "all" => (None, None),
        _ => (Some(content.to_string()), Some(embedding.to_vec())),
    }
}

/// Convert EmbeddingElementType to string representation
pub fn element_type_str(t: agentsdb_format::EmbeddingElementType) -> &'static str {
    match t {
        agentsdb_format::EmbeddingElementType::F32 => "f32",
        agentsdb_format::EmbeddingElementType::I8 => "i8",
    }
}

/// Map standard layer file names to logical layer identifiers
pub fn logical_layer_for_path(rel_path: &str) -> Option<&'static str> {
    match rel_path {
        "AGENTS.db" => Some("base"),
        "AGENTS.user.db" => Some("user"),
        "AGENTS.delta.db" => Some("delta"),
        "AGENTS.local.db" => Some("local"),
        _ => None,
    }
}

/// Get current Unix timestamp in milliseconds
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Truncate a string to a maximum number of characters, appending '…' if truncated
pub fn truncate_preview(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (count, ch) in s.chars().enumerate() {
        if count >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}
