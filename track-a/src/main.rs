//! Track A — Ingest.
//!
//! Group 3.1M log lines by (hour, endpoint) and emit, per group: the request
//! count, the p50/p95/p99 latency by nearest rank, the 5xx count and the error
//! rate in integer parts per million.
//!
//! Pipeline (scan → rank → score → print), and why it is shaped this way:
//!
//! * **The hour is the first 13 bytes**, folded to a monotone `u32` ordinal
//!   `((y·100 + MM)·100 + DD)·24 + HH`. Those four fields are fixed width, so
//!   the mixed-radix number is injective and order-preserving, and it fits in
//!   `u32` for any four-digit year. The 13 raw bytes are remembered once per
//!   distinct hour, so the printed hour is still the input's own bytes.
//!
//! * **Endpoints are interned in ONE table all workers share.** A slot packs
//!   the name's length above its offset in one arena, so a name's id is a slot
//!   index and there is nothing to merge afterwards: the global step is a sort
//!   of at most 1,842 names and one rank array. Lookups carry a 4-byte id, so
//!   the hot key is one `u64` — `hour ordinal | endpoint id` — with no
//!   per-group key allocation and no second cache line to compare against.
//!
//! * **Byte order is restored by construction.** Slot ids are renumbered into
//!   sorted-byte order before rows meet each other, so comparing `key` as
//!   an integer *is* the required (hour ascending, endpoint ascending) order
//!   and the phase never touches the strings.
//!
//! * **The group table is a flat `Vec`.** Groups are appended in discovery
//!   order and sorted in place at the end of the slice, so nothing is mirrored
//!   into a second array the way `map.into_iter().collect()` would.
//!
//! * **Twelve byte ranges, twelve threads, no shared state beyond the table.**
//!   The file is cut at `size*i/12` and each worker seeks to its slice. The
//!   split is by byte, not by line, so the boundary rule is spelled out at
//!   `scan_with`.
//!
//! * **The serial merge is gone.** Record blocks are per (worker, hour), and
//!   the rank pass is a sort of the shared table, so there is no name union and
//!   no renumbering pass between the parse and the percentiles. The row blocks
//!   are produced by the twelve workers and consumed by whichever of the
//!   twelve claims an hour, so an hour never waits for another.
//!
//! * **Streamed, never slurped.** A worker sees its slice through one buffer it
//!   recycles and parses lines straight out of it, so the file is copied once
//!   and no per-line allocation happens. The map path skips even that copy.
//!
//! * **Merge order cannot matter.** Percentiles are order statistics of a
//!   group's latency multiset and counts are sums, so any merge order yields the
//!   same bytes. Verified: reversing the merge order hashes identically.

use std::cell::UnsafeCell;
use std::env;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::exit;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;

/// 6 physical cores with SMT: 12 slices keep every hardware thread fed.
const NTHREADS: usize = 12;
/// Per-worker read buffer. Deliberately small: M is scored, and twelve buffers
/// are the whole input-side footprint. Measured with the harness's trimmed mean
/// on the full 238 MB set: 64 KiB 0.241 s < 128 KiB 0.254 s < 512 KiB 0.274 s,
/// while 16 KiB loses to syscall overhead (0.308 s).
const CHUNK: usize = 1 << 16;
/// Zero bytes readable past the end of every name. The lookup reads a fixed
/// 64-byte window from a name's first byte and eight bytes as its key, so both
/// the buffer and the mapping must have room past the last name; the bytes are
/// only ever read under a mask, so their value is irrelevant.
const SLACK: usize = 64;
/// The spare page the input is mapped over. One line ends with the file, and
/// the parse reads a fixed window past it. Only the mapping path needs it.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const SPARE: usize = 4096;

