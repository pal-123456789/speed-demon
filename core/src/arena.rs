//! Bump arena + string interner — zero per-record allocation.
//!
//! # Why spans instead of `&str`
//!
//! The obvious arena API (`fn alloc_str(&mut self, s: &str) -> &str`) cannot
//! intern two strings: the first returned `&str` borrows the arena
//! mutably, so the second call is rejected by the borrow checker. That
//! pattern works only for `&self` arenas, which need interior mutability and
//! therefore `unsafe` lifetime extension.
//!
//! The production-safe design hands out **spans** instead:
//!
//! * [`Arena::alloc`] takes `&mut self` and returns a [`Span`] (offset+len).
//! * [`Arena::get`] / [`Arena::get_str`] take `&self` and return borrowed
//!   slices. Shared borrows coexist, so any number of interned strings can be
//!   alive at once.
//!
//! The arena's buffer is only ever appended to (never shrunk, never
//! reordered), so a span handed out stays valid for the arena's lifetime.
//!
//! Phase 2 (H0 deep block) swaps the single buffer for a chunked arena
//! (64 KiB chunks, one bump pointer per chunk) so a shard's arena resets
//! without touching the whole buffer and the backing storage stops
//! reallocating and copying on growth.

use std::collections::HashMap;

/// A stable (offset, len) handle into an [`Arena`]'s byte buffer.
///
/// `off`/`len` are `u32`, so a single arena is limited to 4 GiB of interned
/// bytes. That is far above any track's working set (the largest dataset is
/// 312 MiB), but the limit is real: [`Arena::alloc`] asserts against it rather
/// than truncating silently, because a wrapped offset aliases an earlier span
/// and [`Arena::get`] would then return the wrong bytes with no panic — the
/// one failure mode a SHA gate cannot catch in review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    pub off: u32,
    pub len: u32,
}

impl Span {
    pub const EMPTY: Span = Span { off: 0, len: 0 };

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A bump arena over a single growable byte buffer.
///
/// Carries a `generation` counter that is bumped by [`Arena::reset`]. A
/// [`Span`] does not itself store the generation (it is a plain `u32` pair for
/// cheap hashing), so this field exists to document and support the invariant
/// that an [`Interner`] must be reset together with its arena — see
/// [`Interner::reset`] and the [`Arena::reset`] warning.
#[derive(Debug, Default)]
pub struct Arena {
    buf: Vec<u8>,
    pos: usize,
    generation: u32,
}

impl Arena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: vec![0u8; cap],
            pos: 0,
            generation: 0,
        }
    }

    /// Copy `bytes` into the arena and return its span.
    ///
    /// # Panics
    ///
    /// Panics if the resulting offset or length would not fit in the `u32`
    /// fields of [`Span`] (a single arena exceeding 4 GiB). Failing loudly is
    /// deliberate: silently truncating produces an offset that aliases an
    /// earlier span, and the caller then hashes the wrong bytes.
    pub fn alloc(&mut self, bytes: &[u8]) -> Span {
        let len = bytes.len();
        // Checked arithmetic: `pos + len` can overflow usize in theory, and
        // `next_power_of_two` panics past 2^63. Both would be an unexplained
        // abort deep inside the arena rather than a diagnosable error.
        let end = self.pos.checked_add(len).expect("arena offset overflow");
        assert!(
            end <= u32::MAX as usize,
            "arena span exceeds u32 (offset+len = {end}); a single arena is \
             limited to 4 GiB — use a chunked arena for larger working sets"
        );
        if end > self.buf.len() {
            // Grow geometrically. `resize` preserves the existing prefix, so
            // spans handed out earlier stay valid.
            let need = end
                .checked_next_power_of_two()
                .expect("arena capacity overflow")
                .max(self.buf.len().saturating_mul(2))
                .max(64 * 1024);
            self.buf.resize(need, 0);
        }
        let start = self.pos;
        self.buf[start..end].copy_from_slice(bytes);
        self.pos = end;
        Span {
            off: start as u32,
            len: len as u32,
        }
    }

    /// Borrow the bytes of a previously allocated span.
    #[inline]
    pub fn get(&self, span: Span) -> &[u8] {
        let s = span.off as usize;
        let e = s + span.len as usize;
        &self.buf[s..e]
    }

    /// Borrow the bytes of a span as UTF-8.
    ///
    /// # Panics
    ///
    /// Panics if the span's bytes are not valid UTF-8. Spans created through
    /// [`Interner::intern`] (which only takes `&str`) are always valid; a span
    /// from a raw [`Arena::alloc`] of arbitrary bytes may not be.
    #[inline]
    pub fn get_str(&self, span: Span) -> &str {
        std::str::from_utf8(self.get(span)).expect("arena holds valid UTF-8")
    }

    /// Reset the bump pointer without deallocating backing storage.
    ///
    /// # This invalidates every span previously returned by [`Arena::alloc`]
    ///
    /// If an [`Interner`] is still alive and pointing into this arena, its map
    /// now holds spans that alias whatever is allocated next. Interning after a
    /// reset then returns a span that reads back **different bytes with no
    /// panic** — the worst possible failure for a byte-hash gate. Always call
    /// [`Interner::reset`] on every interner on this arena in the same breath
    /// as this method.
    pub fn reset(&mut self) {
        self.pos = 0;
        // Bumped so a future generation-checking Span can detect this; today
        // it is documentation of the invariant plus a hook for Phase 2.
        self.generation = self.generation.wrapping_add(1);
    }

    /// The number of times this arena has been reset. Every span obtained
    /// before the most recent reset is invalid.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    pub fn used_bytes(&self) -> usize {
        self.pos
    }

    pub fn capacity_bytes(&self) -> usize {
        self.buf.len()
    }
}

