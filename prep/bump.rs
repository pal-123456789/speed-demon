//! Typed bump arena — generic per-thread scratch (copy into track-*/src at H0).
//!
//! Allocates from a pre-sized slab by bumping a cursor; `reset()` rewinds in
//! O(1). Safe Rust, zero deps. The point (P7 §5/§7 of the model digest): no
//! per-item allocation, no `Vec` doubling transients (old+new live together),
//! and a flat, predictable memory.peak.
//!
//! ```
//! let mut arena = Bump::<u32>::with_capacity(1 << 20);
//! for v in 0..1000 { arena.push(v); }   // no realloc, no per-item malloc
//! arena.reset();                        // rewind; capacity is reused
//! ```

/// A bump arena for one type `T`. Not thread-safe — give every worker its own.
pub struct Bump<T: Copy> {
    buf: Vec<T>,
    pos: usize,
}

impl<T: Copy> Bump<T> {
    /// Pre-size the slab. Over-reserving is nearly free: untouched capacity is
    /// virtual, and only touched pages count toward memory.peak (P7 §5).
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
            pos: 0,
        }
    }

    /// Number of live items.
    pub fn len(&self) -> usize {
        self.pos
    }

    /// True if nothing was pushed since the last reset.
    pub fn is_empty(&self) -> bool {
        self.pos == 0
    }

    /// Push `v`, returning a stable reference into the slab.
    /// Reuses the slot on a re-push after reset — the capacity is never freed
    /// and never reallocated unless the initial capacity is exceeded.
    pub fn push(&mut self, v: T) -> &mut T {
        if self.pos == self.buf.len() {
            self.buf.push(v);
        } else {
            self.buf[self.pos] = v;
        }
        self.pos += 1;
        &mut self.buf[self.pos - 1]
    }

    /// Push a slice of items (e.g. an already-computed run of values).
    pub fn extend_from_slice(&mut self, items: &[T]) {
        for &v in items {
            self.push(v);
        }
    }

    /// Rewind the cursor. Capacity is kept and reused; nothing is freed.
    pub fn reset(&mut self) {
        self.pos = 0;
    }

    /// Drop the slab entirely (frees the capacity — use only between phases).
    pub fn clear(&mut self) {
        self.buf.clear();
        self.pos = 0;
    }

    /// Borrow the live items.
    pub fn as_slice(&self) -> &[T] {
        &self.buf[..self.pos]
    }
}

impl<T: Copy> Default for Bump<T> {
    fn default() -> Self {
        Self::with_capacity(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_reset_reuse() {
        let mut b = Bump::<u32>::with_capacity(8);
        b.extend_from_slice(&[1, 2, 3]);
        assert_eq!(b.as_slice(), &[1, 2, 3]);
        b.reset();
        assert!(b.is_empty());
        b.push(9);
        // the same slot is reused; the value is overwritten
        assert_eq!(b.as_slice(), &[9]);
    }

    #[test]
    fn grows_beyond_capacity() {
        let mut b = Bump::<u64>::with_capacity(4);
        for i in 0..1000u64 {
            b.push(i * i);
        }
        assert_eq!(b.len(), 1000);
        assert_eq!(b.as_slice()[999], 999 * 999);
    }
}