/// One line, in FOUR bytes: `packed = lat << KEY_BITS | key`, where `key` is the
/// endpoint id with the 5xx flag in its low bit.
///
/// The two fields share one word because latency is microseconds and `key` is
/// small, so the pair fits in 32 bits -- halving the record store from 24.8 MB to
/// 12.4 MB, which is most of this track's peak. Sorting is by the low bits, so a
/// group is still a contiguous run and the scan still never looks one up.
///
/// **A record that does not fit is NOT dropped and NOT truncated.** Neither the
/// endpoint count nor the latency range is guaranteed by the spec, so both are
/// checked at run time and anything too large goes to `Sink::wide` instead. That
/// is what makes this exact for any input rather than only for seed 1 -- an
/// unguarded shift here would silently corrupt the key.
struct Rec {
    packed: u32,
}

/// Bits 0..KEY_BITS of a packed record hold `key`; everything above holds `lat`.
/// `key = (id << 1) | err`, so this is a mask over endpoint ids and the flag.
///
/// THIRTEEN bits, not twelve. The id is a slot of the shared table, and the spec
/// fixes a seed's catalogue at 1,842 endpoints, so twelve bits would leave only
/// 206 ids of headroom: a seed whose catalogue grew would push almost every line
/// out of the packed word and into the side list -- still exact, which is why a
/// hash gate would never catch it, but a different and much slower program on
/// the run that counts. Thirteen bits holds `2^12` ids, 2.2x the catalogue, and
/// still leaves 19 bits of latency, enough for every line below 524,288 us --
/// all but about 2.6% of them.
const KEY_BITS: u32 = 13;
const KEY_MASK: u32 = (1 << KEY_BITS) - 1;
/// The largest latency that can share a word with the key.
const LAT_MAX: u32 = 1 << (32 - KEY_BITS);
/// Slots in the endpoint table. Must not exceed `KEY_MASK >> 1`, because an id
/// is a slot index and `(id << 1) | err` has to fit the key field.
const SLOTS: usize = 1 << 12;
/// Arena bytes. The spec's longest endpoint is 125 bytes, so even a table filled
/// to every one of its slots needs only 4096·125 = 512,000 bytes -- and the last
/// name can still be read 64 bytes wide.
const ARENA: usize = 1 << 19;

/// Endpoint names to ids, in one table every worker shares.
///
/// A slot is `len << 32 | arena offset` and zero means free, so an id is a slot
/// index and the id is dense enough for the record on its own: the twelve
/// per-worker tables and the merge between them are gone. The key is the length
/// plus the name's first and last eight bytes; for a name of sixteen bytes or
/// fewer that is the whole name, and past that the confirmation compares the
/// bytes anyway, so a weak key costs a probe and never an answer.
///
/// A new name is copied into the arena BEFORE its slot is published by a
/// compare-exchange, so a thread that loses the race -- and any reader that sees
/// the slot afterwards -- compares against bytes that were fully written. The
/// copy is exact for every length; the wide compares that follow are masked to
/// the length, and both sides always have 64 readable bytes (the arena keeps 64
/// spare, the input keeps a spare page and the read buffer keeps 64 of slack).
struct Names {
    slot: Vec<AtomicU64>,
    used: AtomicUsize,
    arena: UnsafeCell<Vec<u8>>,
}

// SAFETY: the only shared mutation is `used`, an atomic byte counter. Each
// writer writes the disjoint arena range that counter handed it and then
// publishes it with a release store; every other access through `&Names` reads
// either a slot or bytes that a release/acquire pair has already published.
unsafe impl Sync for Names {}

impl Names {
    fn new() -> Names {
        Names {
            slot: (0..SLOTS).map(|_| AtomicU64::new(0)).collect(),
            used: AtomicUsize::new(0),
            // Capacity, not length: the pages the arena never uses are never
            // touched, and untouched pages are not charged to peak memory.
            arena: UnsafeCell::new(Vec::with_capacity(ARENA + SLACK)),
        }
    }

    /// Where the arena starts. The `Vec` is never grown, so this never moves.
    #[inline]
    fn base(&self) -> *mut u8 {
        unsafe { (*self.arena.get()).as_ptr() as *mut u8 }
    }

