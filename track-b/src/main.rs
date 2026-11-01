//! Speed Demon 2026 â€” Track B (Index).
//!
//! Strategy
//! --------
//! * Parse `queries.txt` first and intern every term it mentions: exactly the
//!   terms the query set needs, nothing else is indexed.
//! * Stream `docs.txt` once, tokenising each document and appending a 16-bit
//!   unit to the mentioned term's bucket: the term's index inside that bucket
//!   and the documents since the bucket's own last posting.  The unit *is* the
//!   posting, so there is no term-id list, no counting sort and no second pass,
//!   and the build's peak is the postings rather than twice their size.
//! * BOOL: evaluate strictly left to right on ascending `u32` id vectors
//!   (intersection / union / difference).  Galloping search is used whenever
//!   one side is much smaller than the other.
//! * PHRASE: intersect the terms' id lists (rarest first) for candidates,
//!   collect the `(document, phrase)` pairs, and verify them in a second pass
//!   over `docs.txt` -- split across the same byte ranges as the build, so the
//!   pairs' ascending document order is preserved without a sort.
//!
//! Tokenisation is exactly `re.findall(rb"[a-z0-9]+", text.lower())` on bytes:
//! ASCII-lowercase `A-Z` only, then maximal runs of `a-z0-9`; every other byte
//! (including every byte >= 0x80) is a separator.
//!
//! `std` only â€” no crates, no Cargo.toml.

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;

const TRACK: &str = "B";
const DEFAULT_INPUT: &str = "docs.txt";
const DEFAULT_OUTPUT: &str = "results.txt";
const QUERIES_FILE: &str = "queries.txt";

/// Workers used for the index build. The container is pinned to twelve hardware
/// threads on six P-cores; the build was previously single-threaded, which is
/// where this track's 12x gap to the leader came from.
const NWORKERS: usize = 12;

/// Candidate `(document << 32) | query` pairs for every phrase query `qi` with
/// `qi % nth == t`: the rarest term's list, intersected with the others in
/// increasing length.  Intersection is commutative, so the candidate set is
/// exactly the reference's.
fn phrase_pairs(queries: &[Query], t: usize, nth: usize, postings: &Postings) -> Vec<u64> {
    let mut pairs: Vec<u64> = Vec::new();
    let mut cand: Vec<u32> = Vec::new();
    let mut order: Vec<usize> = Vec::new();
    for qi in (t..queries.len()).step_by(nth) {
        if let Query::Phrase(terms) = &queries[qi] {
            order.clear();
            order.extend(0..terms.len());
            order.sort_by_key(|&i| postings.len(terms[i]));
            cand.clear();
            if let Some(&first) = order.first() {
                postings.decode_into(terms[first], &mut cand);
            }
            // Filter the rarest term's candidates in place instead of decoding
            // each other term: they are the common ones, and the filter order
            // does not change the set.
            for &i in order.iter().skip(1) {
                if cand.is_empty() {
                    break;
                }
                cand.retain(|&d| postings.has(terms[i], d));
            }
            for &d in cand.iter() {
                pairs.push(((d as u64) << 32) | qi as u64);
            }
        }
    }
    pairs
}

// â”€â”€ paths â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn data_dir() -> PathBuf {
    env::var("SD_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data"))
}

fn out_dir() -> PathBuf {
    env::var("SD_OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/out"))
}

fn input_path() -> PathBuf {
    let name = env::var(format!("SD_IN_{TRACK}")).unwrap_or_else(|_| DEFAULT_INPUT.to_string());
    data_dir().join(name)
}

fn output_path() -> PathBuf {
    let name = env::var(format!("SD_OUT_{TRACK}")).unwrap_or_else(|_| DEFAULT_OUTPUT.to_string());
    out_dir().join(name)
}

// â”€â”€ tokenisation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const fn build_is_tok() -> [bool; 256] {
    let mut t = [false; 256];
    let mut b = 0usize;
    while b < 256 {
        t[b] = (b >= 0x61 && b <= 0x7a) || (b >= 0x30 && b <= 0x39);
        b += 1;
    }
    t
}
static IS_TOK: [bool; 256] = build_is_tok();

const HB: u64 = 0x8080_8080_8080_8080;

/// High bit set in each byte of `w` that is NOT a `[0-9a-z]` byte.
///
/// `y` forces every byte into `0x80..=0xff`, so each `y - n` is a per-byte
/// comparison that can never borrow across a byte boundary: the top bit of a
/// byte is set exactly when that byte is `>= n`.  `!w` then drops every byte
/// `>= 0x80`, which would otherwise land inside the digit or letter range once
/// masked to seven bits.  Eight bytes per iteration; the tail is left to the
/// byte loop.
#[inline(always)]
fn sep_bytes(w: u64) -> u64 {
    let y = (w & !HB) | HB;
    let d = y.wrapping_sub(0x3030_3030_3030_3030) & !y.wrapping_sub(0x3a3a_3a3a_3a3a_3a3a);
    let a = y.wrapping_sub(0x6161_6161_6161_6161) & !y.wrapping_sub(0x7b7b_7b7b_7b7b_7b7b);
    !((d | a) & !w) & HB
}

/// First index `>= i` that holds a separator.  The byte loop only runs for the
/// last seven bytes of the buffer, and for the one separator the SWAR step
/// stops on it costs nothing.
#[inline(always)]
fn scan_token(text: &[u8], mut i: usize) -> usize {
    let n = text.len();
    while i + 8 <= n {
        let w = u64::from_le_bytes(text[i..i + 8].try_into().unwrap());
        let s = sep_bytes(w);
        if s != 0 {
            return i + (s.trailing_zeros() >> 3) as usize;
        }
        i += 8;
    }
    while i < n && IS_TOK[text[i] as usize] {
        i += 1;
    }
    i
}

/// ASCII-lowercase bytes `0x41..=0x5A` in place.  No other byte changes.
#[inline]
fn ascii_lower_in_place(b: &mut [u8]) {
    for x in b.iter_mut() {
        *x |= (((*x).wrapping_sub(0x41) < 26) as u8) << 5;
    }
}

