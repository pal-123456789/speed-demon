//! HEL1-safe buffered reader — prefers streaming over mmap.
//!
//! Why not mmap: on HEL1 `mmap`'d pages count toward cgroup `memory.peak`
//! and can push the M axis artificially. A `BufReader` with a 1 MiB buffer
//! streams without charging the full 312/250/204 MiB to the peak. Callers
//! that explicitly want mmap can still use `std::fs::File` directly, but
//! every track defaults to this reader and measures both.
//!
//! `ChunkReader` yields newline-delimited slices that never split a logical
//! row mid-quote (caller provides a `is_row_boundary` predicate for Track C).

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

const DEFAULT_BUF_CAP: usize = 1 << 20; // 1 MiB

#[derive(Debug)]
pub struct StreamingReader {
    inner: BufReader<File>,
}

impl StreamingReader {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_cap(path, DEFAULT_BUF_CAP)
    }

    pub fn open_with_cap(path: impl AsRef<Path>, cap: usize) -> io::Result<Self> {
        let f = File::open(path)?;
        Ok(Self {
            inner: BufReader::with_capacity(cap, f),
        })
    }

    /// Iterate raw lines (bytes, without the trailing newline). Returns
    /// `None` at EOF. The returned `Vec<u8>` is reused across calls via
    /// the internal BufReader buffer; caller must copy if it retains.
    pub fn for_each_line<F>(&mut self, mut f: F) -> io::Result<()>
    where
        F: FnMut(&[u8]),
    {
        let mut line = Vec::with_capacity(4096);
        loop {
            line.clear();
            let n = self.inner.read_until(b'\n', &mut line)?;
            if n == 0 {
                break;
            }
            // strip trailing \n and optional preceding \r
            if line.ends_with(b"\n") {
                line.pop();
                if line.ends_with(b"\r") {
                    line.pop();
                }
            }
            f(&line);
        }
        Ok(())
    }

    /// Convenience: collect all lines as owned byte vecs (only for small inputs / tests).
    pub fn read_all_lines(&mut self) -> io::Result<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        self.for_each_line(|l| out.push(l.to_vec()))?;
        Ok(out)
    }
}

/// Find safe split points for parallel chunking of a byte buffer.
///
/// `is_boundary(offset)` must return true only at a true logical row
/// boundary (not inside a quoted field for Track C). This function snaps
/// the naive `size/num_chunks` cut to the next boundary, guaranteeing no
/// worker sees a partial row.
///
/// Returns **at most** `num_chunks+1` offsets, strictly increasing, with
/// `offsets[0]=0` and `offsets[last]=size`. It returns fewer than
/// `num_chunks+1` when the buffer is too small to hold that many non-empty
/// chunks, or when the boundary predicate is so sparse that later cuts would
/// collapse onto the end. Zero-length chunks are never emitted.
///
/// Performance note: each cut search starts from the previous cut, not from
/// the naive stride position, so the total predicate work is O(size) rather
/// than O(size × num_chunks). The earlier version rescanned toward EOF from
/// every cut, which was quadratic when the predicate was sparse (measured
/// 550,000 calls for a 100 KB buffer at 12 chunks, and ~1.8 billion in the
/// worst case for a 312 MiB Track A input) — and because `par_chunks` calls
/// this on the main thread before spawning, that cost was fully serial.
pub fn chunk_boundaries<F>(size: usize, num_chunks: usize, mut is_boundary: F) -> Vec<usize>
where
    F: FnMut(usize) -> bool,
{
    if num_chunks == 0 || size == 0 {
        return vec![0, size];
    }
    let mut offsets = Vec::with_capacity(num_chunks + 1);
    offsets.push(0);
    let stride = size / num_chunks;

    for i in 1..num_chunks {
        // Never search before the previous cut: that is what keeps the total
        // work linear. `.max(last)` also guarantees strict monotonicity.
        let last = *offsets.last().unwrap();
        let mut cut = (i * stride).max(last);
        while cut < size && !is_boundary(cut) {
            cut += 1;
        }
        // Ran into the end (or landed on it): the remaining chunks are empty,
        // so stop. The trailing `size` below closes the last real chunk.
        if cut >= size {
            break;
        }
        // Only record a genuinely new cut. When `num_chunks > size` the stride
        // is 0 and `cut` pins to the previous offset; pushing it again would
        // emit a zero-length chunk and break the strict-monotonicity promise.
        if cut > last {
            offsets.push(cut);
        }
    }
    offsets.push(size);
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunk_boundaries_simple() {
        let b = chunk_boundaries(100, 4, |off| off % 25 == 0);
        assert_eq!(b.first(), Some(&0));
        assert_eq!(b.last(), Some(&100));
    }
}
