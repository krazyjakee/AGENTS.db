use anyhow::Context;

pub fn ensure_sha256_hex(expected: &str) -> anyhow::Result<()> {
    let expected = expected.trim();
    if expected.len() != 64 {
        anyhow::bail!(
            "expected sha256 must be 64 lowercase hex chars (got len={})",
            expected.len()
        );
    }
    if !expected
        .as_bytes()
        .iter()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        anyhow::bail!("expected sha256 must be lowercase hex");
    }
    Ok(())
}

pub fn verify_model_sha256(expected: Option<&str>, actual_lower_hex: &str) -> anyhow::Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let expected = expected.trim();
    ensure_sha256_hex(expected).context("validate expected sha256")?;
    if expected != actual_lower_hex {
        anyhow::bail!(
            "downloaded model sha256 mismatch (expected {expected}, got {actual_lower_hex})"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_model_sha256_accepts_match() {
        let sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        verify_model_sha256(Some(sha), sha).expect("should match");
    }

    #[test]
    fn verify_model_sha256_rejects_mismatch() {
        let expected = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let actual = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let err = verify_model_sha256(Some(expected), actual).unwrap_err();
        assert!(err.to_string().contains("downloaded model sha256 mismatch"));
    }

    #[test]
    fn verify_model_sha256_rejects_uppercase() {
        let sha = "0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef";
        let actual = sha.to_ascii_lowercase();
        let err = verify_model_sha256(Some(sha), &actual).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("expected sha256"));
    }
}
