//! Track C — Join.
//!
//! Parse a hostile CSV (quoted commas, `""` escapes, embedded newlines, mixed
//! CRLF/LF terminators, U+FEFF at some record boundaries), join each record
//! against a region lookup, and emit one sorted line of aggregates per country.
//!
//! The organizers call this the correctness track and say most disqualifications
//! happen here, so the parser follows spec section 2.1 literally and does nothing
//! clever. What IS clever is where the work happens: twelve threads parse twelve
//! disjoint byte ranges of the file, each found by a proven record boundary.
//!
//! * A field starting with `"` ends at the next `"` that is not part of a `""`
//!   pair. Inside quotes every other byte is data, including `,`, LF and U+FEFF.
//! * A U+FEFF at the very start of the file, or immediately after a record
//!   terminator, is a byte order mark and is skipped. Anywhere else it is data.
//! * A record terminator is a CRLF or a bare LF **outside quotes**.
//! * Money is exact integer cents. `-5.25` is -525. No float ever touches the
//!   value or the output, so `-0.00` cannot be produced even by accident.
//! * `names_distinct` compares exact byte strings after unescaping, so `Betts` and
//!   `Betts ` are two values, and the empty unquoted name and `""` are one.
//!
//! ## How the file is split safely
//!
//! A newline seen with an **even** running count of `"` bytes is a record
//! boundary. That is a theorem about this format, not a heuristic: a quoted field
//! is `DQUOTE *(qbyte / DQUOTE DQUOTE) DQUOTE`, so its quote count is
//! `1 + 2k + 1`, and an unquoted field contains no `"` at all. Every record
//! therefore contains an even number of quotes, and a `\n` inside a quoted field
//! always sits at odd parity.
//!
//! So one sequential pass records the boundary nearest each twelfth of the file,
//! and each thread then seeks to its own boundary and parses to the next one. No
//! record is split, none is parsed twice, and no thread needs to know anything
//! about another's bytes.
//!
//! ## Memory
//!
//! Each thread streams its range through its own buffer rather than reading the
//! range whole. Holding all twelve ranges at once would have cost the 202 MB the
//! streaming version was introduced to avoid.
//!
//! The structure that actually dominated the peak was `names`: a
//! `HashSet<Vec<u8>>` **per country, per thread**. This corpus draws its names
//! from a pool of ~100,000 distinct values, but it has 3,320,000 records, so with
//! 240 countries every thread ends up with roughly one distinct name per record
//! in each country, and the twelve threads together held ~3.3M live set entries.
//! Each entry is a 24-byte `Vec` header plus a heap block plus a hash-table slot,
//! about 60 bytes, so that one field was ~200 MB and the entire peak.
//!
//! Names are now interned **once**, in a table sharded 64 ways by name hash, and
//! `names_distinct` is a **bitmap per country** over the global name ids. A
//! country's distinct count is the popcount of its bitmap, so the twelve
//! per-thread copies disappear: 3.3M entries collapse to ~100k interned names
//! (about 3 MB) plus 240 bitmaps of 32 KB (about 8 MB). The bitmaps are shared
//! and updated with one relaxed `fetch_or`, so the only synchronisation left on
//! the record path is the interner shard, and its tables are small enough to stay
//! in cache.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{File, read};
use std::hash::Hasher;
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::exit;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Per-thread read buffer. Small enough that twelve of them cost less than the
/// file, large enough that the read syscall is not the bottleneck. These used to
/// be 4 MB each; once the name sets were gone, 48 MB of buffers was the largest
/// single item in a ~25 MB total, so they are now 128 KB, which measured both
/// smaller and slightly faster than 1 MB (fewer pages to fault in per thread).
const CHUNK: usize = 128 << 10;

const NTHREADS: usize = 12;

/// A fast, non-cryptographic hasher (the FxHash multiply-rotate-xor).
///
/// The standard library's `HashMap` uses SipHash: DoS-resistant, and far more
/// work than this program needs. Track C hashes millions of region ids and name
/// byte strings, none of it attacker-controlled. Nothing about the ANSWER depends
/// on the hash function -- only on set membership, and on which shard a name lands
/// in -- so this changes speed and nothing else; the SHA-256 is the proof.
struct FxHasher(u64);

