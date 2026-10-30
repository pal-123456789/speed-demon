//! Track A — Ingest.
//!
//! Group 3.1M log lines by (hour, endpoint) and emit, per group: the request
//! count, the p50/p95/p99 latency by nearest rank, the 5xx count and the error
//! rate in integer parts per million.
//!
//! Pipeline (scan → merge → score → print), and why it is shaped this way:
//!
//! * **The hour is the first 13 bytes.** `YYYY-MM-DDTHH` is a prefix of the
//!   RFC 3339 timestamp and the field is fixed width, so byte order is
//!   chronological order and grouping needs no calendar arithmetic at all.
//!
//! * **Twelve byte ranges, twelve threads, no shared state.** The file is cut
//!   at `size*i/12` and each worker opens its OWN handle and seeks to its slice.
//!   The split is by byte, not by line, so the boundary rule matters and is
//!   spelled out at `scan`.
//!
//! * **The group key is the group.** Key = `hour[13] || endpoint`, assembled in
//!   one reusable buffer, so the lookup borrows `[u8]` and allocates nothing on
//!   a hit; and because the key is the exact bytes the output sorts on, sorting
//!   keys *is* sorting rows. No intern tables, no id indirection.
//!
//! * **Every serial stage was removed.** Workers sort their own rows before
//!   returning, so the global order is a 12-list merge (n·log2 12 compares)
//!   instead of one 127k-element sort; the percentile pass is split over the
//!   same 12 threads; and each rank is selected inside the prefix that still
//!   contains it, so p95 and p50 do not re-partition the whole group.
//!
//! * **Streamed, never slurped.** A worker sees its slice through one 512 KiB
//!   buffer it recycles and parses lines straight out of it, so the file is
//!   copied once and no per-line allocation happens. Twelve buffers is the
//!   entire input-side footprint.
//!
//! * **Merge order cannot matter.** Percentiles are order statistics of a
//!   group's latency multiset and counts are sums, so any merge order yields the
//!   same bytes. Verified: reversing the merge order hashes identically.

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::hash::{BuildHasherDefault, Hasher};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::iter::Peekable;
use std::process::exit;
use std::thread;
use std::vec::IntoIter;

/// 6 physical cores with SMT: 12 slices keep every hardware thread fed.
const NTHREADS: usize = 12;
/// Per-worker read buffer. Deliberately not several MiB: M is scored, and
/// twelve buffers are the whole input-side footprint. 512 KiB still amortises
/// the ~20 MB each worker reads down to ~40 reads.
const CHUNK: usize = 1 << 19;

/// One (hour, endpoint) group: its latencies, its 5xx count, its percentiles.
#[derive(Default)]
struct Group {
    latencies: Vec<u32>,
    errors: u32,
    p50: u32,
    p95: u32,
    p99: u32,
}

/// A row is the raw key (`hour[13] || endpoint`) plus its group.
type Row = (Box<[u8]>, Group);
type Map = HashMap<Box<[u8]>, Group, BuildHasherDefault<Fx>>;

/// Multiply-xor hasher (FxHash shape). std's SipHash spends real time on the
/// ~45-byte keys this hot loop hashes 3.1M times; keys are not attacker-chosen
/// here, and bucket order is unobservable because the output is sorted.
#[derive(Default)]
struct Fx(u64);

