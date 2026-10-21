//! SIMD byte-scanning primitives — scalar fallback now, AVX2 later.
//!
//! Scaffold: scalar `memchr`-style scan. H0 deep block replaces the
//! inner loop with `_mm256` / `std::simd` (when stable) behind
//! `#[cfg(target_feature = "avx2")]`. Callers never branch on the
//! backend — they call `find_byte`, `find_any`, `count_byte`.

/// Find the next occurrence of `needle` in `haystack[start..]`.
/// Returns `Some(index)` or `None`. Scalar fallback — correct, not fast.
pub fn find_byte(haystack: &[u8], needle: u8, start: usize) -> Option<usize> {
    haystack[start..]
        .iter()
        .position(|&b| b == needle)
        .map(|p| start + p)
}

/// Find the next occurrence of ANY byte in `needles`.
pub fn find_any(haystack: &[u8], needles: &[u8], start: usize) -> Option<usize> {
    // tiny needles set (e.g. b",\"\n") — scalar is fine for scaffold
    haystack[start..]
        .iter()
        .position(|&b| needles.contains(&b))
        .map(|p| start + p)
}

/// Count occurrences of `needle` in `haystack`.
pub fn count_byte(haystack: &[u8], needle: u8) -> usize {
    haystack.iter().filter(|&&b| b == needle).count()
}

/// Classify bytes into a bitmask — one bit per byte in a 32-byte window.
/// Returns a `u32` where bit `i` = 1 iff `haystack[offset+i]` matches `needle`.
/// Scalar scaffold; AVX2 version uses `_mm256_cmpeq_epi8` + `_mm256_movemask_epi8`.
pub fn classify_byte_mask(haystack: &[u8], offset: usize, needle: u8) -> u32 {
    let end = (offset + 32).min(haystack.len());
    let mut mask = 0u32;
    for (i, &b) in haystack[offset..end].iter().enumerate() {
        if b == needle {
            mask |= 1u32 << i;
        }
    }
    mask
}

/// Find the next `"` or `,` or `\n` — the hot inner loop for CSV/ingest.
/// Returns the index and which byte was found.
pub fn find_csv_special(haystack: &[u8], start: usize) -> Option<(usize, u8)> {
    for (i, &b) in haystack[start..].iter().enumerate() {
        if b == b'"' || b == b',' || b == b'\n' || b == b'\r' {
            return Some((start + i, b));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn find_byte_basic() {
        assert_eq!(find_byte(b"hello, world", b',', 0), Some(5));
        assert_eq!(find_byte(b"hello", b'x', 0), None);
    }
    #[test]
    fn classify_mask() {
        let h = b"a,b,c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r,s,t,u,v,w,x,y,z,0,1,2,3,4,5";
        let m = classify_byte_mask(h, 0, b',');
        assert_ne!(m, 0);
    }
}