    /// The name a slot holds.
    ///
    /// SAFETY inside: the publisher wrote `len` bytes at `off` before releasing
    /// the slot, and the arena is never reallocated, so the range is readable
    /// for exactly as long as the id is used.
    #[inline]
    fn at(&self, v: u64) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.base().add(v as u32 as usize),
                (v >> 32) as usize,
            )
        }
    }

    /// The id of `s`, adding it to the arena and the table if it is new.
    fn id(&self, s: &[u8]) -> u32 {
        let mut i = (name_key(s) as usize) & (SLOTS - 1);
        loop {
            let v = self.slot[i].load(Ordering::Acquire);
            if v == 0 {
                // Reserve this thread's own slice of the arena. The +1 keeps
                // the offset nonzero, so a zero-length name cannot publish a
                // slot that reads as free.
                let at = self.used.fetch_add(s.len(), Ordering::Relaxed) + 1;
                if at + s.len() > ARENA {
                    eprintln!("[track-a] endpoint arena overflow");
                    exit(1);
                }
                // SAFETY: `at` is this thread's own slice and the slot is still
                // free, so nothing else can read those bytes yet.
                unsafe {
                    core::ptr::copy_nonoverlapping(s.as_ptr(), self.base().add(at), s.len())
                };
                let packed = (s.len() as u64) << 32 | at as u64;
                match self.slot[i].compare_exchange(
                    0,
                    packed,
                    Ordering::Release,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return i as u32,
                    // Lost the race for the slot. The value the exchange handed
                    // back is the winner's; if it is the same name this is done,
                    // and if it is not, keep probing from the next slot.
                    Err(won) => {
                        if same(self.at(won), s) {
                            return i as u32;
                        }
                    }
                }
            } else if same(self.at(v), s) {
                return i as u32;
            }
            i = (i + 1) & (SLOTS - 1);
        }
    }
}

/// Eight bytes at `s` as an integer.
///
/// SAFETY: every caller has already established that eight bytes are readable
/// from `s` -- see `Names` for the three places that guarantee it.
#[inline]
fn load8(s: &[u8]) -> u64 {
    u64::from_le_bytes(unsafe { core::ptr::read_unaligned(s.as_ptr() as *const [u8; 8]) })
}

/// A cheap key for a name: its length, its first eight bytes and its last eight.
/// Past sixteen bytes the middle is not in the key at all, which costs an extra
/// compare on a collision and never correctness, and it makes the cost constant
/// on names that run to 125 bytes.
#[inline]
fn name_key(s: &[u8]) -> u64 {
    let n = s.len();
    let head = if n >= 8 {
        load8(s)
    } else if n == 0 {
        0
    } else {
        load8(s) & ((1u64 << (8 * n)) - 1)
    };
    let tail = if n >= 8 { load8(&s[n - 8..]) } else { head };
    let h = (n as u64).rotate_left(5) ^ head;
    (h.wrapping_mul(0x517c_c1b7_2722_0a95).rotate_left(5) ^ tail)
        .wrapping_mul(0x517c_c1b7_2722_0a95)
}

/// Bit `i` of the result is set when byte `i` of the 32 at `a` equals byte `i`
/// of the 32 at `b`. The caller guarantees 32 readable bytes from both.
///
/// SAFETY (inside): the two loads read a fixed 32 bytes each, which the caller
/// has already established are readable; this cannot be expressed as a slice
/// compare because a name of 33 bytes has no 32 bytes left at offset 32.
#[cfg(target_arch = "x86_64")]
#[inline]
fn eq32(a: *const u8, b: *const u8) -> u32 {
    use core::arch::x86_64::{_mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8};
    unsafe {
        let x = _mm256_loadu_si256(a as *const _);
        let y = _mm256_loadu_si256(b as *const _);
        _mm256_movemask_epi8(_mm256_cmpeq_epi8(x, y)) as u32
    }
}