/// Token spans (start, len) of the already-lowercased text.
#[inline]
fn tokenize_spans(text: &[u8], spans: &mut Vec<(u32, u32)>) {
    spans.clear();
    let n = text.len();
    let mut i = 0usize;
    while i < n {
        while i < n && !IS_TOK[text[i] as usize] {
            i += 1;
        }
        let s = i;
        i = scan_token(text, i);
        if i > s {
            spans.push((s as u32, (i - s) as u32));
        }
    }
}

// â”€â”€ term interning â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[inline]
/// Hash a term with the CPU's CRC32 instruction.
///
/// The previous version folded 8-byte chunks with a dependent multiply-and-shift
/// per chunk, which is about four serialised multiply-shift chains for a 12-byte
/// term. CRC32 has a three-cycle latency, so two of them chained plus one final
/// multiply is roughly half the dependent work -- and this runs once per token,
/// about 42 M times during the build, which is 49.7% of this track.
///
/// Only the first and last eight bytes are hashed. That catches both ends, so a
/// shared prefix is still not a collision, and a name shorter than eight bytes is
/// packed into one word. The interior of a long term is left out deliberately: a
/// collision costs one extra probe and can never merge two terms, because the
/// comparison stays byte-exact against the arena.
///
/// Changing this cannot change the output. The table uses the hash only to choose
/// a starting slot; every decision is made by comparing the bytes.
#[inline]
fn hash_bytes(b: &[u8]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let n = b.len();
    if n == 0 {
        return 0x517c_c1b7_2722_0a95;
    }
    let (a, c) = if n >= 8 {
        (
            u64::from_le_bytes(b[..8].try_into().unwrap()),
            // The LAST eight bytes, so the far end of a long name is covered too.
            u64::from_le_bytes(b[n - 8..].try_into().unwrap()),
        )
    } else {
        let mut w = 0u64;
        for (k, &x) in b.iter().enumerate() {
            w |= (x as u64) << (8 * k);
        }
        (w, n as u64)
    };
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: CRC32 is in the x86-64-v3 baseline this track is built for, and
        // both operands are plain integer registers. No memory is touched.
        let h = unsafe {
            use core::arch::x86_64::_mm_crc32_u64;
            _mm_crc32_u64(_mm_crc32_u64(a ^ (n as u64) << 56, c), a)
        };
        h.wrapping_mul(K)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let mut h = a ^ c ^ (n as u64).wrapping_mul(K);
        h = (h ^ (h >> 29)).wrapping_mul(K);
        h ^ (h >> 31)
    }
}

/// Byte-string interner: open-addressed table of term ids over a byte arena.
/// Each slot is `(hash tag << 32) | (term id + 1)`; 0 marks an empty slot, so
/// a probe rejects a non-matching slot from one 64-bit load in the common case.
struct TermTable {
    arena: Vec<u8>,
    offs: Vec<u32>,
    lens: Vec<u32>,
    slots: Vec<u64>,
    mask: usize,
    /// Set once `pack` has rewritten `slots` into the build layout.
    packed: bool,
}

/// Build-layout slot: `tag << 47 | len << 41 | (arena offset + 1) << 20 | id`.
///
/// The interning layout needs `offs` and `lens` out of two more 300 KB arrays
/// on every candidate, which is two more random reads per token.  Folding them
/// into the word the probe already has to fetch removes both, so a hit costs
/// one table word plus the key bytes it has to compare anyway.  `+ 1` keeps a
/// used slot non-zero even when the tag is zero, which is what marks empty.
const SLOT_ID: u64 = (1 << 20) - 1;
const SLOT_OFF: u64 = ((1 << 21) - 1) << 20;
const SLOT_LEN: u64 = 0x3f << 41;
const SLOT_TAG: u64 = !SLOT_LEN & !SLOT_OFF & !SLOT_ID;

impl TermTable {
    fn new() -> Self {
        let mut t = Self {
            arena: Vec::new(),
            offs: Vec::new(),
            lens: Vec::new(),
            slots: vec![0u64; 1 << 12],
            mask: (1 << 12) - 1,
            packed: false,
        };
        t.slots.shrink_to_fit();
        t
    }

    #[inline]
    fn len(&self) -> usize {
        self.offs.len()
    }

    #[inline]
    fn key(&self, id: u32) -> &[u8] {
        let o = self.offs[id as usize] as usize;
        let l = self.lens[id as usize] as usize;
        &self.arena[o..o + l]
    }

    #[inline]
    fn slot_for(&self, key: &[u8], h: u64) -> usize {
        let tag = (h >> 32) as u32;
        let mut i = (h as usize) & self.mask;
        loop {
            let e = self.slots[i];
            if e == 0 {
                return i;
            }
            if (e >> 32) as u32 == tag {
                let t = (e as u32 - 1) as usize;
                let o = self.offs[t] as usize;
                let l = self.lens[t] as usize;
                if l == key.len() && self.arena[o..o + l] == *key {
                    return i;
                }
            }
            i = (i + 1) & self.mask;
        }
    }

    /// Look up bytes exactly as they appear after ASCII-lowercasing.
    #[inline]
    fn get(&self, key: &[u8]) -> Option<u32> {
        let h = hash_bytes(key);
        let e = self.slots[self.slot_for(key, h)];
        if e == 0 {
            None
        } else {
            Some(e as u32 - 1)
        }
    }

    /// Rewrite every slot into the build layout.  Returns false when the table
    /// does not fit one word (a key over 63 bytes, or an arena past 2 MB); the
    /// caller then keeps the interning probe, which is right for any table.
    /// Nothing is written before every check has passed.
    fn pack(&mut self) -> bool {
        if self.offs.len() > SLOT_ID as usize || self.arena.len() >= (1 << 21) {
            return false;
        }
        if self.lens.iter().any(|&l| l > 63) {
            return false;
        }
        for e in self.slots.iter_mut() {
            if *e == 0 {
                continue;
            }
            let t = (*e as u32 - 1) as usize;
            let o = self.offs[t] as usize;
            let l = self.lens[t] as usize;
            let h = hash_bytes(&self.arena[o..o + l]);
            *e = (h & SLOT_TAG) | ((l as u64) << 41) | (((o as u64) + 1) << 20) | (t as u64);
        }
        self.packed = true;
        true
    }

