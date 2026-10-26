//! Minimal generic streaming I/O, self-contained in this track folder.
//!
//! Reading is streaming (never mmap): mmap pages are charged to cgroup
//! memory.peak and would inflate the M axis. Writing is buffered and
//! flushed exactly once; the byte-hash gates compare file contents, and
//! the container is torn down after the run, so fsync buys nothing.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

/// Streaming line reader: 1 MiB buffer, strips trailing `\n` and `\r\n`.
pub struct LineReader {
    inner: BufReader<File>,
}

impl LineReader {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            inner: BufReader::with_capacity(1 << 20, File::open(path)?),
        })
    }

    /// Call `f` with each line's bytes (no trailing newline).
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
}

/// Buffered deterministic writer; a single flush at `finish()`.
pub struct DetWriter {
    inner: BufWriter<File>,
    bytes: u64,
}

impl DetWriter {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            inner: BufWriter::with_capacity(1 << 20, File::create(path)?),
            bytes: 0,
        })
    }

    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.inner.write_all(line.as_bytes())?;
        self.inner.write_all(b"\n")?;
        self.bytes += line.len() as u64 + 1;
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<u64> {
        self.inner.flush()?;
        Ok(self.bytes)
    }
}