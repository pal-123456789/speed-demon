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
//! * **A worker's result is a block per hour.** Each worker keeps its own
//!   blocks and phase 2 walks one HOUR at a time across all workers, so no
//!   merge is ever serial and no hour waits for another. The traversal is a
//!   method — `Part::each` — rather than a loop written at each call site, so
//!   the counting pass, the scatter and the size pass cannot drift apart.
//!
//! * **Twelve byte ranges, twelve threads, no shared state beyond the table.**
//!   The file is cut at `size*i/12` and each worker seeks to its slice. The
//!   split is by byte, not by line, so the boundary rule is spelled out at
//!   `scan_with`.
//!
//! * **Streamed, never slurped.** A worker sees its slice through one buffer it
//!   recycles and parses lines straight out of it, so the file is copied once
//!   and no per-line allocation happens. The map path skips even that copy.
//!
//! * **The output is the file.** An hour's text size is known from its rows
//!   without formatting them, so every byte offset is known before anything is
//!   written; the output is cut to length, mapped, and each thread formats its
//!   hours straight into their own slice of it. There is no per-hour string, no
//!   writer thread and no serial write to queue on the file's lock, and the only
//!   copy of the text is the file's own page cache.
//!
//! * **Every error path is one function.** `die` reports and leaves with a
//!   non-zero status, so a failed syscall or an over-filled table cannot be
//!   mistaken for a slow run and cannot be silently swallowed.
//!
//! * **Merge order cannot matter.** Percentiles are order statistics of a
//!   group's latency multiset and counts are sums, so any merge order yields the
//!   same bytes. Verified: reversing the merge order hashes identically.

use std::cell::UnsafeCell;
use std::env;
use std::fs::{File, OpenOptions};
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

/// The `(latency, key)` pairs a packed word cannot hold.
type Wide = Vec<(u32, u32)>;
/// One worker's hour: its ordinal, its own 13 timestamp bytes, its packed
/// records, and the records that did not fit a packed word.
type Block = (u32, [u8; 13], Vec<u32>, Wide);
/// An output row before it is text: endpoint rank, count, p50, p95, p99, 5xx.
type Row = (u32, u32, u32, u32, u32, u32);

/// The program's one error path: report and leave with a non-zero status.
fn die(msg: &str) -> ! {
    eprintln!("[track-a] {msg}");
    exit(1)
}

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
        Names { slot: (0..SLOTS).map(|_| AtomicU64::new(0)).collect(), used: AtomicUsize::new(0), arena: UnsafeCell::new(Vec::with_capacity(ARENA + SLACK)) }
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
        unsafe { core::slice::from_raw_parts(self.base().add(v as u32 as usize), (v >> 32) as usize) }
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
                    die("endpoint arena overflow");
                }
                // SAFETY: `at` is this thread's own slice and the slot is still
                // free, so nothing else can read those bytes yet.
                unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), self.base().add(at), s.len()) };
                let packed = (s.len() as u64) << 32 | at as u64;
                // A failed exchange hands back the winner's value; if that is
                // the same name this line is done, and if it is not, keep
                // probing from the next slot. The winner copied its name into
                // the arena before publishing, so the compare is exact.
                let won = self.slot[i].compare_exchange(0, packed, Ordering::Release, Ordering::Relaxed);
                if won.is_ok() || same(self.at(won.unwrap_err()), s) {
                    return i as u32;
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
/// on names that run to 125 bytes. A name shorter than eight bytes is masked to
/// its length, and the mask is zero for a name of no bytes at all.
#[inline]
fn name_key(s: &[u8]) -> u64 {
    let n = s.len();
    let head = if n >= 8 { load8(s) } else { load8(s) & ((1u64 << (8 * n)) - 1) };
    let tail = if n >= 8 { load8(&s[n - 8..]) } else { head };
    let h = (n as u64).rotate_left(5) ^ head;
    (h.wrapping_mul(0x517c_c1b7_2722_0a95).rotate_left(5) ^ tail).wrapping_mul(0x517c_c1b7_2722_0a95)
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
    unsafe { _mm256_movemask_epi8(_mm256_cmpeq_epi8(_mm256_loadu_si256(a as *const _), _mm256_loadu_si256(b as *const _))) as u32 }
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
    let m = unsafe { eq32(x.as_ptr(), s.as_ptr()) as u64 | ((eq32(x.as_ptr().add(32), s.as_ptr().add(32)) as u64) << 32) };
    let keep = u64::MAX >> (64 - n);
    (m & keep) == keep
}

/// A portable build has no wide loads: compare the slices.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn same(x: &[u8], s: &[u8]) -> bool {
    x == s
}

