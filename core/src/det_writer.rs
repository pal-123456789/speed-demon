//! Deterministic output writer — SHA-gate safe.
//!
//! Rules enforced here:
//! - No HashMap iteration leaks into output (callers sort keys outside).
//! - No float formatting through `Display` on `f64` without an explicit spec.
//! - Writes are buffered and flushed exactly once at `finish()`.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// A buffered, deterministic file writer.
#[derive(Debug)]
pub struct DetWriter {
    inner: BufWriter<File>,
    bytes_written: u64,
}

impl DetWriter {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let f = File::create(path)?;
        Ok(Self {
            inner: BufWriter::with_capacity(1 << 20, f),
            bytes_written: 0,
        })
    }

    /// Write a full line including the trailing `\n`.
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.write_bytes(line.as_bytes())?;
        self.write_bytes(b"\n")
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.write_all(bytes)?;
        self.bytes_written += bytes.len() as u64;
        Ok(())
    }

    /// Format an integer number of cents as `dollars.cents` with exactly 2
    /// decimal places (Track C). Handles negative values. Integer-only —
    /// no float ever touches the amount path.
    pub fn write_cents(&mut self, cents: i64) -> io::Result<()> {
        let neg = cents < 0;
        // i128 avoids overflow on i64::MIN (whose absolute value exceeds i64::MAX).
        let abs128 = (cents as i128).abs();
        let dollars = abs128 / 100;
        let rem = (abs128 % 100) as u8;
        if neg {
            self.write_bytes(b"-")?;
        }
        // write dollars without allocation
        let mut buf = [0u8; 32];
        let s = format_int(dollars, &mut buf);
        self.write_bytes(s.as_bytes())?;
        self.write_bytes(b".")?;
        let tens = rem / 10;
        let ones = rem % 10;
        self.write_bytes(&[b'0' + tens, b'0' + ones])
    }

    /// Flush and sync; call exactly once at the end.
    pub fn finish(mut self) -> io::Result<u64> {
        self.inner.flush()?;
        Ok(self.bytes_written)
    }
}

fn format_int(mut n: i128, buf: &mut [u8; 32]) -> &str {
    if n == 0 {
        buf[31] = b'0';
        return unsafe { std::str::from_utf8_unchecked(&buf[31..32]) };
    }
    let mut pos = 32;
    while n > 0 {
        pos -= 1;
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    unsafe { std::str::from_utf8_unchecked(&buf[pos..32]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn cents_formatting() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("det_writer_test_{}.tmp", std::process::id()));
        let mut w = DetWriter::create(&path).unwrap();
        w.write_cents(142000).unwrap(); // 1420.00
        w.write_bytes(b"\n").unwrap();
        w.write_cents(-525).unwrap(); // -5.25
        w.write_bytes(b"\n").unwrap();
        w.write_cents(0).unwrap();
        w.finish().unwrap();
        let mut s = String::new();
        File::open(&path).unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "1420.00\n-5.25\n0.00");
        let _ = std::fs::remove_file(&path);
    }
}