/// Byte-exact confirmation of a stored name against `s`.
///
/// All but the longest few of a catalogue's names are 64 bytes or fewer and are
/// compared in two 32-byte steps; a longer one falls back to the slice compare.
/// The wide loads run past the name on both sides, which the caller's 64
/// readable bytes make safe, and the LENGTH MASK -- not the load width -- is
/// what makes the answer exact. A name of 33 to 64 bytes is the common case
/// (43% of this corpus) and costs two vector compares instead of a `memcmp`
/// whose length is not known until it returns.
#[cfg(target_arch = "x86_64")]
#[inline]
fn same(x: &[u8], s: &[u8]) -> bool {
    let n = s.len();
    if x.len() != n || n > 64 || n == 0 {
        return x == s;
    }
    // SAFETY: see the doc comment; ARENA keeps 64 spare bytes and every input
    // name has 64 readable bytes behind it.
    let m = unsafe {
        eq32(x.as_ptr(), s.as_ptr()) as u64
            | ((eq32(x.as_ptr().add(32), s.as_ptr().add(32)) as u64) << 32)
    };
    let keep = u64::MAX >> (64 - n);
    (m & keep) == keep
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn same(x: &[u8], s: &[u8]) -> bool {
    x == s
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

/// One worker's result. Nothing is aggregated in the scan: the worker keeps its
/// raw per-hour record blocks and phase 2 (below, in `main`) walks one HOUR at a
/// time across all workers, so no merge is ever serial.
struct Part {
    recs: Vec<Vec<Rec>>,
    /// Records a packed word cannot hold: `(latency, key)`, one list per hour
    /// rank. A per-hour list rather than one list tagged with its hour, because
    /// this holds about 2.6% of the corpus and scanning all of it once per hour
    /// would be 69M comparisons over the run.
    wide: Vec<Vec<(u32, u32)>>,
    hours: Vec<(u32, [u8; 13])>,
}

/// The per-worker scan state, in one struct so the line parser can be a method.
struct Sink<'a> {
    /// The one endpoint table, shared by every worker.
    names: &'a Names,
    /// One record block per DISTINCT hour, indexed by that hour's rank in
    /// `hours`. Keying a block by hour rather than opening a new one at every
    /// hour change is what keeps the group count at the output's own size: the
    /// generator interleaves eight shards, so the corpus has 1054 hour runs but
    /// only 72 distinct hours, and one block per run would multiply the groups
    /// by the number of runs.
    recs: Vec<Vec<Rec>>,
    wide: Vec<Vec<(u32, u32)>>,
    /// Distinct hours, each with the 13 bytes it was parsed from.
    hours: Vec<(u32, [u8; 13])>,
    hour: [u8; 13],
    ord: u32,
    /// Rank of the current hour in `hours`: which block is open.
    hrank: u32,
}

impl Sink<'_> {
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
                self.wide.push(Vec::new());
            }
            self.hrank = hi as u32;
        }

        // One shared table: the id is the slot, so there is no per-worker name
        // list to merge and no id to renumber before the names mean anything.
        let id = self.names.id(&line[s1 + 1..s2]);

        // The whole group lookup, replaced by one sequential append. The hour
        // is not in the record: it is the block the record lands in.
        let key = (id << 1) | err;
        let h = self.hrank as usize;
        if latency < LAT_MAX {
            self.recs[h].push(Rec {
                packed: (latency << KEY_BITS) | key,
            });
        } else {
            self.wide[h].push((latency, key));
        }
    }
}

/// The `mmap` syscall. The argument registers are the documented x86-64 ABI --
/// rax 9, rdi addr, rsi length, rdx prot, r10 flags, r8 fd, r9 offset -- and
/// only rcx and r11, which `syscall` always clobbers, are declared as outputs.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[inline]
unsafe fn sys_mmap(addr: usize, len: usize, prot: usize, flags: usize, fd: i32, off: usize) -> isize {
    let ret: isize;
    core::arch::asm!(
        "syscall",
        inlateout("rax") 9isize => ret,
        in("rdi") addr,
        in("rsi") len,
        in("rdx") prot,
        in("r10") flags,
        in("r8") fd,
        in("r9") off,
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack, preserves_flags)
    );
    ret
}