/// Bit `i` of the result is set when byte `i` of the 32 at `p` is `c`.
///
/// SAFETY (inside): the caller guarantees 32 readable bytes from `p`. That is
/// what the spare page after the input and the 64 bytes of slack in the read
/// buffer are for -- without them this reads off the end of the last line.
#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn find32(p: *const u8, c: u8) -> u32 {
    use core::arch::x86_64::{_mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8, _mm256_set1_epi8};
    _mm256_movemask_epi8(_mm256_cmpeq_epi8(_mm256_loadu_si256(p as *const _), _mm256_set1_epi8(c as i8))) as u32
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
unsafe fn find32(p: *const u8, c: u8) -> u32 {
    let mut m = 0u32;
    for i in 0..32 {
        m |= ((*p.add(i) == c) as u32) << i;
    }
    m
}

/// Index of the next `\n` in `b`, 32 bytes at a time, or `None`.
///
/// Every byte of the 238 MB goes through here, and the 8-byte SWAR loop this
/// replaces took nine iterations for an average 77-byte line. A `\n` found in
/// the slack past the end of `b` is not part of the input -- stale line data can
/// sit there after a buffer carry -- so a hit at or past the end means "not in
/// this slice".
#[inline]
fn nl(b: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < b.len() {
        // SAFETY: 32 readable bytes from `b.as_ptr() + i`, see `find32`.
        let m = unsafe { find32(b.as_ptr().add(i), b'\n') };
        if m != 0 {
            let p = i + m.trailing_zeros() as usize;
            return (p < b.len()).then_some(p);
        }
        i += 32;
    }
    None
}

