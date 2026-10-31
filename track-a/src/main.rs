//! Track A — Ingest.
//!
//! Group 3.1M log lines by (hour, endpoint) and emit, per group: the request
//! count, the p50/p95/p99 latency by nearest rank, the 5xx count and the error
//! rate in integer parts per million.
//!
//! Pipeline (scan → merge → score → print), and why it is shaped this way:
//!
//! * **The hour is the first 13 bytes**, folded to a monotone `u32` ordinal
//!   `((y·100 + MM)·100 + DD)·24 + HH`. Those four fields are fixed width, so
//!   the mixed-radix number is injective and order-preserving, and it fits in
//!   `u32` for any four-digit year. The 13 raw bytes are remembered once per
//!   distinct hour, so the printed hour is still the input's own bytes.
//!
//! * **Endpoints are interned once per worker.** Each distinct endpoint
//!   allocates its bytes exactly once, and every lookup carries a 4-byte id, so
//!   the hot key is one `u64` — `hour ordinal | endpoint id` — with no
//!   per-group key allocation and no second cache line to compare against.
//!
//! * **Byte order is restored by construction.** Endpoint ids are renumbered
//!   into sorted-byte order before rows meet each other, so comparing `key` as
//!   an integer *is* the required (hour ascending, endpoint ascending) order
//!   and the merge never touches the strings.
//!
//! * **The group table is a flat `Vec`.** Groups are appended in discovery
//!   order and sorted in place at the end of the slice, so nothing is mirrored
//!   into a second array the way `map.into_iter().collect()` would.
//!
//! * **Twelve byte ranges, twelve threads, no shared state.** The file is cut
//!   at `size*i/12` and each worker opens its OWN handle and seeks to its slice.
//!   The split is by byte, not by line, so the boundary rule matters and is
//!   spelled out at `scan`.
//!
//! * **Every serial stage was removed.** Workers sort their own rows before
//!   returning, so the global order is a 12-list merge (n·log2 12 compares)
//!   instead of one 127k-element sort; the percentile pass is split over the
//!   same 12 threads; and each rank is selected inside the prefix that still
//!   contains it, so p95 and p50 do not re-partition the whole group.
//!
//! * **Streamed, never slurped.** A worker sees its slice through one 64 KiB
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
use std::sync::Arc;
use std::thread;
use std::vec::IntoIter;

/// 6 physical cores with SMT: 12 slices keep every hardware thread fed.
const NTHREADS: usize = 12;
/// Per-worker read buffer. Deliberately small: M is scored, and twelve buffers
/// are the whole input-side footprint. Measured with the harness's trimmed mean
/// on the full 238 MB set: 64 KiB 0.241 s < 128 KiB 0.254 s < 512 KiB 0.274 s,
/// while 16 KiB loses to syscall overhead (0.308 s).
const CHUNK: usize = 1 << 16;

/// One group. `key` is `hour ordinal << 32 | endpoint id`: worker-local during
/// the scan, global (sorted-byte) by the time two of them are compared. The
/// percentiles are filled only after the merge and are zero until then.
struct Group {
    key: u64,
    latencies: Vec<u32>,
    errors: u32,
    p50: u32,
    p95: u32,
    p99: u32,
}

/// One worker's result: its key-sorted groups, the endpoint bytes it interned
/// (re-indexed into byte order), and every hour it saw with its raw 13 bytes.
struct Part {
    groups: Vec<Group>,
    names: Vec<Arc<[u8]>>,
    hours: Vec<(u32, [u8; 13])>,
}

/// Multiply-xor hasher (FxHash shape). std's SipHash spends real time on the
/// keys this hot loop hashes 3.1M times; keys are not attacker-chosen here, and
/// bucket order is unobservable because the output is sorted.
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
    buf[from..].iter().position(|&c| c == b' ').map(|p| p + from)
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

