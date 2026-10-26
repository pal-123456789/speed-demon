//! Decimal formatter into a reused buffer — generic (copy into track-*/src
//! at H0).
//!
//! One-buffer output building: write all rows into a single `Vec<u8>` with
//! `itoa`/`itoa64`, then one `write_all`. Never `format!` or `println!` in
//! hot paths (P7 §8). Hand-rolled digit extraction beats the formatting
//! machinery by a wide margin and is fully deterministic.
//!
//! ```
//! let mut buf = Vec::with_capacity(64);
//! itoa(8177, &mut buf);
//! buf.push(b'\n');
//! ```

/// Append the decimal representation of `v` to `buf`.
pub fn itoa(mut v: u32, buf: &mut Vec<u8>) {
    if v == 0 {
        buf.push(b'0');
        return;
    }
    let start = buf.len();
    while v > 0 {
        buf.push((b'0' + (v % 10) as u8) as u8);
        v /= 10;
    }
    buf[start..].reverse();
}

/// Append the decimal representation of `v` (u64 — for i64 cents math and
/// graph distances) to `buf`.
pub fn itoa64(mut v: u64, buf: &mut Vec<u8>) {
    if v == 0 {
        buf.push(b'0');
        return;
    }
    let start = buf.len();
    while v > 0 {
        buf.push((b'0' + (v % 10) as u8) as u8);
        v /= 10;
    }
    buf[start..].reverse();
}

/// Append `v / 10^k` and `v % 10^k` zero-padded to `k` digits (fixed-width
/// decimals, e.g. cents): writes "123.45" for v = 12345, k = 2.
pub fn itoa_fixed(mut v: u64, k: u32, buf: &mut Vec<u8>) {
    let mut frac = [0u8; 20];
    let mut n = 0usize;
    for _ in 0..k {
        frac[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    itoa64(v, buf);
    buf.push(b'.');
    while n > 0 {
        n -= 1;
        buf.push(frac[n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn itoa_basics() {
        let mut b = Vec::new();
        itoa(0, &mut b);
        itoa(7, &mut b);
        itoa(8177, &mut b);
        itoa(u32::MAX, &mut b);
        assert_eq!(b, b"0781774294967295");
    }

    #[test]
    fn itoa64_basics() {
        let mut b = Vec::new();
        itoa64(u64::MAX, &mut b);
        assert_eq!(b, b"18446744073709551615");
    }

    #[test]
    fn fixed_decimals() {
        let mut b = Vec::new();
        itoa_fixed(12345, 2, &mut b); // 123.45
        assert_eq!(b, b"123.45");
        let mut c = Vec::new();
        itoa_fixed(5, 2, &mut c); // 0.05
        assert_eq!(c, b"0.05");
        let mut d = Vec::new();
        itoa_fixed(0, 2, &mut d); // 0.00
        assert_eq!(d, b"0.00");
    }
}