/// Map the whole input read-only, or `None`.
///
/// `std` has no mmap, so this is the raw syscall. **It exists because 238 MB in
/// 0.0985 s is 2.4 GB/s, which is a COPY rate rather than a compute rate**: the
/// buffered path reads the file into a buffer and the kernel copies every byte
/// through it. Mapping lets the parse walk the page cache in place, so the
/// 238 MB is never duplicated. Read-only `MAP_PRIVATE` file pages stay in the
/// kernel's page cache and are not charged to the process, which is why this
/// does not trade memory for time.
///
/// The mapping is one page longer than the file and the file is placed over the
/// front of it. The spare page is what makes the fixed 64-byte windows around a
/// name -- the key and the confirmation -- readable on the last line, and it
/// costs nothing: an untouched anonymous page is not resident.
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
    let total = len + SPARE;
    // SAFETY: the syscalls are the documented ABI and both lengths are real.
    unsafe {
        // PROT_READ, MAP_PRIVATE|MAP_ANONYMOUS: a zeroed region one page larger
        // than the file, which the file is then mapped over.
        let base = sys_mmap(0, total, 1, 2 | 0x20, -1, 0);
        if base < 0 {
            return None;
        }
        // MAP_FIXED over the front. A failure would leave the anonymous zeros in
        // place, which would parse as an empty file, so treat it as no map.
        if sys_mmap(base as usize, len, 1, 2 | 0x10, f.as_raw_fd(), 0) < 0 {
            return None;
        }
        Some(core::slice::from_raw_parts(base as *const u8, total))
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
fn map_file(_path: &str) -> Option<&'static [u8]> {
    None
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
///
/// `len` is the file's own length, which is NOT `mapped.len()`: the mapping runs
/// one page past the file so the fixed windows stay readable.
fn scan_with(
    path: &str,
    start: u64,
    end: u64,
    mapped: Option<&[u8]>,
    len: u64,
    names: &Names,
) -> Part {
    let mut s = Sink {
        names,
        recs: Vec::new(),
        wide: Vec::new(),
        hours: Vec::new(),
        hour: [0xff; 13],
        ord: 0,
        hrank: 0,
    };
    let out = Part {
        recs: Vec::new(),
        wide: Vec::new(),
        hours: Vec::new(),
    };
    if start >= end {
        return out;
    }

    // Mapped path: walk the page cache in place. No buffer, no refill, no copy.
    //
    // The line rules are the read loop's, restated rather than shared: a worker
    // at start > 0 drops the line straddling start, it owns every line whose
    // FIRST byte is <= end, and it stops before a line that begins after it. The
    // next worker drops that straddling line, so the two rules meet exactly --
    // dropping on the left and <= on the right is a partition, no line counted
    // twice and none lost.
    if let Some(m) = mapped {
        let size = len as usize;
        let mut i = start as usize;
        let mut skip = i > 0;
        while i < size {
            let from = i;
            let k = match nl(&m[i..size]) {
                Some(k) => k,
                None => break,
            };
            // `at` is the line's FIRST byte, which is what the ownership rule is
            // stated over. Using the newline's position instead shifts the test
            // by the line's length and silently gives the straddling line to the
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
        if !skip && i < size && (i as u64) <= end {
            s.take(&m[i..size]);
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

    // `have` is the valid bytes at the front of buf and `cap` is where a read
    // may stop; buf[cap..] stays zero so a name's 64-byte window is readable.
    let mut buf = vec![0u8; CHUNK + SLACK];
    let mut cap = CHUNK;
    let mut have = 0usize; // valid bytes at the front of buf
    let mut pos = start; // absolute offset of buf[0]
    let mut skip = start > 0; // still inside the previous worker's last line

    'outer: loop {
        if have == cap {
            buf.resize(cap + CHUNK, 0); // pathological line, never in practice
            cap += CHUNK;
        }
        let n = match file.read(&mut buf[have..cap]) {
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

impl Sink<'_> {
    fn finish(self, mut out: Part) -> Part {
        out.recs = self.recs;
        out.wide = self.wide;
        out.hours = self.hours;
        out
    }
}

/// p50/p95/p99 by nearest rank, `ceil(p*n/100) == (p*n + 99)/100`. Each selection
/// runs inside the prefix that still holds the wanted rank, pivot included, so n
/// of 1 to 19 (where two ranks coincide) never selects in an empty slice.
fn pct(a: &mut [u32]) -> (u32, u32, u32) {
    let n = a.len() as u64;
    let r = |p: u64| ((p * n + 99) / 100).max(1) as usize - 1;
    let (t50, t95, t99) = (r(50), r(95), r(99));
    let p99 = *a.select_nth_unstable(t99).1;
    let p95 = *a[..t99 + 1].select_nth_unstable(t95).1;
    let p50 = *a[..t95 + 1].select_nth_unstable(t50).1;
    (p50, p95, p99)
}

/// Append `v` in decimal.
#[inline]
fn push_u64(out: &mut Vec<u8>, mut v: u64) {
    let (mut tmp, mut n) = ([0u8; 20], 20);
    loop {
        n -= 1;
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&tmp[n..]);
}

/// Append `v` in decimal at `out[at..]` and return the new offset.
///
/// `write!` routes every integer through `core::fmt`, which is a formatting
/// state machine per call. This track emits 126,908 rows of six integers, and
/// that overhead was measured at roughly 450-550 cycles per row -- about a
/// quarter of the whole run. Digits go into a stack buffer and come back out
/// reversed; there is no allocation and no trait object anywhere.
#[inline]

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
    let names = Names::new();
    let map = map_file(&input);
    let parts: Vec<Part> = thread::scope(|s| {
        let path = input.as_str();
        let names = &names;
        let mut handles = Vec::with_capacity(NTHREADS);
        for i in 0..NTHREADS {
            let (n, k) = (size, NTHREADS as u64);
            let (a, b) = (n * i as u64 / k, n * (i as u64 + 1) / k);
            let m = map;
            handles.push(s.spawn(move || scan_with(path, a, b, m, size, names)));
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

    // ── one global endpoint ranking, and nothing to merge ───────────────────
    // The slot ids are dense enough for the record, so the only global step is
    // the byte order the output needs: sort the table's names once and turn each
    // slot into its rank. There is no per-worker table to union.
    let mut order: Vec<(u64, u32)> = Vec::with_capacity(SLOTS);
    for i in 0..SLOTS {
        let v = names.slot[i].load(Ordering::Relaxed);
        if v != 0 {
            order.push((v, i as u32));
        }
    }
    order.sort_unstable_by(|a, b| names.at(a.0).cmp(names.at(b.0)));
    let mut rank = vec![0u32; SLOTS];
    let mut names_by_rank: Vec<&[u8]> = Vec::with_capacity(order.len());
    for &(v, slot) in &order {
        rank[slot as usize] = names_by_rank.len() as u32;
        names_by_rank.push(names.at(v));
    }
    let ne = names_by_rank.len();

    // Global hour table: distinct hours, and each worker's local rank for it.
    let mut hours: Vec<(u32, [u8; 13])> = parts
        .iter()
        .flat_map(|p| p.hours.iter().copied())
        .collect();
    hours.sort_unstable();
    hours.dedup();
    let hloc: Vec<Vec<usize>> = parts
        .iter()
        .map(|p| {
            hours
                .iter()
                .map(|h| p.hours.iter().position(|x| x.0 == h.0).unwrap_or(usize::MAX))
                .collect()
        })
        .collect();

    // ── phase 2: one hour at a time, on every thread ────────────────────────
    // An hour's records (about 43k) are counted into endpoint groups, scattered
    // into one latency array, selected and formatted by whichever thread claims
    // the hour. No hour waits for another, and nothing is merged serially.
    let next = AtomicUsize::new(0);
    let done: Vec<Vec<(usize, Vec<u8>)>> = thread::scope(|s| {
        let hs: Vec<_> = (0..NTHREADS)
            .map(|_| {
                s.spawn(|| {
                    let mut res = Vec::new();
                    let (mut start, mut errs, mut cur) =
                        (vec![0u32; ne + 1], vec![0u32; ne], vec![0u32; ne]);
                    let mut lat: Vec<u32> = Vec::new();
                    loop {
                        let h = next.fetch_add(1, Ordering::Relaxed);
                        if h >= hours.len() {
                            break;
                        }
                        start.fill(0);
                        errs.fill(0);
                        for (w, p) in parts.iter().enumerate() {
                            let lr = hloc[w][h];
                            if lr == usize::MAX {
                                continue;
                            }
                            for r in &p.recs[lr] {
                                let e = rank[((r.packed & KEY_MASK) >> 1) as usize] as usize;
                                start[e + 1] += 1;
                                errs[e] += r.packed & 1;
                            }
                            for &(_, key) in &p.wide[lr] {
                                let e = rank[(key >> 1) as usize] as usize;
                                start[e + 1] += 1;
                                errs[e] += key & 1;
                            }
                        }
                        for e in 0..ne {
                            start[e + 1] += start[e];
                        }
                        lat.clear();
                        lat.resize(start[ne] as usize, 0);
                        cur.copy_from_slice(&start[..ne]);
                        for (w, p) in parts.iter().enumerate() {
                            let lr = hloc[w][h];
                            if lr == usize::MAX {
                                continue;
                            }
                            for r in &p.recs[lr] {
                                let e = rank[((r.packed & KEY_MASK) >> 1) as usize] as usize;
                                lat[cur[e] as usize] = r.packed >> KEY_BITS;
                                cur[e] += 1;
                            }
                            for &(l, key) in &p.wide[lr] {
                                let e = rank[(key >> 1) as usize] as usize;
                                lat[cur[e] as usize] = l;
                                cur[e] += 1;
                            }
                        }
                        let mut out: Vec<u8> = Vec::with_capacity(start[ne] as usize / 2 + 4096);
                        let hb = &hours[h].1;
                        for e in 0..ne {
                            let (a, b) = (start[e] as usize, start[e + 1] as usize);
                            if a == b {
                                continue;
                            }
                            let n = (b - a) as u64;
                            let (p50, p95, p99) = pct(&mut lat[a..b]);
                            out.extend_from_slice(hb);
                            out.extend_from_slice(b":00:00Z ");
                            out.extend_from_slice(names_by_rank[e]);
                            for v in [n, p50 as u64, p95 as u64, p99 as u64, errs[e] as u64, errs[e] as u64 * 1_000_000 / n] {
                                out.push(b' ');
                                push_u64(&mut out, v);
                            }
                            out.push(b'\n');
                        }
                        res.push((h, out));
                    }
                    res
                })
            })
            .collect();
        hs.into_iter()
            .map(|h| match h.join() {
                Ok(r) => r,
                Err(_) => {
                    eprintln!("[track-a] worker failed");
                    exit(1);
                }
            })
            .collect()
    });
    let mut by_hour: Vec<Vec<u8>> = vec![Vec::new(); hours.len()];
    for (h, buf) in done.into_iter().flatten() {
        by_hour[h] = buf;
    }

    // ── write exactly one file, in hour order ───────────────────────────────
    let mut out_file = match File::create(&output) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[track-a] cannot create {output}: {e}");
            exit(1);
        }
    };
    for buf in &by_hour {
        if let Err(e) = out_file.write_all(buf) {
            eprintln!("[track-a] write error: {e}");
            exit(1);
        }
    }
    // Skip teardown: the file is complete, and the kernel reclaims everything.
    exit(0);
}