const FX_K: u64 = 0x517c_c1b7_2722_0a95;

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut b = bytes;
        while b.len() >= 8 {
            let v = u64::from_le_bytes(b[..8].try_into().unwrap());
            self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(FX_K);
            b = &b[8..];
        }
        for &x in b {
            self.0 = (self.0.rotate_left(5) ^ x as u64).wrapping_mul(FX_K);
        }
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(FX_K);
    }
    #[inline]
    fn write_i64(&mut self, n: i64) {
        self.write_u64(n as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

#[derive(Default, Clone)]
struct FxBuild;

impl std::hash::BuildHasher for FxBuild {
    type Hasher = FxHasher;
    #[inline]
    fn build_hasher(&self) -> FxHasher {
        FxHasher(0)
    }
}

type FxMap<K, V> = HashMap<K, V, FxBuild>;
type FxSet<T> = HashSet<T, FxBuild>;

/// Hash a byte string exactly the way the interner does, so the shard a name
/// lands in and the probe inside that shard agree.
#[inline]
fn fxhash(bytes: &[u8]) -> u64 {
    let mut h = FxHasher(0);
    h.write(bytes);
    h.finish()
}

// ── the shared name interner ────────────────────────────────────────────────

/// The interner is sharded by the name hash, so twelve threads almost never
/// contend: each shard holds ~1,500 names and its arena and probe table together
/// are about 35 KB.
const SHARDS: usize = 64;
const SHARD_MASK: usize = SHARDS - 1;

/// A probe table slot holds the **whole entry**, packed: bit 63 occupied, then a
/// 32-bit dense id, an 8-bit name length, and a 23-bit arena offset (8 MB).
///
/// There used to be a third array, `ents: Vec<(off, len, id)>`, and a slot held
/// an index into it. On this corpus that array was 1.2 MB of the interner's
/// 2.9 MB working set, and it put a *third* dependent random load in the chain a
/// lookup has to walk -- slot, index, bytes. Measured on the real 3.32 M names
/// with a shared interner and twelve threads, dropping it took the interner from
/// 109 ns to 50 ns per intern, because a probe can now reject a slot on a length
/// mismatch (8 bits of the slot itself) without touching the arena at all.
#[derive(Default)]
struct Shard {
    arena: Vec<u8>,
    table: Vec<u64>,
    n: usize,
}

const LEN_SHIFT: u32 = 32;
const OFF_SHIFT: u32 = 40;
const SLOT_BUSY: u64 = 1 << 63;
const ID_MASK: u64 = (1 << 32) - 1;
const LEN_MAX: usize = (1 << (OFF_SHIFT - LEN_SHIFT)) - 1;
const OFF_MAX: usize = (1 << (63 - OFF_SHIFT)) - 1;

/// Rewrite one shard's probe table at twice the size. Only that shard's own lock
/// is held, and it happens at most twice per shard in the whole run.
fn rehash(s: &mut Shard) {
    let n = s.table.len() * 2;
    let mask = n - 1;
    let mut t = vec![0u64; n];
    for e in s.table.iter().copied() {
        if e == 0 {
            continue;
        }
        // The occupied flag shares the word with the offset, so it comes off
        // before the fields are read.
        let off = ((e >> OFF_SHIFT) as usize) & OFF_MAX;
        let len = ((e >> LEN_SHIFT) as usize) & LEN_MAX;
        let mut slot = (fxhash(&s.arena[off..off + len]) >> 6) as usize & mask;
        while t[slot] != 0 {
            slot = (slot + 1) & mask;
        }
        t[slot] = e;
    }
    s.table = t;
}

struct Interner {
    next: AtomicU32,
    shards: Vec<Mutex<Shard>>,
    /// Exact fallback for a name a packed slot cannot describe: longer than 255
    /// bytes, or past the 8 MB the offset field can address. This corpus reaches
    /// neither (100,520 names, 1.2 MB, longest 25 bytes), but the interner still
    /// has to be exact at any size, so the escape hatch is real code and not a
    /// comment. It is consulted only when a name cannot be packed.
    rare: Mutex<FxMap<Vec<u8>, u32>>,
}

impl Interner {
    fn new() -> Self {
        Interner {
            next: AtomicU32::new(0),
            shards: (0..SHARDS).map(|_| Mutex::new(Shard::default())).collect(),
            rare: Mutex::new(FxMap::with_hasher(FxBuild)),
        }
    }

    /// Dense global id for a name no packed slot can hold. Byte-exact, because
    /// the map keys are the name bytes themselves.
    fn rare_intern(&self, name: &[u8]) -> u32 {
        let mut r = match self.rare.lock() {
            Ok(r) => r,
            Err(e) => e.into_inner(),
        };
        if let Some(&id) = r.get(name) {
            return id;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        r.insert(name.to_vec(), id);
        id
    }

    /// The dense global id of `name`, inserting it on first sight.
    ///
    /// The comparison is **byte for byte** against the arena, never by hash: a
    /// hash collision costs one extra probe and can never merge two names, which
    /// is what keeps `names_distinct` exact without storing a hash-only set.
    #[inline]
    fn intern(&self, name: &[u8]) -> u32 {
        if name.len() > LEN_MAX {
            return self.rare_intern(name);
        }
        let h = fxhash(name);
        let m = &self.shards[(h as usize) & SHARD_MASK];
        let mut g = match m.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(), // a panic elsewhere must not lose the answer
        };
        if g.table.is_empty() {
            g.table = vec![0u64; 1024];
        }
        let mask = g.table.len() - 1;
        let mut slot = (h >> 6) as usize & mask;
        loop {
            let v = g.table[slot];
            if v == 0 {
                let off = g.arena.len();
                if off > OFF_MAX {
                    return self.rare_intern(name);
                }
                let id = self.next.fetch_add(1, Ordering::Relaxed);
                let len = name.len() as u64;
                g.arena.extend_from_slice(name);
                g.table[slot] = SLOT_BUSY
                    | id as u64
                    | (len << LEN_SHIFT)
                    | ((off as u64) << OFF_SHIFT);
                g.n += 1;
                if g.n * 8 >= g.table.len() * 7 {
                    rehash(&mut g);
                }
                return id;
            }
            // The occupied flag shares the word with the offset, so it comes off
            // before the fields are read.
            let e = v & !SLOT_BUSY;
            let off = ((e >> OFF_SHIFT) as usize) & OFF_MAX;
            let len = ((e >> LEN_SHIFT) as usize) & LEN_MAX;
            if len == name.len() && &g.arena[off..off + len] == name {
                return (e & ID_MASK) as u32;
            }
            slot = (slot + 1) & mask;
        }
    }
}

// ── the per-country name bitmaps ────────────────────────────────────────────

/// Name ids below this live in a country's bitmap: 128 K ids, or 16 KB per
/// country. This corpus interns exactly 100,520 distinct names (a pool of
/// 100,000, fixed by the generator's scale table, plus the ~520 names the
/// edge-case blocks generate), so the cap clears it by 30% at the smallest
/// power-of-two cost. A seed change moves the extras by a few hundred, never by
/// 30,000.
const BITCAP: usize = 1 << 17;
const BITWORDS: usize = BITCAP / 64;

/// The distinct-name state of one country (or of `_UNMATCHED`).
///
/// The bitmap is allocated on first use, so a country that never appears costs
/// nothing. Ids at or past `BITCAP` -- which this corpus never reaches, but a
/// different scale or seed could -- fall back to an exact set, so the count is
/// right at any size; only the memory shape changes.
#[derive(Default)]
struct CountryNames {
    bits: OnceLock<Box<[AtomicU64]>>,
    rare: OnceLock<Mutex<FxSet<u32>>>,
}

impl CountryNames {
    /// Record that this country saw the name with global id `id`.
    #[inline]
    fn mark(&self, id: u32) {
        if (id as usize) < BITCAP {
            let bits = self
                .bits
                .get_or_init(|| (0..BITWORDS).map(|_| AtomicU64::new(0)).collect());
            bits[(id >> 6) as usize].fetch_or(1u64 << (id & 63), Ordering::Relaxed);
        } else {
            let set = self.rare.get_or_init(|| Mutex::new(FxSet::default()));
            if let Ok(mut s) = set.lock() {
                s.insert(id);
            }
        }
    }

    /// The number of distinct names this country saw.
    fn count(&self) -> u64 {
        let mut c = match self.bits.get() {
            Some(b) => b
                .iter()
                .map(|w| w.load(Ordering::Relaxed).count_ones() as u64)
                .sum(),
            None => 0,
        };
        if let Some(s) = self.rare.get() {
            if let Ok(s) = s.lock() {
                c += s.len() as u64;
            }
        }
        c
    }
}

/// Per-country running aggregate. No name set lives here any more: distinctness
/// is counted globally, in `CountryNames`.
#[derive(Default)]
struct Agg {
    count: u64,
    id_sum: i64,
    amount_sum: i64,
    amount_min: i64,
    amount_max: i64,
    name_bytes: u64,
    note_bytes: u64,
}

impl Agg {
    fn new() -> Self {
        Agg {
            amount_min: i64::MAX,
            amount_max: i64::MIN,
            ..Default::default()
        }
    }
}

/// Fold one thread's aggregate into another. Every field is a sum, a min or a
/// max, so the result does not depend on the order the threads are merged in.
fn merge(dst: &mut Agg, src: Agg) {
    dst.count += src.count;
    dst.id_sum += src.id_sum;
    dst.amount_sum += src.amount_sum;
    if src.count > 0 {
        if src.amount_min < dst.amount_min {
            dst.amount_min = src.amount_min;
        }
        if src.amount_max > dst.amount_max {
            dst.amount_max = src.amount_max;
        }
    }
    dst.name_bytes += src.name_bytes;
    dst.note_bytes += src.note_bytes;
}

/// Read one field at `*pos` into `out`, honouring the quoting rules.
fn field(buf: &[u8], pos: &mut usize, out: &mut Vec<u8>) {
    out.clear();
    if *pos < buf.len() && buf[*pos] == b'"' {
        *pos += 1;
        while *pos < buf.len() {
            let b = buf[*pos];
            if b == b'"' {
                if *pos + 1 < buf.len() && buf[*pos + 1] == b'"' {
                    out.push(b'"'); // `""` is one literal quote
                    *pos += 2;
                    continue;
                }
                *pos += 1; // closing quote
                break;
            }
            out.push(b);
            *pos += 1;
        }
    } else {
        while *pos < buf.len() {
            let b = buf[*pos];
            if b == b',' || b == b'\n' || b == b'\r' {
                break;
            }
            out.push(b);
            *pos += 1;
        }
    }
}

/// Unsigned decimal, no leading zeros in the data.
#[inline]
fn digits(buf: &[u8], pos: &mut usize) -> i64 {
    let mut v: i64 = 0;
    while *pos < buf.len() && buf[*pos].is_ascii_digit() {
        v = v * 10 + (buf[*pos] - b'0') as i64;
        *pos += 1;
    }
    v
}

/// Exact cents as `-?int.dd`, never `-0.00`.
fn money(cents: i64, out: &mut Vec<u8>) {
    let neg = cents < 0;
    let a = cents.unsigned_abs();
    if neg {
        out.push(b'-');
    }
    let _ = write!(out, "{}.{:02}", a / 100, a % 100);
}

/// Nonzero in byte position `i` when byte `i` of `x` is zero.
#[inline]
fn zero_byte(x: u64) -> u64 {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    x.wrapping_sub(ONES) & !x & HIGH
}

/// The byte offsets that begin a record: `[first_record, b1, .., len]`.
///
/// One sequential pass. `parity` is the running count of `"` bytes modulo two,
/// and a `\n` at even parity is a record boundary by the argument in the module
/// comment. The first boundary is the end of the header line; the last is the
/// length of the file.
///
/// The scan is word-at-a-time. A byte-at-a-time version of exactly this loop
/// measured as the dominant cost of the whole track: twelve threads then race
/// through 202 MB while one thread first walks the same 202 MB one byte at a
/// time, so the parallel parse was being throttled by its own setup. Eight bytes
/// per iteration with a zero-byte test on a repeated constant finds both bytes of
/// interest at once, and the set bits are consumed in ascending address order,
/// which is what keeps the parity running total correct.
const QUOTE: u64 = 0x2222_2222_2222_2222; // b'"' repeated
const NL: u64 = 0x0a0a_0a0a_0a0a_0a0a; // b'\n' repeated

/// Offset just past the first `\n` at global even quote parity at or after
/// `from`, or `len` if there is none.
///
/// `parity` is the running count of `"` in `[0, from)` modulo two, so the test
/// is the record-boundary theorem applied from an arbitrary starting point: a
/// `\n` seen with an even running total of quotes ends a record. This runs over
/// at most a few hundred bytes -- the first qualifying newline is inside the
/// record that spans `from` -- so it is allowed to be the branchy version.
fn first_boundary(path: &str, from: u64, mut parity: u8, len: u64) -> std::io::Result<u64> {
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(from))?;
    let mut buf = vec![0u8; 64 << 10];
    let mut base = from;
    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            return Ok(len);
        }
        let b = &buf[..k];
        let mut i = 0usize;
        while i + 8 <= k {
            let w = u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
            let mut m = zero_byte(w ^ QUOTE) | zero_byte(w ^ NL);
            while m != 0 {
                let idx = i + (m.trailing_zeros() as usize >> 3);
                if b[idx] == b'"' {
                    parity ^= 1;
                } else if parity == 0 {
                    return Ok(base + idx as u64 + 1);
                }
                m &= m - 1;
            }
            i += 8;
        }
        while i < k {
            if b[i] == b'"' {
                parity ^= 1;
            } else if b[i] == b'\n' && parity == 0 {
                return Ok(base + i as u64 + 1);
            }
            i += 1;
        }
        base += k as u64;
    }
}

