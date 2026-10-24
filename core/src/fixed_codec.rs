//! Fixed-point and bit-exact codec helpers.
//!
//! - Track C: decimal `amount` → integer cents (`i64` per row, `i128` sum).
//! - Track E blob: `f32` ↔ `u32` bit-pattern transpose (never `f32` arithmetic).

/// Parse a decimal amount string (e.g. `"1420.00"`, `"-5.25"`, `"0.00"`) into
/// integer cents. Returns `None` on malformed input. No float involved.
///
/// # Accepted grammar (deliberately strict)
///
/// ```text
/// amount := [sign] digits [ "." digits{1,2} ]
/// sign   := "+" | "-"          (at most one, only at the start)
/// ```
///
/// A leading UTF-8 BOM is tolerated and stripped, because the frozen Track C
/// corpus is documented to ship one on the header line and may repeat it
/// mid-file; treating it as part of the value would silently drop a record.
///
/// # Why so strict
///
/// The Track C output is a byte-hash gate. Every ambiguous form below has two
/// defensible readings, and picking the wrong one scores zero on the track
/// rather than degrading gracefully. So the rule is: reject anything ambiguous
/// and let the caller decide, rather than guess:
///
/// * more than 2 decimals — truncation (`1.005` -> `100`) and half-up
///   rounding (`1.01`) both look reasonable and give different bytes. The
///   frozen spec pins 2 decimals, so a 3rd decimal is malformed input, not a
///   rounding instruction. Rejected, not silently truncated.
/// * a sign in the fraction (`"5.-9"`) — the old code stripped the sign from
///   the whole string and then parsed the fraction independently, so `-9`
///   leaked in as a *negative* fraction and produced `491` instead of `509`.
///   Rejected.
/// * no digits at all (`"."`, `"-"`, `""`) — these were silently `0`, which
///   turns a malformed row into a valid-looking zero row. Rejected.
/// * non-ASCII bytes — the old code sliced `cents_str[..2]` by byte index,
///   which panics when byte 2 is not a char boundary (`"1.2€"`). Rejected up
///   front, so this can never panic.
pub fn parse_cents(s: &str) -> Option<i64> {
    // Strip a BOM and surrounding ASCII whitespace. `trim` alone does not
    // remove U+FEFF, and U+FEFF is not whitespace to Rust.
    let s = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if s.is_empty() {
        return None;
    }

    // Reject non-ASCII outright. This is what makes the byte-index slicing
    // below safe, and it also refuses exotic digits that would parse into
    // something surprising.
    if !s.is_ascii() {
        return None;
    }

    let (neg, rest) = if let Some(r) = s.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (false, r)
    } else {
        (false, s)
    };
    // A second sign anywhere (leading or trailing) is malformed: it means the
    // field is something other than a plain decimal amount.
    if rest.contains(['+', '-']) {
        return None;
    }

    let (dollars_str, cents_str) = match rest.find('.') {
        Some(dot) => (&rest[..dot], &rest[dot + 1..]),
        None => (rest, ""),
    };

    // At least one digit must be present overall, and every present part must
    // be all-digits.
    if dollars_str.is_empty() && cents_str.is_empty() {
        return None;
    }
    if !dollars_str.is_empty() && !dollars_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !cents_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let dollars: i64 = if dollars_str.is_empty() {
        0
    } else {
        dollars_str.parse().ok()?
    };

    // Widened to i128 for the fraction, then range-checked on the way back.
    // The old code built `total` as i64 and negated it unchecked, so a value
    // that reached i64::MIN aborted the process under `panic = "abort"`.
    let cents: i128 = match cents_str.len() {
        0 => 0,
        1 => (cents_str.as_bytes()[0] - b'0') as i128 * 10,
        2 => {
            let hi = (cents_str.as_bytes()[0] - b'0') as i128;
            let lo = (cents_str.as_bytes()[1] - b'0') as i128;
            hi * 10 + lo
        }
        // 3+ decimals: ambiguous (truncate vs round). Rejected.
        _ => return None,
    };

    let total = (dollars as i128).checked_mul(100)?.checked_add(cents)?;
    let signed = if neg { total.checked_neg()? } else { total };
    i64::try_from(signed).ok()
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
