//! Zero-copy newline splitter — generic input streaming (copy into
//! track-*/src at H0).
//!
//! Splits a `&[u8]` into records on `\n` and `\r\n` (the `\r` is stripped from
//! CRLF endings; a bare `\r` is kept inside the record — callers that need a
//! bare-CR rule handle it). No allocation, no unsafe. The scaffold's `io.rs`
//! `LineReader` feeds whole lines; this splits a larger buffer, which is what
//! chunk-based parallelism wants: each worker owns a chunk, finds the next
//! boundary with `Lines`, and the records never straddle chunks.
//!
//! ```
//! for rec in Lines::new(b"a b\r\nc d\ne f\n") {
//!     // rec == b"a b", b"c d", b"e f"
//! }
//! ```

/// Iterator over newline-terminated records in `&[u8]`.
pub struct Lines<'a> {
    rest: &'a [u8],
}

impl<'a> Lines<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { rest: data }
    }

    /// Whether any input remains (without advancing).
    pub fn has_more(&self) -> bool {
        !self.rest.is_empty()
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.rest.is_empty() {
            return None;
        }
        // find the next '\n' or the end
        let mut i = 0;
        while i < self.rest.len() && self.rest[i] != b'\n' {
            i += 1;
        }
        let end = if i < self.rest.len() {
            i
        } else {
            self.rest.len()
        };
        // strip a trailing '\r' of a CRLF ending
        let rec_end = if end > 0 && self.rest[end - 1] == b'\r' {
            end - 1
        } else {
            end
        };
        let rec = &self.rest[..rec_end];
        // consume the record plus the terminator (or the tail)
        self.rest = if i < self.rest.len() {
            &self.rest[i + 1..]
        } else {
            &self.rest[self.rest.len()..]
        };
        Some(rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(data: &[u8]) -> Vec<&[u8]> {
        Lines::new(data).collect()
    }

    #[test]
    fn lf_and_crlf() {
        assert_eq!(
            collect(b"a b\r\nc d\ne f\n"),
            vec![&b"a b"[..], &b"c d"[..], &b"e f"[..]]
        );
    }

    #[test]
    fn no_trailing_newline() {
        assert_eq!(collect(b"one\ntwo"), vec![&b"one"[..], &b"two"[..]]);
    }

    #[test]
    fn empty_and_blank_lines() {
        assert_eq!(collect(b"\n\n"), vec![&b""[..], &b""[..]]);
        assert_eq!(collect(b""), Vec::<&[u8]>::new());
    }
}