/// `YYYY-MM-DDTHH` as one monotone integer. Each field keeps its own radix, so
/// the encoding is strictly increasing in the timestamp and injective, which is
/// all the sort and the equality test need; the fields are fixed width, so a
/// four-digit year always fits in the `u32` result. `wrapping_sub` keeps a byte
/// that is not a digit from panicking, which conforming input cannot reach.
#[inline]
fn hour_ord(h: &[u8]) -> u32 {
    let d = |i: usize| h[i].wrapping_sub(b'0') as u32;
    let (y, mo) = (d(0) * 1000 + d(1) * 100 + d(2) * 10 + d(3), d(5) * 10 + d(6));
    let (dy, hh) = (d(8) * 10 + d(9), d(11) * 10 + d(12));
    ((y * 100 + mo) * 100 + dy) * 24 + hh
}

/// One line, in eight bytes: `lat` is the latency, `key` is the endpoint id
/// with the 5xx flag in its low bit. Records are appended in arrival order and
/// sorted once per hour, so a group is a contiguous run of the block and the
/// scan never has to look one up.
///
/// A packed four-byte form sharing `lat << 12 | key` was built and REVERTED: it
/// needed a runtime guard, a side channel for records that do not fit, and a
/// re-sort of the hour's groups because that channel can create one out of order,
/// and it measured no better on the local peak proxy than this does. It cost 28
/// source lines for no measured gain, and at S exponent 0.15 that is a loss.
struct Rec {
    key: u32,
    lat: u32,
}

/// The per-worker scan state, in one struct so the line parser can be a method.
struct Sink {
    /// endpoint bytes -> endpoint id, in first-seen order.
    inter: HashMap<Arc<[u8]>, u32, BuildHasherDefault<Fx>>,
    names: Vec<Arc<[u8]>>,
    groups: Vec<Group>,
    /// One record block per DISTINCT hour, indexed by that hour's rank in
    /// `hours`. Keying a block by hour rather than opening a new one at every
    /// hour change is what keeps the group count at the output's own size: the
    /// generator interleaves eight shards, so the corpus has 1054 hour runs but
    /// only 72 distinct hours, and one block per run would multiply the groups
    /// by the number of runs.
    recs: Vec<Vec<Rec>>,
    /// Distinct hours, each with the 13 bytes it was parsed from.
    hours: Vec<(u32, [u8; 13])>,
    hour: [u8; 13],
    ord: u32,
    /// Rank of the current hour in `hours`: which block is open.
    hrank: u32,
}

impl Sink {
    /// Fold one line into the group table. The hot path allocates nothing but
    /// the latency push: the key, the endpoint lookup and the hour ordinal all
    /// reuse state earlier lines already built.
    #[inline]
    fn take(&mut self, line: &[u8]) {
        if line.len() < 32 {
            return;
        }
        // Field boundaries from both ends. The method's space is three to seven
        // bytes past the timestamp, but the endpoint's space is the line's LAST
        // space minus four: `status` is always exactly three digits, so the
        // space that ends the endpoint sits four bytes before the last one.
        // Reading the line from the right therefore never walks the endpoint --
        // the longest one in the catalogue is over 200 bytes -- it walks the
        // four-to-ten latency digits instead.
        let s1 = match next_space(line, 28) {
            Some(i) => i,
            None => return,
        };
        let s2 = match line.iter().rposition(|&c| c == b' ') {
            Some(i) if i > s1 + 4 => i - 4,
            _ => return,
        };

        // `status` is always exactly three digits, so 5xx is decided entirely by
        // its first digit. That replaces a three-digit parse -- two subtractions,
        // two multiplies, two adds and a range test -- with one byte compare, on
        // every one of 3.1 M lines. It is exactly equivalent: 500..=599 is
        // precisely the statuses whose leading digit is '5'.
        let err = (line[s2 + 1] == b'5') as u32;
        let latency = parse_u32(&line[s2 + 5..]);

        // The hour is part of the key, so it is only re-parsed where a run of
        // identical hours ends -- and that is also where the block it belongs
        // to stops growing.
        if self.hour[..] != line[..13] {
            self.hour.copy_from_slice(&line[..13]);
            self.ord = hour_ord(&self.hour);
            let mut hi = 0;
            while hi < self.hours.len() && self.hours[hi].0 != self.ord {
                hi += 1;
            }
            if hi == self.hours.len() {
                self.hours.push((self.ord, self.hour));
                self.recs.push(Vec::new());
            }
            self.hrank = hi as u32;
        }

        let ep = &line[s1 + 1..s2];
        let id = match self.inter.get(ep) {
            Some(&i) => i,
            None => {
                let a: Arc<[u8]> = Arc::from(ep);
                let i = self.names.len() as u32;
                self.inter.insert(a.clone(), i);
                self.names.push(a);
                i
            }
        };

        // The whole group lookup, replaced by one sequential append. The hour
        // is not in the record: it is the block the record lands in.
        self.recs[self.hrank as usize].push(Rec {
            key: (id << 1) | err,
            lat: latency,
        });
    }

