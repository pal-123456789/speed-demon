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
//! streaming version was introduced to avoid -- the board measured 422 MB for a
//! single heap copy of this file, against 254 MB for a streamed one.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{File, read};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::exit;

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Per-thread read buffer. Small enough that twelve of them cost less than the
/// file, large enough that the read syscall is not the bottleneck.
const CHUNK: usize = 4 << 20;

const NTHREADS: usize = 12;

/// A fast, non-cryptographic hasher (the FxHash multiply-rotate-xor).
///
/// The standard library's `HashMap` uses SipHash: DoS-resistant, and far more
/// work than this program needs. Track C hashes millions of region ids and name
/// byte strings, none of it attacker-controlled. Nothing about the ANSWER depends
/// on the hash function, only on set membership, so this changes speed and
/// nothing else; the SHA-256 is the proof.
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

/// Per-country running aggregate.
#[derive(Default)]
struct Agg {
    count: u64,
    id_sum: i64,
    amount_sum: i64,
    amount_min: i64,
    amount_max: i64,
    names: FxSet<Vec<u8>>,
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

/// Fold one thread's aggregate into another. The name sets UNION, because
/// `names_distinct` is a count of distinct values, not a sum of counts.
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
    for n in src.names {
        dst.names.insert(n);
    }
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
fn record_boundaries(path: &str, nthreads: usize) -> std::io::Result<Vec<u64>> {
    const QUOTE: u64 = 0x2222_2222_2222_2222; // b'"' repeated
    const NL: u64 = 0x0a0a_0a0a_0a0a_0a0a; // b'\n' repeated

    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut interior: Vec<u64> = Vec::with_capacity(nthreads - 1);
    let mut targets: Vec<u64> = (1..nthreads)
        .map(|i| len * i as u64 / nthreads as u64)
        .collect();
    let mut next = 0usize;
    let mut buf = vec![0u8; 1 << 20];
    let mut off: u64 = 0;
    let mut parity = 0u8;
    let mut header_end: Option<u64> = None;

    // Records a boundary that ends at absolute offset `end`.
    #[inline]
    fn note(
        end: u64,
        header_end: &mut Option<u64>,
        interior: &mut Vec<u64>,
        targets: &[u64],
        next: &mut usize,
    ) {
        if header_end.is_none() {
            *header_end = Some(end);
            return;
        }
        if *next < targets.len() && end >= targets[*next] {
            interior.push(end);
            while *next < targets.len() && end >= targets[*next] {
                *next += 1;
            }
        }
    }

    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            break;
        }
        let base = off;
        let mut i = 0usize;
        while i + 8 <= k {
            let w = u64::from_le_bytes(buf[i..i + 8].try_into().unwrap());
            let mut m = zero_byte(w ^ QUOTE) | zero_byte(w ^ NL);
            while m != 0 {
                let idx = i + (m.trailing_zeros() as usize >> 3);
                if buf[idx] == b'"' {
                    parity ^= 1;
                } else if parity == 0 {
                    note(base + idx as u64 + 1, &mut header_end, &mut interior, &targets, &mut next);
                }
                m &= m - 1;
            }
            i += 8;
        }
        while i < k {
            let b = buf[i];
            if b == b'"' {
                parity ^= 1;
            } else if b == b'\n' && parity == 0 {
                note(base + i as u64 + 1, &mut header_end, &mut interior, &targets, &mut next);
            }
            i += 1;
        }
        off += k as u64;
    }
    let _ = targets.drain(..);

    let mut pts = Vec::with_capacity(interior.len() + 2);
    pts.push(header_end.unwrap_or(0));
    pts.extend_from_slice(&interior);
    pts.push(len);
    Ok(pts)
}

/// Parse `[start, end)` and return this range's aggregates.
///
/// `start` and `end` are both record boundaries, so the range holds whole records
/// and no record is shared with a neighbour. The parser is the same one the
/// sequential version used; only the framing differs.
fn parse_range(
    path: &str,
    start: u64,
    end: u64,
    lookup: &FxMap<i64, [u8; 3]>,
) -> (FxMap<[u8; 3], Agg>, Agg, bool) {
    let mut groups: FxMap<[u8; 3], Agg> = FxMap::with_capacity_and_hasher(256, FxBuild);
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
            let want = (total - done).min(buf.len() - have);
            if want == 0 {
                break; // a record larger than the buffer cannot occur
            }
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
            let mut cents = digits(recs, &mut pos) * 100;
            if pos + 1 < have && recs[pos] == b'.' {
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

            let agg = match lookup.get(&region) {
                Some(code) => groups.entry(*code).or_insert_with(Agg::new),
                None => {
                    saw_unmatched = true;
                    &mut unmatched
                }
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
            if !agg.names.contains(&name[..]) {
                agg.names.insert(name.clone());
            }
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
    let regions = match read(format!("{data}/regions.csv")) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[track-c] cannot read regions.csv: {e}");
            exit(1);
        }
    };
    let mut lookup: FxMap<i64, [u8; 3]> = FxMap::with_capacity_and_hasher(200_000, FxBuild);
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
                lookup.insert(rid, [regions[p], regions[p + 1], regions[p + 2]]);
                p += 3;
            }
            while p < regions.len() && regions[p] != b'\n' {
                p += 1;
            }
        }
    }

    // ── the split, then twelve threads over disjoint records ────────────────
    let pts = match record_boundaries(&records, NTHREADS) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[track-c] cannot scan records.csv: {e}");
            exit(1);
        }
    };

    let mut parts: Vec<(FxMap<[u8; 3], Agg>, Agg, bool)> = Vec::with_capacity(pts.len());
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(pts.len());
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let path = records.as_str();
            let lookup = &lookup;
            handles.push(scope.spawn(move || parse_range(path, a, b, lookup)));
        }
        for h in handles {
            if let Ok(r) = h.join() {
                parts.push(r);
            }
        }
    });

    // ── merge, then one total order ─────────────────────────────────────────
    let mut groups: FxMap<[u8; 3], Agg> = FxMap::with_capacity_and_hasher(256, FxBuild);
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

    let mut keys: Vec<[u8; 3]> = groups.keys().copied().collect();
    keys.sort_unstable();

    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    for k in keys {
        let a = &groups[&k];
        out.extend_from_slice(&k);
        let _ = write!(out, " {} {} ", a.count, a.id_sum);
        money(a.amount_sum, &mut out);
        out.push(b' ');
        money(a.amount_min, &mut out);
        out.push(b' ');
        money(a.amount_max, &mut out);
        let _ = write!(out, " {} {} {}\n", a.names.len(), a.name_bytes, a.note_bytes);
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
        let _ = write!(out, " {} {} {}\n", a.names.len(), a.name_bytes, a.note_bytes);
    }

    if let Err(e) =
        File::create(format!("{outd}/aggregates.txt")).and_then(|mut f| f.write_all(&out))
    {
        eprintln!("[track-c] cannot write aggregates.txt: {e}");
        exit(1);
    }
}