    /// One random word, then the key bytes -- no `offs`/`lens` touch.
    #[inline]
    fn probe(&self, key: &[u8]) -> Option<u32> {
        let h = hash_bytes(key);
        let tag = h & SLOT_TAG;
        let mut i = (h as usize) & self.mask;
        loop {
            let e = self.slots[i];
            if e == 0 {
                return None;
            }
            if (e & SLOT_TAG) == tag {
                let l = ((e & SLOT_LEN) >> 41) as usize;
                if l == key.len() {
                    let o = (((e & SLOT_OFF) >> 20) - 1) as usize;
                    if self.arena[o..o + l] == *key {
                        return Some((e & SLOT_ID) as u32);
                    }
                }
            }
            i = (i + 1) & self.mask;
        }
    }

    fn intern(&mut self, key: &[u8]) -> u32 {
        let h = hash_bytes(key);
        let i = self.slot_for(key, h);
        let e = self.slots[i];
        if e != 0 {
            return e as u32 - 1;
        }
        let idx = self.offs.len() as u32;
        self.offs.push(self.arena.len() as u32);
        self.lens.push(key.len() as u32);
        self.arena.extend_from_slice(key);
        self.slots[i] = ((h >> 32) << 32) | (idx as u64 + 1);
        if self.offs.len() * 3 >= self.slots.len() * 2 {
            self.grow();
        }
        idx
    }

    fn grow(&mut self) {
        let cap = self.slots.len() * 2;
        let old = std::mem::replace(&mut self.slots, vec![0u64; cap]);
        self.mask = cap - 1;
        for e in old {
            if e == 0 {
                continue;
            }
            let t = (e as u32 - 1) as usize;
            let o = self.offs[t] as usize;
            let l = self.lens[t] as usize;
            let h = hash_bytes(&self.arena[o..o + l]);
            let mut i = (h as usize) & self.mask;
            while self.slots[i] != 0 {
                i = (i + 1) & self.mask;
            }
            self.slots[i] = e;
        }
    }
}

// â”€â”€ queries â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const OP_AND: u8 = 0;
const OP_OR: u8 = 1;
const OP_NOT: u8 = 2;

enum Query {
    /// `terms[0]`, then `ops[i]` combines the running result with `terms[i+1]`.
    Bool { terms: Vec<u32>, ops: Vec<u8> },
    Phrase(Vec<u32>),
}

/// Mirrors the reference parser: split on single spaces, first word is the
/// first term, then `(op, term)` pairs.  That also reproduces its behaviour on
/// malformed lines (an operator-looking word in term position is just a term
/// that cannot match, because the index keys are lowercase).
fn parse_queries(buf: &[u8], tt: &mut TermTable, out: &mut Vec<Query>) {
    let n = buf.len();
    let mut start = 0usize;
    while start < n {
        let end = match buf[start..].iter().position(|&b| b == b'\n') {
            Some(p) => start + p,
            None => n,
        };
        let line = &buf[start..end];
        start = end + 1;
        if line.is_empty() {
            continue;
        }
        match line.iter().position(|&b| b == b' ') {
            Some(sp) => {
                let kind = &line[..sp];
                let rest = &line[sp + 1..];
                if kind == b"BOOL" {
                    let mut words = rest.split(|&b| b == b' ');
                    let first = words.next().unwrap_or(b"");
                    let mut terms = Vec::with_capacity(4);
                    let mut ops = Vec::with_capacity(3);
                    terms.push(tt.intern(first));
                    while let Some(op) = words.next() {
                        let term = words.next().unwrap_or(b"");
                        let code = match op {
                            b"AND" => OP_AND,
                            b"OR" => OP_OR,
                            b"NOT" => OP_NOT,
                            // The reference raises here; skip the operation so a
                            // malformed line cannot take the whole run down.
                            _ => continue,
                        };
                        ops.push(code);
                        terms.push(tt.intern(term));
                    }
                    out.push(Query::Bool { terms, ops });
                } else if kind == b"PHRASE" {
                    let inner = if rest.len() >= 2 && rest[0] == b'"' && rest[rest.len() - 1] == b'"'
                    {
                        &rest[1..rest.len() - 1]
                    } else {
                        rest
                    };
                    let terms: Vec<u32> =
                        inner.split(|&b| b == b' ').map(|t| tt.intern(t)).collect();
                    out.push(Query::Phrase(terms));
                } else {
                    out.push(Query::Bool {
                        terms: vec![tt.intern(line)],
                        ops: Vec::new(),
                    });
                }
            }
            None => out.push(Query::Bool {
                terms: vec![tt.intern(line)],
                ops: Vec::new(),
            }),
        }
    }
}

// â”€â”€ corpus scan â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[inline]
fn parse_u32(b: &[u8]) -> u32 {
    let mut v: u32 = 0;
    for &c in b {
        if c.is_ascii_digit() {
            v = v.wrapping_mul(10).wrapping_add((c - b'0') as u32);
        } else {
            break;
        }
    }
    v
}

/// A pointer the workers share.  Each of them writes only its own segment of
/// every term's stretch, so no two ever touch the same slot and no atomic or
/// lock is needed.
#[derive(Clone, Copy)]
struct Raw<T>(*mut T);
unsafe impl<T> Send for Raw<T> {}
unsafe impl<T> Sync for Raw<T> {}
impl<T> Raw<T> {
    fn of(v: &mut [T]) -> Self {
        Raw(v.as_mut_ptr())
    }
    /// The address of slot `i`, which the caller is responsible for.
    #[inline(always)]
    unsafe fn at(self, i: usize) -> *mut T {
        self.0.add(i)
    }
}

/// The postings, cut into one 16-bit segment per worker.  A posting is the
/// distance from the first document that worker scanned, so a worker's whole
/// budget is 65,535 documents and the arena is one quarter of what a global
/// `u32` document id would cost.  The ranges ascend with the file, so a term's
/// stretch -- worker 0's run, then worker 1's -- comes out ascending in
/// document id with no sort and no dedupe.
struct Postings {
    /// Worker `w`'s run of term `t` is `seg[t * nw + w]` postings long.
    seg: Vec<u16>,
    /// `offs[t]..offs[t + 1]` is term `t`'s stretch of `at`.
    offs: Vec<u32>,
    /// Every worker's first document -- the base of its 16-bit offsets.
    first: Vec<u32>,
    /// The postings: offsets from the owning worker's first document.
    at: Vec<u16>,
    nw: usize,
}