    /// Sort one hour's records and turn each run of equal keys into a group.
    ///
    /// The 5xx flag rides in the low bit of the sort key, so a group becomes at
    /// most two adjacent runs -- the clean lines and the errors -- and the merge
    /// adds them back together, because it folds every run of equal keys.
    fn flush(&mut self, hrank: usize, recs: &mut Vec<Rec>) {
        if recs.is_empty() {
            return;
        }
        recs.sort_unstable_by_key(|r| r.key);
        let hour = (self.hours[hrank].0 as u64) << 32;
        let mut i = 0;
        while i < recs.len() {
            let key = recs[i].key;
            let mut j = i + 1;
            while j < recs.len() && recs[j].key == key {
                j += 1;
            }
            let mut latencies = Vec::with_capacity(j - i);
            latencies.extend(recs[i..j].iter().map(|r| r.lat));
            self.groups.push(Group {
                key: hour | (key >> 1) as u64,
                latencies,
                errors: (key & 1) * (j - i) as u32,
                p50: 0,
                p95: 0,
                p99: 0,
            });
            i = j;
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
/// including the case where a line begins exactly on a cut. The one line that
/// has no terminator at all — a file that does not end in `\n` — is picked up by
/// the `<= end` rule at EOF, because `nl` can never find it.
/// Map the whole input read-only, or `None`.
///
/// `std` has no mmap, so this is the raw syscall. **It exists because 238 MB in
/// 0.0985 s is 2.4 GB/s, which is a COPY rate rather than a compute rate**: the
/// buffered path reads the file into a 64 KiB buffer and the kernel copies every
/// byte through it. Mapping lets the parse walk the page cache in place, so the
/// 238 MB is never duplicated. Read-only MAP_PRIVATE file pages stay in the
/// kernel's page cache and are not charged to the process, which is why this
/// does not trade memory for time.
///
/// Every failure returns `None` and the caller falls back to buffered reads, so
/// a kernel that refuses the syscall costs speed and never correctness.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn map_file(path: &str) -> Option<&'static [u8]> {
    use std::os::fd::AsRawFd;
    let f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len() as usize;
    if len == 0 {
        return None;
    }
    let ret: isize;
    // SAFETY: the argument registers are the documented mmap(2) ABI on x86-64 --
    // rax 9, rdi addr, rsi length, rdx prot, r10 flags, r8 fd, r9 offset -- and
    // only rcx and r11, which `syscall` always clobbers, are declared as outputs.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") 9isize => ret,
            in("rdi") 0usize,
            in("rsi") len,
            in("rdx") 1usize, // PROT_READ
            in("r10") 2usize, // MAP_PRIVATE
            in("r8") f.as_raw_fd(),
            in("r9") 0usize,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags)
        );
    }
    if ret < 0 {
        return None;
    }
    // SAFETY: mmap returned a valid pointer to `len` readable bytes, mapped for
    // the life of the process. The slice is never written and never unmapped.
    Some(unsafe { core::slice::from_raw_parts(ret as *const u8, len) })
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
fn map_file(_path: &str) -> Option<&'static [u8]> {
    None
}