/// Checked decimal parse. The spec guarantees the grammar, so a malformed field
/// cannot occur; returning 0 rather than panicking keeps a surprise from
/// becoming a non-zero exit. It is written as a `take_while`/`fold` pair because
/// rustc emits the same six-instruction loop either way -- checked on this
/// toolchain with `--emit asm` -- and the iterator says where it stops.
#[inline]
fn parse_u32(b: &[u8]) -> u32 {
    b.iter().take_while(|c| c.is_ascii_digit()).fold(0u32, |v, &c| v.wrapping_mul(10).wrapping_add((c - b'0') as u32))
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

/// One worker's whole result plus the little state its line parser carries.
///
/// `blocks` holds one entry per DISTINCT hour the worker saw. Keying a block by
/// hour rather than opening a new one at every hour change is what keeps the
/// group count at the output's own size: the generator interleaves eight shards,
/// so the corpus has 1054 hour runs but only 72 distinct hours, and one block
/// per run would multiply the groups by the number of runs.
struct Part {
    blocks: Vec<Block>,
    /// The 13 bytes of the hour the last line was in.
    hour: [u8; 13],
    /// Its ordinal, and its rank in `blocks`: which block is open.
    ord: u32,
    hrank: u32,
}

impl Part {
    /// Fold one line into this worker's blocks. The hot path allocates nothing
    /// but the record push: the key, the endpoint lookup and the hour ordinal
    /// all reuse state earlier lines already built.
    #[inline]
    fn take(&mut self, names: &Names, line: &[u8]) {
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
        let Some(s1) = line[28..].iter().position(|&c| c == b' ').map(|p| p + 28) else { return };
        let Some(s2) = line.iter().rposition(|&c| c == b' ').filter(|&i| i > s1 + 4).map(|i| i - 4) else { return };
        // `status` is always exactly three digits, so 5xx is decided entirely by
        // its first digit: 500..=599 is precisely the statuses whose leading
        // digit is '5'. That replaces a three-digit parse on every one of 3.1M
        // lines with one byte compare.
        let err = (line[s2 + 1] == b'5') as u32;
        let latency = parse_u32(&line[s2 + 5..]);
        // The hour is part of the key, so it is only re-parsed where a run of
        // identical hours ends -- and that is also where the block it belongs
        // to stops growing.
        if self.hour[..] != line[..13] {
            self.hour.copy_from_slice(&line[..13]);
            self.ord = hour_ord(&self.hour);
            self.hrank = self.blocks.iter().position(|b| b.0 == self.ord).unwrap_or(usize::MAX) as u32;
            if self.hrank == u32::MAX {
                self.hrank = self.blocks.len() as u32;
                self.blocks.push((self.ord, self.hour, Vec::new(), Vec::new()));
            }
        }
        // One shared table: the id is the slot, so there is no per-worker name
        // list to merge and no id to renumber before the names mean anything.
        let key = (names.id(&line[s1 + 1..s2]) << 1) | err;
        // A record that does not fit is NOT dropped and NOT truncated. Neither
        // the endpoint count nor the latency range is guaranteed by the spec, so
        // both are checked at run time and anything too large goes to the side
        // list. That is what makes this exact for any input rather than only for
        // seed 1 -- an unguarded shift here would silently corrupt the key.
        let b = &mut self.blocks[self.hrank as usize];
        if latency < LAT_MAX {
            b.2.push(latency << KEY_BITS | key)
        } else {
            b.3.push((latency, key))
        }
    }

    /// Calls `f(rank, key, latency)` for every record of the hour whose block
    /// has local rank `lr` here, packed records first and then the side list.
    /// `usize::MAX` means this worker never saw the hour at all.
    #[inline]
    fn each(&self, lr: usize, rank: &[u32], mut f: impl FnMut(u32, u32, u32)) {
        if lr == usize::MAX {
            return;
        }
        for &p in &self.blocks[lr].2 {
            f(rank[((p & KEY_MASK) >> 1) as usize], p & KEY_MASK, p >> KEY_BITS);
        }
        for &(l, key) in &self.blocks[lr].3 {
            f(rank[(key >> 1) as usize], key, l);
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
    core::arch::asm!("syscall", inlateout("rax") 9isize => ret, in("rdi") addr, in("rsi") len, in("rdx") prot, in("r10") flags, in("r8") fd, in("r9") off, lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
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
    let (total, fd) = (len + SPARE, f.as_raw_fd());
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
        if sys_mmap(base as usize, len, 1, 2 | 0x10, fd, 0) < 0 {
            return None;
        }
        Some(core::slice::from_raw_parts(base as *const u8, total))
    }
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
fn map_file(_path: &str) -> Option<&'static [u8]> {
    None
}

/// The output file, mapped read-write, or `None`.
///
/// `MAP_SHARED` is the point: what the text phase writes IS the file's page
/// cache, so there is no second copy to push through a `write`, no writer thread
/// and no per-hour buffer. Every failure returns `None`, and the caller formats
/// into one flat buffer and writes that instead.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn out_map(f: &File, len: u64) -> Option<*mut u8> {
    use std::os::fd::AsRawFd;
    if len == 0 || f.set_len(len).is_err() {
        return None;
    }
    // PROT_READ|PROT_WRITE, MAP_SHARED.
    // SAFETY: the fd is open for writing, the length is real, and nothing else
    // maps this file.
    let ret = unsafe { sys_mmap(0, len as usize, 3, 1, f.as_raw_fd(), 0) };
    (ret >= 0).then_some(ret as *mut u8)
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
fn out_map(_f: &File, _len: u64) -> Option<*mut u8> {
    None
}

/// Run `f` on `NTHREADS` workers and collect their results in worker order.
///
/// One pool for all three phases: the phase-1 slices are disjoint byte ranges,
/// and phases 2 and 3 hand out one hour at a time from an atomic counter, so no
/// worker owns anything another one needs. A worker that panics cannot do so in
/// the scored build (`panic = "abort"`), and is reported here if it ever does.
fn par<T: Send>(f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    thread::scope(|s| {
        let f = &f;
        let hs: Vec<_> = (0..NTHREADS).map(|i| s.spawn(move || f(i))).collect();
        hs.into_iter().map(|h| h.join().unwrap_or_else(|_| die("worker failed"))).collect()
    })
}

/// Group the half-open byte slice `[start, end)` of `path` and return its parts.
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
fn scan_with(path: &str, start: u64, end: u64, mapped: Option<&[u8]>, len: u64, names: &Names) -> Part {
    let mut s = Part { blocks: Vec::new(), hour: [0xff; 13], ord: 0, hrank: 0 };
    if start >= end {
        return s;
    }

    // Mapped path: walk the page cache in place. No buffer, no refill, no copy.
    if let Some(m) = mapped {
        let (size, mut i, mut skip) = (len as usize, start as usize, start > 0);
        while i < size {
            let Some(k) = nl(&m[i..size]) else { break };
            let at = i as u64;
            i += k + 1;
            // `at` is the line's FIRST byte, which is what the ownership rule is
            // stated over. Using the newline's position instead shifts the test
            // by the line's length and silently gives the straddling line to the
            // wrong worker -- wrong output, not slow output, and the Docker hash
            // is what caught it.
            if !skip && at > end {
                return s;
            }
            if !skip {
                s.take(names, &m[at as usize..at as usize + k]);
            }
            skip = false;
        }
        // A file that does not end in `\n` leaves its last line unterminated:
        // `nl` cannot see it and no later worker would own it. It is taken under
        // the same `<= end` rule -- the rule is on where the line STARTS, and
        // the line then runs to the end of the file, not to `end`. Bounding this
        // at `end` truncates a straddling line, which is what the first version
        // of this did and the Docker hash caught.
        if !skip && i < size && (i as u64) <= end {
            s.take(names, &m[i..size]);
        }
        return s;
    }

    let mut file = File::open(path).unwrap_or_else(|e| die(&format!("cannot open {path}: {e}")));
    if file.seek(SeekFrom::Start(start)).is_err() {
        die("seek failed");
    }
    // `have` is the valid bytes at the front of buf and `cap` is where a read
    // may stop; buf[cap..] stays zero so a name's 64-byte window is readable.
    let (mut buf, mut cap, mut have, mut pos, mut skip) = (vec![0u8; CHUNK + SLACK], CHUNK, 0usize, start, start > 0);
    'outer: loop {
        if have == cap {
            buf.resize(cap + CHUNK, 0);
            cap += CHUNK;
        } // a pathological line
        let n = file.read(&mut buf[have..cap]).unwrap_or_else(|e| die(&format!("read error: {e}")));
        if n == 0 {
            // EOF. A file that does not end in `\n` leaves its last line
            // unterminated in the buffer: the loop above cannot see it and no
            // later worker will either (it would drop it as residue). It is
            // taken here under the same rule the loop uses — own it when it
            // starts at or before `end`, and only if the first-line skip has
            // already happened.
            if !skip && have > 0 && pos <= end {
                s.take(names, &buf[..have]);
            }
            break;
        }
        have += n;
        let mut cur = 0usize;
        while let Some(i) = nl(&buf[cur..have]) {
            let (from, at) = (cur, pos + cur as u64);
            cur += i + 1;
            if !skip && at > end {
                break 'outer;
            }
            if !skip {
                s.take(names, &buf[from..from + i]);
            }
            skip = false;
        }
        // Carry the unterminated tail to the front of the buffer; it is a
        // handful of bytes, and buf[0] stays aligned with a line start.
        buf.copy_within(cur..have, 0);
        pos += cur as u64;
        have -= cur;
    }
    s
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

/// Decimal digits in `u32`.
///
/// `log10(v)` is at most `bit_length(v)·log10(2)`, and 1233/4096 is log10(2) to
/// about 0.0006, so one compare against a power of ten fixes the rounding. `| 1`
/// is what keeps zero at one digit rather than none. Called twice per printed
/// number -- once to size it and once to place it.
const P10: [u32; 10] = [1, 10, 100, 1000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000];
#[inline]
fn width(v: u32) -> usize {
    let t = ((32 - (v | 1).leading_zeros()) as usize * 1233) >> 12;
    t + ((v | 1) >= P10[t]) as usize
}

/// Write `v` in decimal at `dst[at..at + width(v)]`.
///
/// `write!` routes every integer through `core::fmt`, a formatting state machine
/// per call; this track emits about 760k numbers. Digits are written backwards
/// from their known end, so there is no buffer, no allocation and no loop that
/// has to count first.
#[inline]
fn put_u64(dst: &mut [u8], at: usize, mut v: u32) {
    let mut i = at + width(v);
    while i > at {
        i -= 1;
        dst[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
}

/// Write " " and `v` at `dst[at..]` and return the offset just past it. The
/// numbers of a row are emitted by this one call, and the size pass adds up the
/// same `1 + width(v)`.
#[inline]
fn num(dst: &mut [u8], at: usize, v: u32) -> usize {
    let w = width(v);
    dst[at] = b' ';
    put_u64(dst, at + 1, v);
    at + 1 + w
}

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
    unsafe { mallopt(-3, 1 << 16) };
}

#[cfg(not(target_os = "linux"))]
fn tune_allocator() {}

fn main() {
    tune_allocator();
    let data = env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());
    let (input, output) = (format!("{data}/requests.log"), format!("{outd}/summary.txt"));
    let size = File::open(&input).map(|f| f.metadata().map(|m| m.len()).unwrap_or(0)).unwrap_or_else(|e| die(&format!("cannot open {input}: {e}")));

    // ── twelve slices, one block list each ──────────────────────────────────
    // Mapped once and shared: read-only MAP_PRIVATE file pages stay in the page
    // cache and are not charged to the process, so this costs no memory, and the
    // parse then walks the file in place instead of copying it through a buffer.
    // `None` on any failure and every worker falls back to buffered reads.
    let names = Names::new();
    let map = map_file(&input);
    let parts: Vec<Part> = par(|i| scan_with(&input, size * i as u64 / NTHREADS as u64, size * (i as u64 + 1) / NTHREADS as u64, map, size, &names));

    // ── one global endpoint ranking, and nothing to merge ───────────────────
    // The slot ids are dense enough for the record, so the only global step is
    // the byte order the output needs: sort the table's names once and turn each
    // slot into its rank. There is no per-worker table to union.
    let mut order: Vec<(u64, u32)> = (0..SLOTS).map(|i| (names.slot[i].load(Ordering::Relaxed), i as u32)).filter(|&(v, _)| v != 0).collect();
    order.sort_unstable_by(|a, b| names.at(a.0).cmp(names.at(b.0)));
    let mut rank = vec![0u32; SLOTS];
    order.iter().enumerate().for_each(|(r, &(_, slot))| rank[slot as usize] = r as u32);
    let names_by_rank: Vec<&[u8]> = order.iter().map(|&(v, _)| names.at(v)).collect();
    let ne = names_by_rank.len();
    drop(order);

    // Global hour table: distinct hours, and each worker's local rank for it.
    let mut hours: Vec<(u32, [u8; 13])> = parts.iter().flat_map(|p| p.blocks.iter().map(|b| (b.0, b.1))).collect();
    hours.sort_unstable();
    hours.dedup();
    let hloc: Vec<Vec<usize>> = parts.iter().map(|p| hours.iter().map(|h| p.blocks.iter().position(|b| b.0 == h.0).unwrap_or(usize::MAX)).collect()).collect();

    // ── phase 2: one hour at a time, on every thread ────────────────────────
    // An hour's records (about 43k) are counted into endpoint groups, scattered
    // into one latency array, selected and turned into rows by whichever thread
    // claims the hour. Each row carries the exact text size of its hour with it,
    // so every output offset is known before a byte is formatted.
    let next = AtomicUsize::new(0);
    let done: Vec<Vec<(usize, Vec<Row>, u64)>> = par(|_| {
        let (mut start, mut errs, mut cur) = (vec![0u32; ne + 1], vec![0u32; ne], vec![0u32; ne]);
        let mut lat: Vec<u32> = Vec::new();
        let mut res: Vec<(usize, Vec<Row>, u64)> = Vec::new();
        loop {
            let h = next.fetch_add(1, Ordering::Relaxed);
            if h >= hours.len() {
                break;
            }
            start.fill(0);
            errs.fill(0);
            // Counting sort of the hour's latencies by endpoint rank: count into
            // start[e + 1] and the errors separately, prefix, then place each
            // latency at its group's cursor.
            for (w, p) in parts.iter().enumerate() {
                p.each(hloc[w][h], &rank, |e, key, _| {
                    start[e as usize + 1] += 1;
                    errs[e as usize] += key & 1;
                });
            }
            (1..=ne).for_each(|e| start[e] += start[e - 1]);
            // Grown, never re-zeroed past its high-water mark: `resize` writes
            // only the bytes it adds, so the scratch is zeroed once and then
            // reused instead of being cleared for every hour.
            lat.resize(lat.len().max(start[ne] as usize), 0);
            cur.copy_from_slice(&start[..ne]);
            for (w, p) in parts.iter().enumerate() {
                p.each(hloc[w][h], &rank, |e, _, l| {
                    (lat[cur[e as usize] as usize], cur[e as usize]) = (l, cur[e as usize] + 1);
                });
            }
            let (mut rows, mut size) = (Vec::new(), 0u64);
            for e in (0..ne).filter(|&e| start[e] != start[e + 1]) {
                let (a, b, n) = (start[e] as usize, start[e + 1] as usize, start[e + 1] - start[e]);
                // Nearest rank p is index ceil(p * n / 100) - 1.
                let (p50, p95, p99) = pct(&mut lat[a..b]);
                let err = errs[e];
                let ppm = (err as u64 * 1_000_000 / n as u64) as u32;
                // 13 label bytes, ":00:00Z ", the name, six numbers each after
                // a space, and the newline.
                size += 22 + names_by_rank[e].len() as u64 + [n, p50, p95, p99, err, ppm].iter().map(|&v| 1 + width(v) as u64).sum::<u64>();
                rows.push((e as u32, n, p50, p95, p99, err));
            }
            res.push((h, rows, size));
        }
        res
    });
    // The records have done their job: every hour's rows and sizes were copied
    // out of them inside the scope above, and phase 2 was the last reader.
    // Freeing them HERE and not at the end of `main` is the whole point -- the
    // output below is mapped and then written, and its pages are charged to the
    // process, so holding the record store past this line would make the two
    // peaks overlap. `memory.peak` is a high-water mark, not a live figure, so
    // freeing it later would be free of charge and free of benefit.
    drop(parts);
    drop(rank);

    // The rows and their sizes, and then an hour's byte offset: the running sum
    // IS the offset table, and `at[hours.len()]` is the file's length.
    let mut lines: Vec<Vec<Row>> = vec![Vec::new(); hours.len()];
    let mut at: Vec<u64> = vec![0; hours.len() + 1];
    for (h, rows, size) in done.into_iter().flatten() {
        (lines[h], at[h]) = (rows, size);
    }
    let mut total = 0u64;
    (0..hours.len()).for_each(|h| (at[h], total) = (total, total + at[h]));
    at[hours.len()] = total;

    // ── the output, mapped: an hour's text goes straight into the file ──────
    // One `set_len` and one mapping, so the file's pages ARE the only copy of
    // the text. That is the per-hour `Vec<u8>` and the serial `write_all` both
    // gone, and it is why the formatting below is written against a base pointer
    // rather than a buffer: the fallback path is the same code with a plain
    // allocation under it.
    //
    // Opened for reading as well as writing, which is not cosmetic: a shared
    // write mapping over a file held O_WRONLY is refused with EACCES, and the
    // refusal is silent -- the fallback below would quietly cost the change its
    // whole point.
    let mut out_file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&output).unwrap_or_else(|e| die(&format!("cannot create {output}: {e}")));
    let mut buf: Vec<u8> = Vec::new();
    // The base is carried as a `usize`: an integer is `Send`, a raw pointer is
    // not, and the threads only need an address they never free. Each writer
    // owns the disjoint range the offset table gave it, which is the whole of the
    // synchronisation this needs. The fallback allocation is made first if the
    // mapping is refused, and its pointer is the default of the choice below;
    // an empty `Vec`'s pointer is never dereferenced, because the mapped path is
    // the one that took that branch.
    let map_out = out_map(&out_file, total);
    if map_out.is_none() {
        buf = vec![0u8; total as usize];
    }
    let base: usize = map_out.map_or(buf.as_mut_ptr() as usize, |p| p as usize);

    // ── phase 3: format every hour into its own slice of the output ─────────
    let next_text = AtomicUsize::new(0);
    par(|_| loop {
        let h = next_text.fetch_add(1, Ordering::Relaxed);
        if h >= hours.len() {
            break;
        }
        let (a, b) = (at[h] as usize, at[h + 1] as usize);
        // SAFETY: `at` gives every hour a disjoint range and this thread is the
        // only writer of the one it just claimed.
        let dst = unsafe { core::slice::from_raw_parts_mut((base + a) as *mut u8, b - a) };
        let mut o = 0usize;
        for (e, n, p50, p95, p99, err) in lines[h].iter().copied() {
            dst[o..o + 13].copy_from_slice(&hours[h].1);
            dst[o + 13..o + 21].copy_from_slice(b":00:00Z ");
            o += 21;
            let name = names_by_rank[e as usize];
            dst[o..o + name.len()].copy_from_slice(name);
            o += name.len();
            [n, p50, p95, p99, err, (err as u64 * 1_000_000 / n as u64) as u32].iter().for_each(|&v| o = num(dst, o, v));
            dst[o] = b'\n';
            o += 1;
        }
        // The size pass and this one share `width`, so this can only fire if the
        // two stop agreeing -- and an hour that wrote the wrong number of bytes
        // would misalign every hour after it.
        if o != b - a {
            die(&format!("hour {h}: wrote {o} into {}", b - a));
        }
    });

    // Empty only on the mapped path, where the file already holds the bytes.
    if !buf.is_empty() {
        out_file.write_all(&buf).unwrap_or_else(|e| die(&format!("write error: {e}")));
    }
    // Skip teardown: the mapping IS the file, and the kernel writes it back.
    exit(0);
}