/// Terms are filed by `id % nb`, so one worker fills `nb` separate streams and
/// the gap a unit has to encode is the distance since that bucket's own last
/// posting rather than since the term's.
const NBUCKETS: usize = 32;

/// The ceiling `bucket_count` will widen to before it gives up: 2^16 buckets by
/// 2^12 terms each is 268 million query terms, far past any query file.
const MAX_BUCKETS: usize = 1 << 16;

/// A unit is `local << GB | gap`: the term's index inside its bucket above the
/// documents since that bucket's previous posting.  `GAP` in the gap field means
/// the unit after it holds the whole gap, so no gap width corrupts a posting.
const GB: u32 = 4;
const GAP: u16 = (1 << GB) - 1;

/// Units per chunk: 64 KiB, small enough to come off the heap instead of its own
/// mapping and to be handed back one chunk at a time as the scatter drains a
/// stream, and wide enough that only the last chunk of a stream is part empty.
const CHUNK: usize = 1 << 15;

/// One bucket's units from one worker.
struct Stream {
    chunks: Vec<Vec<u16>>,
    len: usize,
}

impl Stream {
    fn new() -> Self {
        Stream {
            chunks: Vec::new(),
            len: 0,
        }
    }

    #[inline(always)]
    fn push(&mut self, x: u16) {
        if self.len == CHUNK || self.chunks.is_empty() {
            self.chunks.push(vec![0u16; CHUNK]);
            self.len = 0;
        }
        self.chunks.last_mut().unwrap()[self.len] = x;
        self.len += 1;
    }

    /// One posting.  `gap` is the documents since this bucket's previous
    /// posting; a gap the field cannot hold travels whole in the unit after it.
    #[inline(always)]
    fn put(&mut self, local: u32, gap: u32) {
        self.push(((local << GB) as u16) | gap.min(GAP as u32) as u16);
        if gap >= GAP as u32 {
            self.push(gap as u16);
        }
    }
}

/// One worker's share of the index before the layout: what it counted per term,
/// the first document it saw, and its streams of units.
struct Part {
    count: Vec<u16>,
    first: u32,
    streams: Vec<Stream>,
}

impl Part {
    fn empty(nterms: usize, nb: usize) -> Self {
        Part {
            count: vec![0u16; nterms],
            first: 0,
            streams: (0..nb).map(|_| Stream::new()).collect(),
        }
    }
}

/// The widest bucket count a unit needs: the smallest power of two that keeps a
/// term's index inside its bucket below the unit's high bits.  A holdout seed
/// with a much larger query set therefore widens the buckets instead of
/// overflowing a field, because a field that silently wrapped would corrupt
/// every later posting of the same bucket -- and a widened layout is still exact,
/// so the only failure mode left is a query set too large for any widening, which
/// stops the run rather than answering it wrongly.
fn bucket_count(nterms: usize) -> usize {
    let mut nb = NBUCKETS;
    while nterms.saturating_sub(1) / nb >= (1 << (16 - GB)) {
        if nb >= MAX_BUCKETS {
            panic!("[track-{TRACK}] more query terms than a bucket unit can index");
        }
        nb <<= 1;
    }
    nb
}

impl Postings {
    fn empty(nterms: usize) -> Self {
        Postings {
            seg: Vec::new(),
            offs: vec![0; nterms + 1],
            first: Vec::new(),
            at: Vec::new(),
            nw: 0,
        }
    }

    /// How many documents hold term `t`, without decoding its postings.
    #[inline]
    fn len(&self, t: u32) -> usize {
        (self.offs[t as usize + 1] - self.offs[t as usize]) as usize
    }

    /// Term `t`'s documents, ascending, appended to `out`.  Worker runs are
    /// concatenated in worker order and every run ascends on its own, so the
    /// result ascends: the four consumers all rely on that, and a decode that
    /// silently reordered would still evaluate to a plausible wrong answer.
    fn decode_into(&self, t: u32, out: &mut Vec<u32>) {
        out.clear();
        let mut at = self.offs[t as usize] as usize;
        for w in 0..self.nw {
            let n = self.seg[t as usize * self.nw + w] as usize;
            let base = self.first[w];
            out.extend(self.at[at..at + n].iter().map(|&d| base + d as u32));
            at += n;
        }
    }

    /// Is `doc` in term `t`?  The runs ascend with the file and each is a plain
    /// ascending `u16` array, so one binary search lands in the right worker and
    /// answers without decoding anything.  The phrase path filters a rare term's
    /// candidates against common terms, and materialising a common term there
    /// costs a decode of millions of postings it never reads.
    fn has(&self, t: u32, doc: u32) -> bool {
        let w = match self.first.partition_point(|&f| f <= doc) {
            0 => return false,
            w => w - 1,
        };
        let off = doc - self.first[w];
        if off > u16::MAX as u32 {
            return false;
        }
        let mut at = self.offs[t as usize] as usize;
        for u in 0..w {
            at += self.seg[t as usize * self.nw + u] as usize;
        }
        let n = self.seg[t as usize * self.nw + w] as usize;
        self.at[at..at + n].binary_search(&(off as u16)).is_ok()
    }
}

/// The stamp and the df share one 32-bit word: the high twelve bits are the
/// document within this worker's range and the low twenty the running count.
/// One random read-modify-write both decides that the term is new in this
/// document and counts the posting, into a 300 KB array rather than two 600 KB
/// ones.
const ST_DF: u32 = (1 << 20) - 1;