impl Fx {
    #[inline]
    fn mix(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for Fx {
    #[inline]
    fn write(&mut self, mut b: &[u8]) {
        while b.len() >= 8 {
            let (head, tail) = b.split_at(8);
            self.mix(u64::from_le_bytes(head.try_into().unwrap()));
            b = tail;
        }
        if !b.is_empty() {
            let mut w = 0u64;
            for (i, &x) in b.iter().enumerate() {
                w |= (x as u64) << (8 * i);
            }
            self.mix(w);
        }
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// Index of the next `\n` in `b`, eight bytes at a time. This scans the whole
/// 238 MB, so the byte-at-a-time loop it replaces was a real cost.
#[inline]
fn nl(b: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 8 <= b.len() {
        let mut w = [0u8; 8];
        w.copy_from_slice(&b[i..i + 8]);
        let x = u64::from_le_bytes(w) ^ 0x0a0a_0a0a_0a0a_0a0a;
        // Classic has-zero-byte; the set bit marks a `\n`.
        let z = x.wrapping_sub(0x0101_0101_0101_0101) & !x & 0x8080_8080_8080_8080;
        if z != 0 {
            return Some(i + (z.trailing_zeros() >> 3) as usize);
        }
        i += 8;
    }
    b[i..].iter().position(|&c| c == b'\n').map(|p| p + i)
}

/// Index of the next space at or after `from`, or `None`.
#[inline]
fn next_space(buf: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < buf.len() {
        if buf[i] == b' ' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Checked decimal parse. The spec guarantees the grammar, so a malformed field
/// cannot occur; returning 0 rather than panicking keeps a surprise from
/// becoming a non-zero exit.
#[inline]
fn parse_u32(buf: &[u8]) -> u32 {
    let mut v: u32 = 0;
    for &b in buf {
        if !b.is_ascii_digit() {
            break;
        }
        v = v.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    v
}

/// Fold one line into a worker map, reusing `key` so the hot loop allocates
/// nothing on a hit.
#[inline]
fn take(line: &[u8], key: &mut Vec<u8>, map: &mut Map) {
    if line.len() < 32 {
        return;
    }
    // Field boundaries: SP after the method, then SP after the endpoint.
    let s1 = match next_space(line, 28) {
        Some(i) => i,
        None => return,
    };
    let s2 = match next_space(line, s1 + 1) {
        Some(i) => i,
        None => return,
    };
    if line.len() < s2 + 5 {
        return;
    }

    let st = &line[s2 + 1..s2 + 4];
    let status =
        (st[0] - b'0') as u32 * 100 + (st[1] - b'0') as u32 * 10 + (st[2] - b'0') as u32;
    let latency = parse_u32(&line[s2 + 5..]);
    let err = (500..=599).contains(&status) as u32;

    key.clear();
    key.extend_from_slice(&line[0..13]);
    key.extend_from_slice(&line[s1 + 1..s2]);

    match map.get_mut(&key[..]) {
        Some(g) => {
            g.latencies.push(latency);
            g.errors += err;
        }
        None => {
            let mut g = Group {
                latencies: Vec::with_capacity(8),
                ..Default::default()
            };
            g.latencies.push(latency);
            g.errors = err;
            map.insert(key.as_slice().into(), g);
        }
    }
}

/// Group the half-open byte slice `[start, end)` of `path` and return its rows
/// already key-sorted.
///
/// Split by byte, so the two ends of the slice need opposite treatment. The
/// worker at `start > 0` does not own the line that straddles `start`: it drops
/// everything up to and including the first `\n`. At the far end it owns every
/// line whose FIRST byte is `<= end`, i.e. it finishes the line that straddles
/// `end` and stops only before a line that begins after it. The next worker then
/// drops that straddling line, so the two rules meet exactly: dropping on the
/// left and `<=` on the right is a partition — no line counted twice, none lost,
/// including the case where a line begins exactly on a cut.
fn scan(path: &str, start: u64, end: u64) -> Vec<Row> {
    let mut map = Map::default();
    if start >= end {
        return Vec::new();
    }
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[track-a] cannot open {path}: {e}");
            exit(1);
        }
    };
    if let Err(e) = file.seek(SeekFrom::Start(start)) {
        eprintln!("[track-a] seek failed: {e}");
        exit(1);
    }

    let mut buf = vec![0u8; CHUNK];
    let mut have = 0usize; // valid bytes at the front of buf
    let mut pos = start; // absolute offset of buf[0]
    let mut skip = start > 0; // still inside the previous worker's last line
    let mut key = Vec::with_capacity(64);

    'outer: loop {
        if have == buf.len() {
            buf.resize(buf.len() * 2, 0); // pathological line, never in practice
        }
        let n = match file.read(&mut buf[have..]) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                eprintln!("[track-a] read error: {e}");
                exit(1);
            }
        };
        have += n;

        let mut cur = 0usize;
        while let Some(i) = nl(&buf[cur..have]) {
            let from = cur;
            let at = pos + cur as u64;
            cur += i + 1;
            if skip {
                skip = false; // tail of a line another worker owns
                continue;
            }
            if at > end {
                break 'outer;
            }
            take(&buf[from..from + i], &mut key, &mut map);
        }
        // Carry the unterminated tail to the front of the buffer; it is a
        // handful of bytes, and buf[0] stays aligned with a line start.
        buf.copy_within(cur..have, 0);
        pos += cur as u64;
        have -= cur;
    }

    let mut rows: Vec<Row> = map.into_iter().collect();
    rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// Two-pointer merge of two key-sorted row lists. A group split across workers
/// appears in both lists with the same key, so equal keys concatenate their
/// latencies into one group.
fn merge(a: Vec<Row>, b: Vec<Row>) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::with_capacity(a.len() + b.len());
    let (mut ai, mut bi) = (a.into_iter().peekable(), b.into_iter().peekable());
    loop {
        let take_a = match (ai.peek(), bi.peek()) {
            (Some(x), Some(y)) => x.0 <= y.0,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let mut row = if take_a {
            ai.next().unwrap()
        } else {
            bi.next().unwrap()
        };
        let fold = |front: &mut Peekable<IntoIter<Row>>, row: &mut Row| {
            while matches!(front.peek(), Some(x) if x.0 == row.0) {
                let (_, g) = front.next().unwrap();
                row.1.latencies.extend_from_slice(&g.latencies);
                row.1.errors += g.errors;
            }
        };
        fold(&mut ai, &mut row);
        fold(&mut bi, &mut row);
        out.push(row);
    }
    out
}

/// Fill p50/p95/p99 by nearest rank: `ceil(p*n/100) == (p*n + 99)/100`. Each
/// selection runs inside the prefix that still contains the wanted rank (after
/// selecting rank r the first r+1 slots hold the r+1 smallest values), so the
/// cheaper ranks do not re-partition the whole group.
fn score(g: &mut Group) {
    let n = g.latencies.len() as u64;
    if n == 0 {
        return;
    }
    let rank0 = |p: u64| -> usize { ((p * n + 99) / 100).max(1) as usize - 1 };
    let (t50, t95, t99) = (rank0(50), rank0(95), rank0(99));
    g.p99 = *g.latencies.select_nth_unstable(t99).1;
    g.p95 = *g.latencies[..t99 + 1].select_nth_unstable(t95).1;
    g.p50 = *g.latencies[..t95 + 1].select_nth_unstable(t50).1;
}

fn main() {
    let data = env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());
    let input = format!("{data}/requests.log");
    let output = format!("{outd}/summary.txt");

    let size = match File::open(&input) {
        Ok(f) => f.metadata().map(|m| m.len()).unwrap_or(0),
        Err(e) => {
            eprintln!("[track-a] cannot open {input}: {e}");
            exit(1);
        }
    };

    // ── twelve slices, one sorted row list each ─────────────────────────────
    let mut parts: Vec<Vec<Row>> = thread::scope(|s| {
        let path = input.as_str();
        let mut handles = Vec::with_capacity(NTHREADS);
        for i in 0..NTHREADS {
            let (n, k) = (size, NTHREADS as u64);
            let (a, b) = (n * i as u64 / k, n * (i as u64 + 1) / k);
            handles.push(s.spawn(move || scan(path, a, b)));
        }
        handles
            .into_iter()
            .map(|h| match h.join() {
                Ok(rows) => rows,
                Err(_) => {
                    eprintln!("[track-a] worker failed");
                    exit(1);
                }
            })
            .collect()
    });

    // ── merge the sorted lists pairwise: n·log2(12) compares, not a sort ────
    while parts.len() > 1 {
        let mut next = Vec::with_capacity(parts.len() / 2 + 1);
        let mut it = parts.into_iter();
        while let Some(a) = it.next() {
            match it.next() {
                Some(b) => next.push(merge(a, b)),
                None => next.push(a),
            }
        }
        parts = next;
    }
    let mut rows = parts.pop().unwrap_or_default();

    // ── percentiles, again over twelve threads ──────────────────────────────
    let per = rows.len().div_ceil(NTHREADS).max(1);
    thread::scope(|s| {
        for chunk in rows.chunks_mut(per) {
            s.spawn(move || {
                for (_, g) in chunk.iter_mut() {
                    score(g);
                }
            });
        }
    });

    // ── write exactly one file, in key order ────────────────────────────────
    let out_file = match File::create(&output) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[track-a] cannot create {output}: {e}");
            exit(1);
        }
    };
    let mut w = BufWriter::with_capacity(1 << 20, out_file);
    for (k, g) in &rows {
        let (n, errors) = (g.latencies.len(), g.errors);
        let (p50, p95, p99) = (g.p50, g.p95, g.p99);
        let ppm = errors as u64 * 1_000_000 / n as u64;
        let r = w
            .write_all(&k[..13])
            .and_then(|_| w.write_all(b":00:00Z "))
            .and_then(|_| w.write_all(&k[13..]))
            .and_then(|_| write!(w, " {n} {p50} {p95} {p99} {errors} {ppm}\n"));
        if let Err(e) = r {
            eprintln!("[track-a] write error: {e}");
            exit(1);
        }
    }
    if let Err(e) = w.flush() {
        eprintln!("[track-a] flush error: {e}");
        exit(1);
    }
}