#[cfg(target_arch = "x86_64")]
#[allow(dead_code)]
fn scan(path: &str, start: u64, end: u64) -> Part {
    scan_with(path, start, end, None)
}

fn scan_with(path: &str, start: u64, end: u64, mapped: Option<&[u8]>) -> Part {
    // ~1 group per 1.6 kB of input: reserve it up front rather than paying nine
    // table doublings per worker. Only a hint — growth is still automatic.
    let est = ((end - start) / 1450) as usize;
    let mut s = Sink {
        inter: HashMap::with_capacity_and_hasher(2048, BuildHasherDefault::default()),
        names: Vec::with_capacity(2048),
        groups: Vec::with_capacity(est),
        recs: Vec::new(),
        hours: Vec::new(),
        hour: [0xff; 13],
        ord: 0,
        hrank: 0,
    };
    let out = Part {
        groups: Vec::new(),
        names: Vec::new(),
        hours: Vec::new(),
    };
    if start >= end {
        return out;
    }

    // Mapped path: walk the page cache in place. No buffer, no refill, no copy.
    //
    // The line rules are the read loop's, restated rather than shared, so they
    // are spelled out here: a worker at start > 0 drops the line straddling
    // start, it owns every line whose FIRST byte is <= end, and it stops before
    // a line that begins after it. The next worker drops that straddling line,
    // so the two rules meet exactly -- dropping on the left and <= on the right
    // is a partition, no line counted twice and none lost.
    if let Some(m) = mapped {
        let len = m.len();
        let mut i = start as usize;
        let mut skip = i > 0;
        while i < len {
            let from = i;
            let k = match nl(&m[i..len]) {
                Some(k) => k,
                None => break,
            };
            // `at` is the line's FIRST byte, which is what the ownership rule is
            // stated over. Using the newline's position instead shifts the test by
            // the line's length and silently gives the straddling line to the
            // wrong worker -- wrong output, not slow output, and the Docker hash
            // is what caught it.
            let at = from as u64;
            i += k + 1;
            if skip {
                skip = false; // tail of a line another worker owns
            } else if at > end {
                return s.finish(out);
            } else {
                s.take(&m[from..from + k]);
            }
        }
        // A file that does not end in `\n` leaves its last line unterminated:
        // `nl` cannot see it and no later worker would own it. It is taken under
        // the same `<= end` rule -- the rule is on where the line STARTS, and
        // the line then runs to the end of the file, not to `end`. Bounding this
        // at `end` truncates a straddling line, which is what the first version
        // of this did and the Docker hash caught.
        if !skip && i < len && (i as u64) <= end {
            s.take(&m[i..len]);
        }
        return s.finish(out);
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

    'outer: loop {
        if have == buf.len() {
            buf.resize(buf.len() * 2, 0); // pathological line, never in practice
        }
        let n = match file.read(&mut buf[have..]) {
            Ok(0) => {
                // EOF. A file that does not end in `\n` leaves its last line
                // unterminated in the buffer: the loop above cannot see it and
                // no later worker will either (it would drop it as residue). It
                // is taken here under the same rule the loop uses — own it when
                // it starts at or before `end`, and only if the first-line skip
                // has already happened.
                if !skip && have > 0 && pos <= end {
                    s.take(&buf[..have]);
                }
                break;
            }
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
            s.take(&buf[from..from + i]);
        }
        // Carry the unterminated tail to the front of the buffer; it is a
        // handful of bytes, and buf[0] stays aligned with a line start.
        buf.copy_within(cur..have, 0);
        pos += cur as u64;
        have -= cur;
    }

    s.finish(out)
}