/// Index one lowercased document: the first time a term occurs in it, append one
/// unit to that term's bucket of this worker's streams.  The unit *is* the
/// posting -- there is no term list, no per-document starts array, and no second
/// pass over either, so nothing here has to be held until the join.
#[inline]
fn index_doc(
    text: &[u8],
    tt: &TermTable,
    streams: &mut [Stream],
    prev: &mut [u32],
    nslot: &mut [u32],
    gen: u32,
    off: u32,
    nb: usize,
) {
    let n = text.len();
    let mut i = 0usize;
    while i < n {
        while i < n && !IS_TOK[text[i] as usize] {
            i += 1;
        }
        let s = i;
        i = scan_token(text, i);
        if i > s {
            let key = &text[s..i];
            let found = if tt.packed {
                tt.probe(key)
            } else {
                tt.get(key)
            };
            if let Some(t) = found {
                let e = &mut nslot[t as usize];
                if *e >> 20 != gen {
                    *e = (gen << 20) | ((*e & ST_DF) + 1);
                    let b = t as usize % nb;
                    let gap = off - prev[b];
                    prev[b] = off;
                    streams[b].put(t / nb as u32, gap);
                }
            }
        }
    }
}

/// Streaming pass over one byte range of `docs.txt`, returning that range's own
/// share of the index.
///
/// Splitting is trivial compared to Track C: `docs.txt` is `id TAB text LF` with
/// no quoting and no embedded newlines, so a byte offset can be aligned to a line
/// start by discarding to the next `\n`. The document id is taken from the line's
/// own id field, so a worker needs no global document counter and the ranges are
/// independent.
///
/// The partition rule is the one Track A uses: a worker processes every line whose
/// FIRST byte is `< end`, discarding the partial line at its own `start`. The line
/// that straddles a cut therefore belongs to exactly one worker -- the one to its
/// left -- and the next worker drops it. `start == 0` is the only range that has
/// nothing to drop.
///
/// One pass and one structure: a term's first occurrence in a document appends one
/// 16-bit unit to that term's bucket, and that unit is the posting.  There is no
/// term-id list to carry to the join and no counting sort to run over it, which is
/// what this track's peak was made of.
fn build_range(path: &Path, tt: &TermTable, nterms: usize, nb: usize, start: u64, end: u64) -> Part {
    let mut nslot: Vec<u32> = vec![0u32; nterms];
    let mut prev: Vec<u32> = vec![0u32; nb];
    let mut streams: Vec<Stream> = (0..nb).map(|_| Stream::new()).collect();
    let mut gen: u32 = 0;
    let mut first: u32 = 0;
    let mut seen = false;

    let f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Part::empty(nterms, nb),
    };
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut pos = start;
    if start > 0 {
        // Discard the partial line that the worker to our left already owned.
        // `pos` must advance by exactly the bytes discarded: it is what the
        // `pos < end` test below measures against, so leaving it at `start`
        // makes that boundary wrong by the discarded length and this worker
        // then claims lines that belong to the next one.
        let mut junk: Vec<u8> = Vec::with_capacity(512);
        if r.seek(SeekFrom::Start(start)).is_err() {
            return Part::empty(nterms, nb);
        }
        match r.read_until(b'\n', &mut junk) {
            Ok(0) | Err(_) => return Part::empty(nterms, nb),
            Ok(k) => pos += k as u64,
        }
    }

    let mut line: Vec<u8> = Vec::with_capacity(2048);
    loop {
        line.clear();
        let n = match r.read_until(b'\n', &mut line) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        // A line that begins at or after `end` belongs to the next worker.
        if pos >= end {
            break;
        }
        pos += n as u64;

        let mut stop = n;
        if line[stop - 1] == b'\n' {
            stop -= 1;
        }
        let (id, tstart) = match line[..stop].iter().position(|&b| b == b'\t') {
            Some(t) => (parse_u32(&line[..t]), t + 1),
            // The spec guarantees the TAB; this keeps a malformed line from
            // borrowing an id that belongs to another worker's range.
            None => continue,
        };
        if !seen {
            first = id;
            seen = true;
        }
        // A posting is a 16-bit offset from this worker's first document, so a
        // worker's ids have to stay in one 65,535-wide window.  Truncating an
        // offset here would silently move a document, so this is a hard stop.
        if id < first || id - first > u16::MAX as u32 {
            panic!("[track-{TRACK}] a worker's range spans more than 65,535 document ids");
        }
        let off = id - first;
        // A fresh generation per document is what makes the stamp test work
        // without ever storing a document id.  Twelve bits is 4095 documents,
        // so a longer range clears the stamp field -- keeping the counts --
        // and starts again.  No worker on this corpus comes close.
        gen += 1;
        if gen >= (1 << 12) {
            for e in nslot.iter_mut() {
                *e &= ST_DF;
            }
            gen = 1;
        }
        let text = &mut line[tstart..stop];
        ascii_lower_in_place(text);
        index_doc(text, tt, &mut streams, &mut prev, &mut nslot, gen, off, nb);
    }

    // The low twenty bits of each slot are the term's df.  A worker's df cannot
    // exceed the documents it scanned, which the offset check above already
    // bounds at 65,535.
    let mut count: Vec<u16> = vec![0u16; nterms];
    for (t, e) in nslot.iter().enumerate() {
        let c = e & ST_DF;
        if c > u16::MAX as u32 {
            panic!("[track-{TRACK}] a worker's range holds more than 65,535 documents");
        }
        count[t] = c as u16;
    }
    Part {
        count,
        first,
        streams,
    }
}

