pub fn hash_embed(text: &str, dim: usize) -> Vec<f32> {
    if dim == 0 {
        return Vec::new();
    }

    let mut v = vec![0.0f32; dim];
    for token in text.split_whitespace() {
        let h = fnv1a32(token.as_bytes());
        let idx = (h as usize) % dim;
        let sign = if (h & 0x8000_0000) != 0 { -1.0 } else { 1.0 };
        v[idx] += sign;
    }

    l2_normalize(&mut v);
    v
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    const OFFSET: u32 = 0x811c9dc5;
    const PRIME: u32 = 0x0100_0193;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(PRIME);
    }
    h
}

fn l2_normalize(v: &mut [f32]) {
    let mut sum = 0.0f32;
    for x in v.iter() {
        sum += x * x;
    }
    if sum == 0.0 {
        return;
    }
    let inv = 1.0 / sum.sqrt();
    for x in v.iter_mut() {
        *x *= inv;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_embed_is_deterministic() {
        let a = hash_embed("hello world", 8);
        let b = hash_embed("hello world", 8);
        assert_eq!(a, b);
    }

    #[test]
    fn hash_embed_normalizes_nonzero() {
        let v = hash_embed("x y z", 16);
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }
}
