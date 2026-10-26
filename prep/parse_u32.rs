//! Fast checked decimal `u32` parse — generic (copy into track-*/src at H0).
//!
//! Scans digits from a `&[u8]`, returns the value and the number of bytes
//! consumed. Rejects: overflow (more digits than fit, or a value > u32::MAX),
//! a leading non-digit, and an empty field. This is the scalar starting point
//! for the P2/P5 SWAR designs — the vector paths must produce the same
//! (value, consumed) pair for every input this accepts, and must bail to the
//! slow path for everything this rejects.
//!
//! ```
//! let (v, n) = parse_u32(b"8177 ").unwrap();   // v = 8177, n = 4
//! let (v2, _) = parse_u32(b"4294967295").unwrap();   // u32::MAX, 10 digits
//! assert!(parse_u32(b"4294967296").is_none());       // overflow
//! assert!(parse_u32(b"abc").is_none());              // not a digit
//! ```

/// Parse leading decimal digits into a `u32`.
///
/// Returns `(value, bytes_consumed)`. Consumes only leading ASCII digits;
/// stops at the first non-digit. `None` if the first byte is not a digit or
/// the value overflows `u32`.
pub fn parse_u32(b: &[u8]) -> Option<(u32, usize)> {
    let first = *b.first()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let mut v: u32 = (first - b'0') as u32;
    let mut i = 1;
    while i < b.len() && b[i].is_ascii_digit() {
        let d = (b[i] - b'0') as u32;
        v = v.checked_mul(10)?.checked_add(d)?;
        i += 1;
    }
    Some((v, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert_eq!(parse_u32(b"8177 "), Some((8177, 4)));
        assert_eq!(parse_u32(b"0"), Some((0, 1)));
        assert_eq!(parse_u32(b"0000001"), Some((1, 7)));
        assert_eq!(parse_u32(b"4294967295"), Some((u32::MAX, 10)));
    }

    #[test]
    fn stops_at_non_digit() {
        assert_eq!(parse_u32(b"12ab"), Some((12, 2)));
        assert_eq!(parse_u32(b"7,8"), Some((7, 1)));
    }

    #[test]
    fn rejects() {
        assert!(parse_u32(b"").is_none());
        assert!(parse_u32(b"abc").is_none());
        assert!(parse_u32(b"-5").is_none());
        assert!(parse_u32(b"4294967296").is_none()); // overflow
    }
}