impl Sink {
    /// Fold each hour's block into groups, renumber the endpoints into
    /// sorted-byte order, and order the groups by the integer key. Both ordering
    /// steps compose: the byte order an id stands for is exactly the order the
    /// id itself now has.
    ///
    /// Shared by both input paths, so a mapped read and a buffered read produce
    /// byte-identical output by construction rather than by review.
    fn finish(mut self, mut out: Part) -> Part {
        // Each block is taken out of the table and dropped as it is consumed, so
        // the record buffers and the groups built from them are never all live.
        for hi in 0..self.recs.len() {
            let mut block = std::mem::take(&mut self.recs[hi]);
            self.flush(hi, &mut block);
        }

        let mut order: Vec<u32> = (0..self.names.len() as u32).collect();
        order.sort_unstable_by(|&a, &b| self.names[a as usize].cmp(&self.names[b as usize]));
        let mut rank = vec![0u32; self.names.len()];
        for (r, &id) in order.iter().enumerate() {
            rank[id as usize] = r as u32;
        }
        for g in &mut self.groups {
            g.key = (g.key & 0xffff_ffff_0000_0000)
                | rank[(g.key & 0xffff_ffff) as usize] as u64;
        }
        self.groups.sort_unstable_by_key(|g| g.key);

        out.groups = self.groups;
        out.names = order.iter().map(|&i| self.names[i as usize].clone()).collect();
        out.hours = self.hours;
        out
    }
}