/// Deduplicating string interner over an [`Arena`].
///
/// One `Vec<u8>` allocation per *distinct* string (not per record) — the hot
/// loop interns a few thousand distinct endpoint names / terms and then
/// reuses their spans for millions of records.
///
/// # Staleness is detected, not merely documented
///
/// An interner holds spans into its arena, so [`Arena::reset`] invalidates
/// every one of them. Rather than trusting callers to remember (the failure is
/// silent and produces **wrong bytes**, which a SHA gate cannot catch in
/// review), this type records the arena's generation and drops its map when it
/// sees the arena has been reset. Interning is then always consistent with the
/// arena: after a reset, the first `intern` rebuilds from scratch.
///
/// Phase 2 replaces the `HashMap` with an open-addressing table keyed by a
/// 64-bit hash (FNV-1a / xxhash-style) to remove the per-lookup allocation
/// and the `SipHash` cost. The scaffold keeps the safe `HashMap` so the
/// determinism story is trivially auditable.
#[derive(Debug, Default)]
pub struct Interner {
    map: HashMap<Vec<u8>, Span>,
    /// The arena generation these spans were allocated against. A mismatch
    /// means the arena was reset underneath us.
    generation: u32,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `s`, returning its span. Repeated calls with equal bytes
    /// return the same span and allocate nothing new.
    pub fn intern(&mut self, arena: &mut Arena, s: &str) -> Span {
        // Cheap guard against the one silent-corruption path. If the arena
        // was reset, every span in `map` now aliases whatever gets allocated
        // next, so reading one back returns different bytes with no panic.
        if self.generation != arena.generation() {
            self.map.clear();
            self.generation = arena.generation();
        }
        if let Some(&span) = self.map.get(s.as_bytes()) {
            return span;
        }
        let span = arena.alloc(s.as_bytes());
        self.map.insert(s.as_bytes().to_vec(), span);
        span
    }

    /// Number of distinct interned strings.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Drop every remembered span.
    ///
    /// **Call this in the same breath as [`Arena::reset`].** The map holds
    /// spans into the arena; after a reset those spans alias whatever is
    /// allocated next, so `intern` would return a stale span whose bytes have
    /// been overwritten. Because `intern` checks the map first, the corruption
    /// is silent: interning `/v2/orders`, resetting, then interning
    /// `/v2/refunds` makes `get(old_span)` return `"/v2/refund"` — wrong bytes,
    /// no panic, and a byte-hash gate fails with no clue why.
    pub fn reset(&mut self) {
        self.map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_alloc_and_reset() {
        let mut a = Arena::with_capacity(16);
        let s1 = a.alloc(b"hello");
        let s2 = a.alloc(b"world");
        assert_eq!(a.get_str(s1), "hello");
        assert_eq!(a.get_str(s2), "world");
        assert!(a.used_bytes() > 0);
        a.reset();
        assert_eq!(a.used_bytes(), 0);
    }

    #[test]
    fn earlier_spans_survive_growth() {
        let mut a = Arena::with_capacity(4);
        let first = a.alloc(b"stable");
        let _big = a.alloc(&vec![b'x'; 1000]);
        assert_eq!(
            a.get_str(first),
            "stable",
            "arena must not invalidate earlier spans"
        );
    }

    #[test]
    fn interner_dedups_and_coexists() {
        let mut a = Arena::new();
        let mut i = Interner::new();
        // The whole point: many live interned strings at once.
        let s1 = i.intern(&mut a, "/v2/orders/{id}");
        let s2 = i.intern(&mut a, "/v2/checkout");
        let s3 = i.intern(&mut a, "/v2/orders/{id}");
        assert_eq!(s1, s3, "identical strings must share a span");
        assert_ne!(s1, s2);
        assert_eq!(a.get_str(s1), "/v2/orders/{id}");
        assert_eq!(a.get_str(s2), "/v2/checkout");
        assert_eq!(i.len(), 2);
    }
}
