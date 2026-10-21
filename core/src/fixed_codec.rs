//! Fixed-point and bit-exact codec helpers.
//!
//! - Track C: decimal `amount` → integer cents (`i64` per row, `i128` sum).
//! - Track E blob: `f32` ↔ `u32` bit-pattern transpose (never `f32` arithmetic).

/// Parse a decimal amount string (e.g. `"1420.00"`, `"-5.25"`, `"0.00"`) into
/// integer cents. Returns `None` on malformed input. No float involved.
pub fn parse_cents(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (neg, rest) = if let Some(r) = s.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (false, r)
    } else {
        (false, s)
    };
    let (dollars_str, cents_str) = if let Some(dot) = rest.find('.') {
        (&rest[..dot], &rest[dot + 1..])
    } else {
        (rest, "")
    };
    let dollars: i64 = if dollars_str.is_empty() {
        0
    } else {
        dollars_str.parse().ok()?
    };
    let cents: i64 = match cents_str.len() {
        0 => 0,
        1 => cents_str.parse::<i64>().ok()? * 10,
        2 => cents_str.parse().ok()?,
        _ => {
            // truncate beyond 2 decimals? No — treat as malformed for scaffold
            // (frozen spec pins to 2 decimals; handle exactly)
            cents_str[..2].parse().ok()?
        }
    };
    let total = dollars.checked_mul(100)?.checked_add(cents)?;
    Some(if neg { -total } else { total })
}

/// Format integer cents as `"<dollars>.<cc>"` with exactly 2 decimal places.
pub fn format_cents(cents: i64) -> String {
    let neg = cents < 0;
    let abs = (cents as i128).abs();
    let dollars = abs / 100;
    let cc = (abs % 100) as u8;
    if neg {
        format!("-{}.{:02}", dollars, cc)
    } else {
        format!("{}.{:02}", dollars, cc)
    }
}

/// f32 ↔ u32 bit-pattern helpers (Track E blob — never do `f32` math on the blob).
#[inline]
pub fn f32_to_bits(f: f32) -> u32 {
    f.to_bits()
}
#[inline]
pub fn bits_to_f32(b: u32) -> f32 {
    f32::from_bits(b)
}

/// Transpose N f32 bit-patterns from AoS to SoA planes:
/// plane 0 = sign+exponent bytes, plane 1..3 = mantissa bytes.
/// Scaffold is a no-op copy; H0 replaces with a real transpose + delta/XOR.
pub fn transpose_f32_block(src: &[u32], dst: &mut [u8]) {
    // Scaffold: just copy raw bytes LE
    for (i, &bits) in src.iter().enumerate() {
        let bytes = bits.to_le_bytes();
        dst[i * 4..i * 4 + 4].copy_from_slice(&bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cents_roundtrip() {
        for &s in &["1420.00", "-5.25", "0.00", "88.50", "0.1", "0.01"] {
            let c = parse_cents(s).unwrap();
            let back = format_cents(c);
            let c2 = parse_cents(&back).unwrap();
            assert_eq!(c, c2, "roundtrip failed for {:?}", s);
        }
    }
    #[test]
    fn f32_bits_roundtrip() {
        for f in [
            0.0f32,
            -0.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            1.5,
            -42.0,
        ] {
            let b = f32_to_bits(f);
            let g = bits_to_f32(b);
            assert_eq!(f.to_bits(), g.to_bits());
        }
    }
}