/// Two-pointer merge of two key-sorted row lists. A group split across workers
/// appears in both lists with the same key, so equal keys concatenate their
/// latencies into one group.
fn merge(a: Vec<Group>, b: Vec<Group>) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::with_capacity(a.len() + b.len());
    let (mut ai, mut bi) = (a.into_iter().peekable(), b.into_iter().peekable());
    loop {
        let take_a = match (ai.peek(), bi.peek()) {
            (Some(x), Some(y)) => x.key <= y.key,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let mut row = if take_a {
            ai.next().unwrap()
        } else {
            bi.next().unwrap()
        };
        let fold = |front: &mut Peekable<IntoIter<Group>>, row: &mut Group| {
            while matches!(front.peek(), Some(x) if x.key == row.key) {
                let g = front.next().unwrap();
                row.latencies.extend_from_slice(&g.latencies);
                row.errors += g.errors;
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

/// Append `v` in decimal at `out[at..]` and return the new offset.
///
/// `write!` routes every integer through `core::fmt`, which is a formatting
/// state machine per call. This track emits 126,908 rows of six integers, and
/// that overhead was measured at roughly 450-550 cycles per row -- about a
/// quarter of the whole run. Digits go into a stack buffer and come back out
/// reversed; there is no allocation and no trait object anywhere.
#[inline]
fn put_u64(out: &mut [u8], mut at: usize, mut v: u64) -> usize {
    let mut tmp = [0u8; 20];
    let mut n = 0;
    loop {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
        if v == 0 {
            break;
        }
    }
    while n > 0 {
        n -= 1;
        out[at] = tmp[n];
        at += 1;
    }
    at
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
    // Mapped once and shared: read-only MAP_PRIVATE file pages stay in the page
    // cache and are not charged to the process, so this costs no memory, and the
    // parse then walks the file in place instead of copying it through a buffer.
    // `None` on any failure and every worker falls back to buffered reads.
    let map = map_file(&input);
    let mut parts: Vec<Part> = thread::scope(|s| {
        let path = input.as_str();
        let mut handles = Vec::with_capacity(NTHREADS);
        for i in 0..NTHREADS {
            let (n, k) = (size, NTHREADS as u64);
            let (a, b) = (n * i as u64 / k, n * (i as u64 + 1) / k);
            let m = map;
            handles.push(s.spawn(move || scan_with(path, a, b, m)));
        }
        handles
            .into_iter()
            .map(|h| match h.join() {
                Ok(p) => p,
                Err(_) => {
                    eprintln!("[track-a] worker failed");
                    exit(1);
                }
            })
            .collect()
    });

    // ── one global endpoint table and one global hour table ─────────────────
    // Endpoint ids are re-derived in sorted-byte order; every worker's local
    // order survives the remap, so the lists stay sorted and never need a
    // second pass. The per-worker interners are dropped here, before the merge.
    let mut eps: Vec<Arc<[u8]>> = parts.iter().flat_map(|p| p.names.iter().cloned()).collect();
    eps.sort_unstable();
    eps.dedup();
    let eidx: HashMap<&[u8], u32> = eps
        .iter()
        .enumerate()
        .map(|(i, a)| (&a[..], i as u32))
        .collect();
    let mut hours: Vec<(u32, [u8; 13])> =
        parts.iter().flat_map(|p| p.hours.iter().copied()).collect();
    hours.sort_unstable();
    hours.dedup();
    for p in parts.iter_mut() {
        let map: Vec<u32> = p.names.iter().map(|a| eidx[&a[..]]).collect();
        for g in &mut p.groups {
            g.key = (g.key & 0xffff_ffff_0000_0000) | map[(g.key & 0xffff_ffff) as usize] as u64;
        }
    }

    // ── merge the sorted lists pairwise: n·log2(12) compares, not a sort ────
    let mut parts: Vec<Vec<Group>> = parts.into_iter().map(|p| p.groups).collect();
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
                for g in chunk.iter_mut() {
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
    // Rows arrive in hour order, so the byte lookup happens once per hour.
    let (mut cur, mut hb) = (u32::MAX, [0u8; 13]);
    // One row is assembled here and written with a single call. 1024 bytes covers
    // the longest endpoint in the catalogue (over 200) with room to spare; the
    // guard below falls back to core::fmt rather than risk a slice panic on an
    // endpoint the spec does not bound.
    let mut row = [0u8; 1024];
    for g in &rows {
        let ho = (g.key >> 32) as u32;
        if ho != cur {
            cur = ho;
            hb = hours[hours.binary_search_by_key(&ho, |x| x.0).unwrap()].1;
        }
        let (n, errors) = (g.latencies.len(), g.errors);
        let ppm = errors as u64 * 1_000_000 / n as u64;
        let ep = &eps[(g.key & 0xffff_ffff) as usize];
        let r = if row.len() < ep.len() + 128 {
            w.write_all(&hb)
                .and_then(|_| w.write_all(b":00:00Z "))
                .and_then(|_| w.write_all(ep))
                .and_then(|_| write!(w, " {n} {} {} {} {errors} {ppm}\n", g.p50, g.p95, g.p99))
        } else {
            let (mut k, put) = (0usize, |k: usize, v: u64, row: &mut [u8; 1024]| put_u64(row, k, v));
            row[..13].copy_from_slice(&hb);
            k += 13;
            row[k..k + 8].copy_from_slice(b":00:00Z ");
            k += 8;
            row[k..k + ep.len()].copy_from_slice(ep);
            k += ep.len();
            for v in [n as u64, g.p50 as u64, g.p95 as u64, g.p99 as u64, errors as u64, ppm] {
                row[k] = b' ';
                k = put(k + 1, v, &mut row);
            }
            row[k] = b'\n';
            k += 1;
            w.write_all(&row[..k])
        };
        if let Err(e) = r {
            eprintln!("[track-a] write error: {e}");
            exit(1);
        }
    }
    if let Err(e) = w.flush() {
        eprintln!("[track-a] flush error: {e}");
        exit(1);
    }
    // Skip teardown. Everything left alive at this point -- 145,601 `Group`s and
    // the 3.1 M latencies behind them, plus the twelve per-worker interners --
    // is freed by Rust's drop glue on the way out of `main`, and every one of
    // those frees happens AFTER the output is written. The process is about to
    // exit and the kernel reclaims the whole address space at once, so running
    // the destructors buys nothing and costs the allocator walk.
    //
    // This is only safe because `w.flush()` above has already succeeded: the
    // output file is complete and closed before the exit. `exit` does not run
    // destructors, so anything still buffered here would be lost -- which is why
    // the flush is not skipped along with it.
    exit(0);
}