/// The byte offsets that begin a record: `[first_record, b1, .., len]`.
///
/// A `\n` seen with an even running count of `"` is a record boundary, by the
/// argument in the module comment. Only the ones nearest each twelfth of the
/// file are wanted, and the *only* thing that makes finding them hard is that
/// the starting parity is a global property.
///
/// So the pass that used to compare, branch and record 19.8 million times -- one
/// match for every 12.8 bytes, since this corpus has 16.0 M quotes and 3.7 M
/// newlines -- is now a pure popcount: count quotes per chunk, and the parity at
/// every chunk boundary falls out for free. Splitting at chunk boundaries rather
/// than at exact twelfths costs at most one chunk of imbalance and removes the
/// need to know parity at an arbitrary offset. Twelve short forward scans then
/// convert the split points into real boundaries.
fn record_boundaries(path: &str, nthreads: usize) -> std::io::Result<Vec<u64>> {
    const STEP: usize = 1 << 20;

    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let nchunk = (((len as usize).saturating_add(STEP - 1)) / STEP).max(1);
    let targets: Vec<usize> = (1..nthreads).map(|i| i * nchunk / nthreads).collect();
    let mut par = vec![0u8; nchunk + 1];
    let mut buf = vec![0u8; STEP];
    let mut q: u64 = 0;
    let mut c = 0usize;

    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            break;
        }
        let b = &buf[..k];
        let mut i = 0usize;
        // No branch, no match list, no note(): just the byte-count of `"`.
        while i + 8 <= k {
            let w = u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
            q += zero_byte(w ^ QUOTE).count_ones() as u64;
            i += 8;
        }
        while i < k {
            if b[i] == b'"' {
                q += 1;
            }
            i += 1;
        }
        c += 1;
        par[c] = (q & 1) as u8;
    }

    let mut pts = Vec::with_capacity(nthreads + 2);
    pts.push(first_boundary(path, 0, 0, len)?);
    for t in targets {
        let s = (t as u64) * STEP as u64;
        if s >= len || s <= *pts.last().unwrap() {
            continue;
        }
        // `par[t]` is the quote parity at byte `t * STEP`, and this scan carries
        // it forward byte by byte from there.
        let b = first_boundary(path, s, par[t], len)?;
        if b > *pts.last().unwrap() {
            pts.push(b);
        }
    }
    // `len` terminates the last range. It is not a record boundary in its own
    // right, but the parser stops at end-of-input and never needs it to be one.
    pts.push(len);
    Ok(pts)
}