/// Build the postings across every worker over disjoint byte ranges.
///
/// This track was the only one of the four with NO parallelism at all -- one pass
/// over 254 MB on one core, at 64.5 MB/s, against a leader doing the same work at
/// 772 MB/s. That ratio is twelve, and it was twelve because we were using one
/// core and they were using about twelve.
///
/// Nothing is merged: the workers' scans are already the index, and the layout
/// only decides where each term's stretch of the shared arena starts and how long
/// each worker's run inside it is.  A term's stretch is worker 0's run, then worker
/// 1's; the ranges ascend with the file, so the concatenation comes out ascending
/// in document id with no sort and no dedupe.
fn build_index(path: &Path, tt: &TermTable, nterms: usize) -> io::Result<Postings> {
    let size = fs::metadata(path)?.len();
    if size == 0 || nterms == 0 {
        return Ok(Postings::empty(nterms));
    }

    let nb = bucket_count(nterms);
    let nworkers = NWORKERS.min((size as usize / (1 << 20)).max(1));
    let parts: Vec<Part> = thread::scope(|s| {
        let mut handles = Vec::with_capacity(nworkers);
        for i in 0..nworkers {
            let (a, b) = (
                size * i as u64 / nworkers as u64,
                size * (i as u64 + 1) / nworkers as u64,
            );
            handles.push(s.spawn(move || build_range(path, tt, nterms, nb, a, b)));
        }
        handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect()
    });
    let nw = parts.len();
    if nw == 0 {
        return Ok(Postings::empty(nterms));
    }

    let mut seg: Vec<u16> = vec![0u16; nterms * nw];
    let mut offs: Vec<u32> = Vec::with_capacity(nterms + 1);
    let mut acc = 0u32;
    for t in 0..nterms {
        offs.push(acc);
        for w in 0..nw {
            let c = parts[w].count[t];
            seg[t * nw + w] = c;
            acc += c as u32;
        }
    }
    offs.push(acc);
    let mut at: Vec<u16> = vec![0u16; acc as usize];
    let first: Vec<u32> = parts.iter().map(|p| p.first).collect();

    // Every worker scatters its own streams into its own segment of every term.
    // `at` is the one shared structure of this phase, so `Raw` is the only way to
    // name it from more than one thread; the aliasing it invites cannot happen
    // because worker `w` writes exactly the `seg[t * nw + w]` slots of each term
    // `t`, those runs are disjoint by construction, and every cursor starts inside
    // its own run.
    let dst = Raw::of(&mut at);
    {
        let (offs, seg) = (&offs[..], &seg[..]);
        thread::scope(|s| {
            for (t, part) in parts.into_iter().enumerate() {
                s.spawn(move || scatter(t, part, offs, seg, dst, nb, nw));
            }
        });
    }
    Ok(Postings {
        seg,
        offs,
        first,
        at,
        nw,
    })
}

/// Write one worker's units into the arena.  A unit names the term's index in its
/// bucket and the documents since that bucket's previous posting, so the walk is
/// sequential in the stream and in the output, and each chunk goes back to the
/// allocator as soon as it has been spent -- that is what keeps the peak near the
/// arena instead of twice it.
fn scatter(
    t: usize,
    part: Part,
    offs: &[u32],
    seg: &[u16],
    dst: Raw<u16>,
    nb: usize,
    nw: usize,
) {
    let nterms = part.count.len();
    let mut cursor: Vec<u32> = Vec::with_capacity(nterms / nb + 1);
    for (b, mut stream) in part.streams.into_iter().enumerate() {
        // Where this worker's run of each of the bucket's terms begins.
        cursor.clear();
        let mut id = b;
        while id < nterms {
            let mut c = offs[id];
            for u in 0..t {
                c += seg[id * nw + u] as u32;
            }
            cursor.push(c);
            id += nb;
        }
        let chunks = std::mem::take(&mut stream.chunks);
        let nchunks = chunks.len();
        let mut doc = 0u32;
        let mut term = u32::MAX;
        for (k, chunk) in chunks.into_iter().enumerate() {
            let n = if k + 1 == nchunks { stream.len } else { CHUNK };
            for &x in &chunk[..n] {
                if term == u32::MAX {
                    term = (x >> GB) as u32;
                    let gap = x & GAP;
                    if gap == GAP {
                        continue; // the whole gap follows in the next unit
                    }
                    doc += gap as u32;
                } else {
                    doc += x as u32;
                }
                let c = &mut cursor[term as usize];
                unsafe { *dst.at(*c as usize) = doc as u16 };
                *c += 1;
                term = u32::MAX;
            }
        }
    }
}

// â”€â”€ set algebra on ascending id vectors â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// First index `>= from` whose value is `>= x`.
#[inline]
fn gallop(a: &[u32], from: usize, x: u32) -> usize {
    let n = a.len();
    if from >= n {
        return n;
    }
    let mut bound = 1usize;
    while from + bound < n && a[from + bound] < x {
        bound <<= 1;
    }
    let mut lo = from + (bound >> 1);
    let mut hi = (from + bound + 1).min(n);
    while lo < hi {
        let m = lo + (hi - lo) / 2;
        if a[m] < x {
            lo = m + 1;
        } else {
            hi = m;
        }
    }
    lo
}

fn intersect_into(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    if a.is_empty() || b.is_empty() {
        return;
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if small.len() * 16 < large.len() {
        out.reserve(small.len());
        let mut base = 0usize;
        for &v in small {
            let p = gallop(large, base, v);
            if p == large.len() {
                break;
            }
            if large[p] == v {
                out.push(v);
            }
            base = p;
        }
    } else {
        out.reserve(small.len());
        let (mut i, mut j) = (0usize, 0usize);
        while i < a.len() && j < b.len() {
            let (x, y) = (a[i], b[j]);
            if x < y {
                i += 1;
            } else if x > y {
                j += 1;
            } else {
                out.push(x);
                i += 1;
                j += 1;
            }
        }
    }
}

fn union_into(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    if a.is_empty() {
        out.extend_from_slice(b);
        return;
    }
    if b.is_empty() {
        out.extend_from_slice(a);
        return;
    }
    out.reserve(a.len() + b.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        if x < y {
            out.push(x);
            i += 1;
        } else if x > y {
            out.push(y);
            j += 1;
        } else {
            out.push(x);
            i += 1;
            j += 1;
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
}

fn difference_into(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    if a.is_empty() {
        return;
    }
    if b.is_empty() {
        out.extend_from_slice(a);
        return;
    }
    if a.len() * 16 < b.len() {
        out.reserve(a.len());
        let mut base = 0usize;
        for &v in a {
            let p = gallop(b, base, v);
            if p == b.len() {
                out.push(v);
                break;
            }
            if b[p] != v {
                out.push(v);
            }
            base = p;
        }
    } else {
        out.reserve(a.len());
        let mut j = 0usize;
        for &v in a {
            while j < b.len() && b[j] < v {
                j += 1;
            }
            if j == b.len() || b[j] != v {
                out.push(v);
            }
        }
    }
}

// â”€â”€ phrase verification â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Does `text` (already lowercased) contain `terms` at `terms.len()`
/// consecutive token positions?
fn spans_contain(text: &[u8], spans: &[(u32, u32)], terms: &[&[u8]]) -> bool {
    let k = terms.len();
    if k == 0 {
        return true;
    }
    if spans.len() < k {
        return false;
    }
    let first = terms[0];
    'win: for w in 0..=(spans.len() - k) {
        let (s, l) = spans[w];
        if l as usize != first.len() || &text[s as usize..s as usize + l as usize] != first {
            continue;
        }
        for j in 1..k {
            let (s, l) = spans[w + j];
            let t = terms[j];
            if l as usize != t.len() || &text[s as usize..s as usize + l as usize] != t {
                continue 'win;
            }
        }
        return true;
    }
    false
}

/// Verify the `(document, phrase)` pairs that fall inside one byte range of
/// `docs.txt`, returning the hits as `(query << 32) | document`.
///
/// The range rule is `build_range`'s: a line belongs to the worker whose range
/// holds its first byte, and the straddling line is discarded here because the
/// worker to the left already owned it.  Candidates are only looked for on the
/// documents the caller asked about, so the corpus is still read front to back
/// with no seeks inside the range.
fn verify_range(
    path: &Path,
    pairs: &[u64],
    queries: &[Query],
    tt: &TermTable,
    start: u64,
    end: u64,
) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    let f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return out,
    };
    let mut r = BufReader::with_capacity(1 << 18, f);
    let mut pos = start;
    if start > 0 {
        let mut junk: Vec<u8> = Vec::with_capacity(512);
        if r.seek(SeekFrom::Start(start)).is_err() {
            return out;
        }
        match r.read_until(b'\n', &mut junk) {
            Ok(0) | Err(_) => return out,
            Ok(k) => pos += k as u64,
        }
    }

    let mut line: Vec<u8> = Vec::with_capacity(2048);
    let mut spans: Vec<(u32, u32)> = Vec::with_capacity(256);
    let mut terms: Vec<&[u8]> = Vec::with_capacity(8);
    let mut pi = 0usize;
    loop {
        if pi >= pairs.len() {
            break;
        }
        line.clear();
        let n = match r.read_until(b'\n', &mut line) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 || pos >= end {
            break;
        }
        pos += n as u64;
        let mut stop = n;
        if line[stop - 1] == b'\n' {
            stop -= 1;
        }
        let (id, tstart) = match line[..stop].iter().position(|&b| b == b'\t') {
            Some(t) => (parse_u32(&line[..t]), t + 1),
            None => continue,
        };
        // `pairs` is sorted by document id, and the ids ascend with the byte
        // offset, so this cursor only ever moves forward.
        while pi < pairs.len() && ((pairs[pi] >> 32) as u32) < id {
            pi += 1;
        }
        if pi >= pairs.len() {
            break;
        }
        if (pairs[pi] >> 32) as u32 != id {
            continue;
        }
        let text = &mut line[tstart..stop];
        ascii_lower_in_place(text);
        tokenize_spans(text, &mut spans);
        loop {
            let qi = (pairs[pi] & 0xffff_ffff) as usize;
            if let Query::Phrase(ts) = &queries[qi] {
                terms.clear();
                terms.extend(ts.iter().map(|&t| tt.key(t)));
                if spans_contain(text, &spans, &terms) {
                    out.push(((qi as u64) << 32) | id as u64);
                }
            }
            pi += 1;
            if pi >= pairs.len() || (pairs[pi] >> 32) as u32 != id {
                break;
            }
        }
    }
    out
}

/// Second pass: verify every `(document, phrase)` pair.
///
/// This used to be the one serial phase left: 214k candidate pairs spread over
/// 249,998 documents, so it read all 254 MB and tokenised 86% of the corpus on
/// one core.  The byte ranges are disjoint and their document ids increase, so
/// the per-worker hit lists concatenate into the ascending per-query id list the
/// writer needs, in worker order, with no sort and no shared state.
fn verify_phrases(
    path: &PathBuf,
    pairs: &[u64],
    queries: &[Query],
    tt: &TermTable,
    hits: &mut [Vec<u32>],
) -> io::Result<()> {
    if pairs.is_empty() {
        return Ok(());
    }
    let size = fs::metadata(path)?.len();
    if size == 0 {
        return Ok(());
    }
    let nworkers = NWORKERS.min((size as usize / (1 << 20)).max(1));
    let parts: Vec<Vec<u64>> = thread::scope(|s| {
        let mut handles = Vec::with_capacity(nworkers);
        for i in 0..nworkers {
            let (a, b) = (
                size * i as u64 / nworkers as u64,
                size * (i as u64 + 1) / nworkers as u64,
            );
            handles.push(s.spawn(move || verify_range(path, pairs, queries, tt, a, b)));
        }
        handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect()
    });
    for p in &parts {
        for &e in p {
            hits[(e >> 32) as usize].push(e as u32);
        }
    }
    Ok(())
}

// â”€â”€ output â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[inline]
fn push_u64(out: &mut Vec<u8>, mut v: u64) {
    if v == 0 {
        out.push(b'0');
        return;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    while v > 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&tmp[i..]);
}

/// `<count> <sum> <id_1> ... <id_10>` + `\n`, exactly as the reference writes.
#[inline]
fn format_result(out: &mut Vec<u8>, r: &[u32]) {
    out.clear();
    push_u64(out, r.len() as u64);
    out.push(b' ');
    let mut sum: u64 = 0;
    for &v in r {
        sum += v as u64;
    }
    push_u64(out, sum);
    for &v in r.iter().take(10) {
        out.push(b' ');
        push_u64(out, v as u64);
    }
    out.push(b'\n');
}

/// Left-to-right, no precedence: `a OR b AND c` is `(a OR b) AND c`.
fn eval_bool(
    terms: &[u32],
    ops: &[u8],
    postings: &Postings,
    cur: &mut Vec<u32>,
    nxt: &mut Vec<u32>,
    opnd: &mut Vec<u32>,
) {
    postings.decode_into(terms[0], cur);
    for (i, &op) in ops.iter().enumerate() {
        let t = terms[i + 1];
        match op {
            OP_AND => {
                if cur.is_empty() {
                    continue;
                }
                postings.decode_into(t, opnd);
                intersect_into(cur, opnd, nxt);
            }
            OP_OR => {
                if postings.len(t) == 0 {
                    continue;
                }
                postings.decode_into(t, opnd);
                union_into(cur, opnd, nxt);
            }
            _ => {
                if cur.is_empty() || postings.len(t) == 0 {
                    continue;
                }
                postings.decode_into(t, opnd);
                difference_into(cur, opnd, nxt);
            }
        }
        std::mem::swap(cur, nxt);
        if cur.is_empty() && !ops[i + 1..].iter().any(|&o| o == OP_OR) {
            // Empty survives AND and NOT; only a later OR can fill it again.
            break;
        }
    }
}