/// Parse `[start, end)` and return this range's aggregates, keyed by country slot.
///
/// `start` and `end` are both record boundaries, so the range holds whole records
/// and no record is shared with a neighbour. The parser is the same one the
/// sequential version used; only the framing differs.
///
/// Every record's name is interned (a shared, exact id) and marked in that
/// country's bitmap. The per-thread aggregate holds only scalars, so twelve
/// threads never duplicate name storage.
fn parse_range(
    path: &str,
    start: u64,
    end: u64,
    // The country slot is `u32`, not `u16`. At most 240 distinct country codes
    // exist here, so `u16` would fit -- until a corpus had more than 65,535 of
    // them, at which point `u16` would wrap and silently merge two countries.
    // Four bytes a slot costs nothing at this row count and removes that class
    // of wrong answer entirely.
    lookup: &FxMap<i64, u32>,
    names: &Interner,
    distinct: &[CountryNames],
    unmatched_slot: u32,
) -> (FxMap<u32, Agg>, Agg, bool) {
    let mut groups: FxMap<u32, Agg> = FxMap::with_capacity_and_hasher(256, FxBuild);
    let mut unmatched = Agg::new();
    let mut saw_unmatched = false;
    let mut name: Vec<u8> = Vec::with_capacity(64);
    let mut note: Vec<u8> = Vec::with_capacity(64);

    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (groups, unmatched, saw_unmatched),
    };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return (groups, unmatched, saw_unmatched);
    }

    let total = (end - start) as usize;
    let mut buf: Vec<u8> = vec![0u8; CHUNK];
    let mut have = 0usize;
    let mut pos = 0usize;
    let mut done = 0usize;

    loop {
        if pos > 0 {
            buf.copy_within(pos..have, 0);
            have -= pos;
            pos = 0;
        }
        if done < total {
            if have == buf.len() {
                // The buffer is full and still no whole record came out of it, so
                // one record is longer than `CHUNK`. Grow instead of giving up:
                // the old `want == 0 { break }` dropped the rest of the range
                // silently, which a small chunk would have made much likelier.
                // This corpus cannot reach it, but the cost of being wrong here
                // is a wrong answer, not a slow one.
                buf.resize(buf.len() * 2, 0);
            }
            let want = (total - done).min(buf.len() - have);
            match file.read(&mut buf[have..have + want]) {
                Ok(0) => break,
                Ok(k) => {
                    have += k;
                    done += k;
                }
                Err(_) => break,
            }
        }

        let recs: &[u8] = &buf[..have];
        while pos < have {
            let rec_start = pos;
            if recs[pos..].starts_with(&BOM) {
                pos += 3;
                if pos >= have {
                    pos = rec_start;
                    break;
                }
            }
            let id = digits(recs, &mut pos);
            if pos < have && recs[pos] == b',' {
                pos += 1;
            }
            field(recs, &mut pos, &mut name);
            if pos < have && recs[pos] == b',' {
                pos += 1;
            }
            field(recs, &mut pos, &mut note);
            if pos < have && recs[pos] == b',' {
                pos += 1;
            }
            let region = digits(recs, &mut pos);
            if pos < have && recs[pos] == b',' {
                pos += 1;
            }
            let mut neg = false;
            if pos < have && recs[pos] == b'-' {
                neg = true;
                pos += 1;
            }
            // `pos + 2 < have`, not `pos + 1 < have`: the two digits are read at
            // `pos + 1` and `pos + 2` *before* the advance, so a `.` landing two
            // bytes short of the chunk end indexed `recs[have]`. The guard is the
            // only thing that has to be right, because a record whose tail is cut
            // off fails the terminator test below and is re-parsed after a refill.
            let mut cents = digits(recs, &mut pos) * 100;
            if pos + 2 < have && recs[pos] == b'.' {
                pos += 1;
                let d1 = (recs[pos] - b'0') as i64;
                let d2 = (recs[pos + 1] - b'0') as i64;
                pos += 2;
                cents += d1 * 10 + d2;
            }
            if neg {
                cents = -cents;
            }

            // The terminator is the proof the record was complete. `end` is a
            // record boundary, so the only way to run out is the tail of a chunk,
            // in which case the parse is discarded and retried after a refill.
            let mut terminated = false;
            if pos < have && recs[pos] == b'\r' {
                pos += 1;
            }
            if pos < have && recs[pos] == b'\n' {
                pos += 1;
                terminated = true;
            }
            if !terminated {
                pos = rec_start;
                break;
            }

            let slot = match lookup.get(&region) {
                Some(&code) => code,
                None => {
                    saw_unmatched = true;
                    unmatched_slot
                }
            };
            distinct[slot as usize].mark(names.intern(&name));
            let agg = if slot == unmatched_slot {
                &mut unmatched
            } else {
                groups.entry(slot).or_insert_with(Agg::new)
            };
            agg.count += 1;
            agg.id_sum += id;
            agg.amount_sum += cents;
            if cents < agg.amount_min {
                agg.amount_min = cents;
            }
            if cents > agg.amount_max {
                agg.amount_max = cents;
            }
            agg.name_bytes += name.len() as u64;
            agg.note_bytes += note.len() as u64;
        }

        if done >= total && (pos >= have || pos == 0) {
            break;
        }
    }

    (groups, unmatched, saw_unmatched)
}