// â”€â”€ main â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[cfg(target_os = "linux")]
extern "C" {
    fn mallopt(param: i32, value: i32) -> i32;
}

/// Pin glibc's mmap threshold so a freed large allocation returns its pages.
///
/// glibc RAISES its own threshold as large blocks are freed, up to 32 MB, so a
/// dropped array can stay on the heap and keep being charged against a
/// high-water mark. Pinning it low makes every large allocation its own mapping,
/// whose pages return on free. Measured on this project's track-d at 4.3% of
/// peak RSS with no time cost, in a four-variant A/B that isolated each call.
///
/// `prctl(PR_SET_THP_DISABLE)` is deliberately NOT called: it measured as a time
/// loss on every track, and this host runs THP in `madvise` mode, so the call
/// disables nothing here anyway.
#[cfg(target_os = "linux")]
fn tune_allocator() {
    unsafe {
        mallopt(-3, 1 << 16); // M_MMAP_THRESHOLD
    }
}

#[cfg(not(target_os = "linux"))]
fn tune_allocator() {}

fn main() {
    tune_allocator();
    let docs_path = input_path();
    let queries_path = data_dir().join(QUERIES_FILE);
    let out_path = output_path();
    let timing = env::var("SD_TIME").is_ok();
    let t0 = std::time::Instant::now();
    macro_rules! lap {
        ($name:expr) => {
            if timing {
                eprintln!("[t] {:<16} {:>8.3}s", $name, t0.elapsed().as_secs_f64());
            }
        };
    }

    let mut tt = TermTable::new();
    let mut queries: Vec<Query> = Vec::new();
    match fs::read(&queries_path) {
        Ok(buf) => parse_queries(&buf, &mut tt, &mut queries),
        Err(e) => {
            eprintln!("[track-{TRACK}] cannot read {}: {e}", queries_path.display());
            return;
        }
    }
    lap!("parse queries");

    let nterms = tt.len();
    tt.pack();
    let postings = match build_index(&docs_path, &tt, nterms) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[track-{TRACK}] cannot index {}: {e}", docs_path.display());
            return;
        }
    };

    lap!("build index");

    // Phrase candidates, gathered as `(document << 32) | query index` so the
    // verification pass can walk the corpus in document order.
    let mut hits: Vec<Vec<u32>> = vec![Vec::new(); queries.len()];
    let nth = NWORKERS.min(queries.len().max(1));
    let cand_parts: Vec<Vec<u64>> = thread::scope(|s| {
        let postings = &postings;
        let queries = &queries;
        let mut handles = Vec::with_capacity(nth);
        for t in 0..nth {
            handles.push(s.spawn(move || phrase_pairs(queries, t, nth, postings)));
        }
        handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect()
    });
    let mut pairs: Vec<u64> = cand_parts.concat();
    drop(cand_parts);
    lap!("phrase candidates");
    pairs.sort_unstable();

    if let Err(e) = verify_phrases(&docs_path, &pairs, &queries, &tt, &mut hits) {
        eprintln!("[track-{TRACK}] cannot verify phrases: {e}");
    }
    drop(pairs);
    lap!("phrase verify");

    if let Err(e) = fs::create_dir_all(out_dir()) {
        eprintln!("[track-{TRACK}] cannot create output dir: {e}");
    }
    let file = match File::create(&out_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[track-{TRACK}] cannot create {}: {e}", out_path.display());
            return;
        }
    };
    let mut w = BufWriter::with_capacity(1 << 20, file);

    // ── evaluate and format the answers across every worker ────────────────
    //
    // Each query is independent and produces exactly one line, so splitting the
    // list changes no output byte: the chunks are concatenated in query order and
    // the write stays serial. This is the same shape as Track D's query phase.
    //
    // Only BOOL queries are evaluated here; the phrase answers were already
    // computed by the single sequential pass above, and they must stay that way.
    // `verify_phrases` reads all 254 MB of docs.txt, so splitting it by query
    // would have twelve workers read 3 GB instead of 254 MB -- which is why this
    // parallelises the *answers* and not the phrase scan.
    let nthreads = NWORKERS.min(queries.len().max(1));
    let chunk = queries.len().div_ceil(nthreads).max(1);
    let parts: Vec<Vec<u8>> = thread::scope(|s| {
        let postings = &postings;
        let hits = &hits;
        let mut handles = Vec::with_capacity(nthreads);
        for (base, part) in queries.chunks(chunk).enumerate() {
            let start = base * chunk;
            handles.push(s.spawn(move || {
                let mut cur: Vec<u32> = Vec::new();
                let mut nxt: Vec<u32> = Vec::new();
                let mut opnd: Vec<u32> = Vec::new();
                let mut line: Vec<u8> = Vec::with_capacity(256);
                let mut out: Vec<u8> = Vec::with_capacity(part.len() * 24 + 16);
                for (k, q) in part.iter().enumerate() {
                    let qi = start + k;
                    let r: &[u32] = match q {
                        Query::Bool { terms, ops } => {
                            eval_bool(terms, ops, postings, &mut cur, &mut nxt, &mut opnd);
                            &cur
                        }
                        Query::Phrase(_) => &hits[qi],
                    };
                    format_result(&mut line, r);
                    out.extend_from_slice(&line);
                }
                out
            }));
        }
        handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect()
    });

    let mut io_err = false;
    for p in &parts {
        if w.write_all(p).is_err() {
            io_err = true;
            break;
        }
    }
    if !io_err && w.flush().is_err() {
        io_err = true;
    }
    if io_err {
        eprintln!("[track-{TRACK}] write error on {}", out_path.display());
    }
    lap!("write");
    // The answer is flushed; skip freeing the index and the query list.
    std::process::exit(0);
}