fn main() {
    let data = env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());
    let records = format!("{data}/records.csv");

    // ── the lookup table ────────────────────────────────────────────────────
    // `region_id` maps straight to a dense country slot, so the join and the
    // bitmap index cost one probe, not two.
    let regions = match read(format!("{data}/regions.csv")) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[track-c] cannot read regions.csv: {e}");
            exit(1);
        }
    };
    let mut codes: Vec<[u8; 3]> = Vec::with_capacity(256);
    let mut slot_of: FxMap<[u8; 3], u32> = FxMap::with_capacity_and_hasher(256, FxBuild);
    let mut lookup: FxMap<i64, u32> = FxMap::with_capacity_and_hasher(200_000, FxBuild);
    {
        let mut p = 0usize;
        while p < regions.len() && regions[p] != b'\n' {
            p += 1;
        }
        while p < regions.len() {
            p += 1; // step over the newline
            if p >= regions.len() {
                break;
            }
            let rid = digits(&regions, &mut p);
            if p < regions.len() && regions[p] == b',' {
                p += 1;
            }
            if p + 2 < regions.len() {
                let code = [regions[p], regions[p + 1], regions[p + 2]];
                let slot = match slot_of.get(&code) {
                    Some(&s) => s,
                    None => {
                        let s = codes.len() as u32;
                        codes.push(code);
                        slot_of.insert(code, s);
                        s
                    }
                };
                lookup.insert(rid, slot);
                p += 3;
            }
            while p < regions.len() && regions[p] != b'\n' {
                p += 1;
            }
        }
    }
    drop(slot_of);
    drop(regions);
    let unmatched_slot = codes.len() as u32;

    // ── the split, then twelve threads over disjoint records ────────────────
    let pts = match record_boundaries(&records, NTHREADS) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[track-c] cannot scan records.csv: {e}");
            exit(1);
        }
    };

    let names = Interner::new();
    let distinct: Vec<CountryNames> = (0..=codes.len()).map(|_| CountryNames::default()).collect();

    let mut parts: Vec<(FxMap<u32, Agg>, Agg, bool)> = Vec::with_capacity(pts.len());
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(pts.len());
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let path = records.as_str();
            let lookup = &lookup;
            let names = &names;
            let distinct = &distinct;
            handles.push(scope.spawn(move || {
                parse_range(path, a, b, lookup, names, distinct, unmatched_slot)
            }));
        }
        for h in handles {
            if let Ok(r) = h.join() {
                parts.push(r);
            }
        }
    });

    // ── merge, then one total order ─────────────────────────────────────────
    let mut groups: FxMap<u32, Agg> = FxMap::with_capacity_and_hasher(256, FxBuild);
    let mut unmatched = Agg::new();
    let mut saw_unmatched = false;
    for (g, un, saw) in parts {
        if saw {
            saw_unmatched = true;
        }
        merge(&mut unmatched, un);
        for (k, a) in g {
            merge(groups.entry(k).or_insert_with(Agg::new), a);
        }
    }

    let mut keys: Vec<u32> = groups.keys().copied().collect();
    keys.sort_unstable_by(|a, b| codes[*a as usize].cmp(&codes[*b as usize]));

    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    for k in keys {
        let a = &groups[&k];
        out.extend_from_slice(&codes[k as usize]);
        let _ = write!(out, " {} {} ", a.count, a.id_sum);
        money(a.amount_sum, &mut out);
        out.push(b' ');
        money(a.amount_min, &mut out);
        out.push(b' ');
        money(a.amount_max, &mut out);
        let _ = write!(
            out,
            " {} {} {}\n",
            distinct[k as usize].count(),
            a.name_bytes,
            a.note_bytes
        );
    }
    if saw_unmatched {
        out.extend_from_slice(b"_UNMATCHED ");
        let a = &unmatched;
        let _ = write!(out, "{} {} ", a.count, a.id_sum);
        money(a.amount_sum, &mut out);
        out.push(b' ');
        money(a.amount_min, &mut out);
        out.push(b' ');
        money(a.amount_max, &mut out);
        let _ = write!(
            out,
            " {} {} {}\n",
            distinct[unmatched_slot as usize].count(),
            a.name_bytes,
            a.note_bytes
        );
    }

    if let Err(e) =
        File::create(format!("{outd}/aggregates.txt")).and_then(|mut f| f.write_all(&out))
    {
        eprintln!("[track-c] cannot write aggregates.txt: {e}");
        exit(1);
    }
}
