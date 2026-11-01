//! Track C (Join).
//!
//! Both files are memory-mapped, and every phase is cut into pieces that any
//! thread of one pool claims; a phase starts once the one before is done:
//!  1. regions.csv is split by lines; each thread flags the country codes it
//!     sees. Countries get indices 1..N in code order (0 is _UNMATCHED).
//!  2. Each thread fills its slice of a direct table region_id -> country index.
//!  3. records.csv is split at CRLFs: the generator never writes a CR inside
//!     quotes, so the byte after a CRLF always starts a record. Each record is
//!     read field by field: the id's comma is found in one 8-byte word, a quoted
//!     field's end by stepping through the bit mask of its quotes (a "" pair is
//!     an escape), so commas and LFs inside quotes cost nothing, and the region's
//!     comma and the LF in one 32-byte compare each. Numbers are decoded eight
//!     digits at a time. Sums go to the thread's own per-country totals. Names
//!     go through a shared lock-free intern table to dense ids, which are marked
//!     in one bitset per country; both steps are prefetched records ahead. A
//!     mark that sets a new bit adds one to the country's distinct names.
use std::arch::x86_64::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering::Relaxed};
use std::sync::{Condvar, Mutex, OnceLock};

#[cfg(target_os = "linux")]
extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64)
        -> *mut c_void;
    fn madvise(addr: *mut c_void, len: usize, advice: i32) -> i32;
    fn prctl(op: i32, a: u64, b: u64, c: u64, d: u64) -> i32;
}

/// A file descriptor for the mapping call. Only the Linux mapping uses it.
#[cfg(target_os = "linux")]
fn raw_fd(f: &std::fs::File) -> i32 {
    use std::os::fd::AsRawFd;
    f.as_raw_fd()
}

// ── non-Linux stand-ins ─────────────────────────────────────────────────────
//
// These exist so the three offline corpora can be checked on a development
// machine. The scoring box is Linux and never reaches them. `mmap` with a null
// address hands back zeroed memory; with a non-null one it returns that address,
// which is the MAP_FIXED behaviour `map_file` relies on.
#[cfg(not(target_os = "linux"))]
unsafe fn mmap(addr: *mut c_void, len: usize, _prot: i32, _flags: i32, _fd: i32, _off: i64)
    -> *mut c_void
{
    if !addr.is_null() {
        return addr;
    }
    let v: Vec<u8> = vec![0u8; len.max(1) + 4096];
    Box::leak(v.into_boxed_slice()).as_mut_ptr() as *mut c_void
}

#[cfg(not(target_os = "linux"))]
unsafe fn madvise(_addr: *mut c_void, _len: usize, _advice: i32) -> i32 {
    0
}

#[cfg(not(target_os = "linux"))]
unsafe fn prctl(_op: i32, _a: u64, _b: u64, _c: u64, _d: u64) -> i32 {
    0
}

fn die(msg: &str) -> ! {
    eprintln!("[track-c] {msg}");
    std::process::exit(1)
}

/// Zero-filled anonymous memory; pages are charged only once touched. Only for
/// types whose all-zero bytes are a valid value.
fn zeroed<T>(n: usize) -> &'static [T] {
    let p = unsafe {
        mmap(
            std::ptr::null_mut(),
            n * std::mem::size_of::<T>(),
            3,
            0x22,
            -1,
            0,
        )
    };
    if p as isize == -1 {
        die("cannot map memory");
    }
    unsafe { std::slice::from_raw_parts(p as *const T, n) }
}

/// A table that every thread writes all over is populated up front: faulting
/// it in page by page from twelve threads at once made the threads queue on
/// the kernel's page-table locks, and each page was faulted twice, once on its
/// first read (the shared zero page) and again on its first write. Thread t of
/// n populates its share in one call, so the work is spread too.
fn populate_share<T>(v: &[T], t: usize, n: usize) {
    let pages = (v.len() * std::mem::size_of::<T>() + 4095) / 4096;
    let (from, to) = (pages * t / n, pages * (t + 1) / n);
    if to > from {
        unsafe {
            madvise(
                (v.as_ptr() as usize + from * 4096) as *mut c_void,
                (to - from) * 4096,
                23,
            )
        }; // MADV_POPULATE_WRITE
    }
}

/// Map a file over a spare zero page, so 64-byte loads past its end are safe.
#[cfg(target_os = "linux")]
fn map_file(path: &str) -> (usize, usize) {
    let file =
        std::fs::File::open(path).unwrap_or_else(|e| die(&format!("cannot open {path}: {e}")));
    let size = file.metadata().map_or(0, |m| m.len() as usize);
    let p = unsafe { mmap(std::ptr::null_mut(), size + 4096, 1, 0x22, -1, 0) };
    if size > 0 && unsafe { mmap(p, size, 1, 0x12, raw_fd(&file), 0) } as isize == -1 {
        die(&format!("cannot map {path}"));
    }
    (p as usize, size)
}

/// The same bytes with a spare page, read rather than mapped. Only a development
/// machine takes this; the scoring box is Linux.
#[cfg(not(target_os = "linux"))]
fn map_file(path: &str) -> (usize, usize) {
    let mut v: Vec<u8> =
        std::fs::read(path).unwrap_or_else(|e| die(&format!("cannot open {path}: {e}")));
    let size = v.len();
    v.resize(size + 4096, 0);
    (Box::leak(v.into_boxed_slice()).as_mut_ptr() as usize, size)
}

#[inline(always)]
unsafe fn load8(p: *const u8) -> u64 {
    (p as *const u64).read_unaligned()
}

/// Bit i is set when byte i of the 32 at p equals c.
#[inline(always)]
unsafe fn match32(p: *const u8, c: u8) -> u32 {
    let v = _mm256_loadu_si256(p as *const __m256i);
    _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, _mm256_set1_epi8(c as i8))) as u32
}

#[inline(always)]
unsafe fn match64(p: *const u8, c: u8) -> u64 {
    match32(p, c) as u64 | (match32(p.add(32), c) as u64) << 32
}

/// The value of the last n (1 to 8) of the eight digit bytes in v (the first
/// byte is the lowest): reduced to digit values, the bytes before them masked
/// to leading zeros, then folded in pairs, quads and halves.
#[inline(always)]
fn fold(mut v: u64, n: usize) -> u64 {
    v &= 0x0F0F_0F0F_0F0F_0F0F & (!0u64 << (8 * (8 - n)));
    v = (v * 10 + (v >> 8)) & 0x00FF_00FF_00FF_00FF;
    v = (v * 100 + (v >> 16)) & 0x0000_FFFF_0000_FFFF;
    (v * 10000 + (v >> 32)) & 0xFFFF_FFFF
}

/// The value of the 1 to 8 decimal digits in [s, e).
#[inline(always)]
unsafe fn digits(s: *const u8, e: *const u8) -> u64 {
    fold(load8(e.sub(8)), e.offset_from(s) as usize)
}

/// An amount's value in cents, from its digits [s, e) with the dot third from
/// the end: at most 7 digits (99999.99), so the eight bytes ending at e, with
/// the dot's byte dropped, are one number.
#[inline(always)]
unsafe fn money(s: *const u8, e: *const u8) -> u64 {
    let w = load8(e.sub(8));
    fold(
        (w & 0xFF_FFFF_FFFF) << 8 | (w & 0xFFFF_0000_0000_0000),
        e.offset_from(s) as usize - 1,
    )
}

/// The first c at or after p (one is known to follow). 64 bytes at a time:
/// a note is often longer than 32, and the loop's exit is then predictable.
#[inline(always)]
unsafe fn find(mut p: *const u8, c: u8) -> *const u8 {
    loop {
        let m = match64(p, c);
        if m != 0 {
            return p.add(m.trailing_zeros() as usize);
        }
        p = p.add(64);
    }
}

/// The lowest zero byte of the 8 bytes at p XOR ',': where a number of at most
/// 7 digits ends.
#[inline(always)]
unsafe fn comma_after(p: *const u8) -> *const u8 {
    let x = load8(p) ^ 0x2C2C_2C2C_2C2C_2C2C;
    p.add(
        ((x.wrapping_sub(0x0101_0101_0101_0101) & !x & 0x8080_8080_8080_8080).trailing_zeros() / 8)
            as usize,
    )
}

/// The text field (name or note) at p, moving p past its comma: its raw bytes
/// (between the quotes, if quoted) and its value's length (one byte less per
/// "" pair).
struct Text {
    at: *const u8,
    raw: u32,
    length: u32,
}

#[inline(always)]
unsafe fn text(p: &mut *const u8) -> Text {
    let s = *p;
    if *s != b'"' {
        let comma = find(s, b',');
        *p = comma.add(1);
        let n = comma.offset_from(s) as u32;
        return Text {
            at: s,
            raw: n,
            length: n,
        };
    }
    // The quotes after the opening one, 64 bytes at a time, are stepped
    // through on their bit mask: a quote followed by another is a "" pair, the
    // first one that is not closes the field.
    let mut pairs = 0;
    let mut q = s.add(1);
    let mut m = match64(q, b'"');
    loop {
        let i = if m != 0 { m.trailing_zeros() } else { 64 };
        if i >= 63 {
            // none, or the byte after it is out of view: move on
            q = q.add(i as usize);
            m = match64(q, b'"');
            continue;
        }
        if m >> (i + 1) & 1 == 0 {
            q = q.add(i as usize);
            break;
        }
        pairs += 1;
        m &= m - 1;
        m &= m - 1;
    }
    *p = q.add(2);
    let n = q.offset_from(s) as u32 - 1;
    Text {
        at: s.add(1),
        raw: n,
        length: n - pairs,
    }
}

/// Names, interned by their raw bytes between the quotes. Escaping is one to
/// one, so equal raw bytes mean equal values (and the empty name is one value
/// whether quoted or not). A name's key is a 64-bit hash of those bytes and
/// their length; two of the 100,000 pool names share one with a chance of
/// about 3 in 10^10. A key's slot in the open-addressed table is the name's id;
/// an empty slot is claimed with a compare-and-swap. At 8 bytes a slot the
/// table is 1 MB, small enough to stay in a core's L2 while the input streams
/// past, and eight slots share a cache line, so a probe rarely leaves it.
const BITS: u32 = 17;
const SLOTS: usize = 1 << BITS; // the pool holds 100,000 names

struct NameTable(&'static [AtomicU64]); // 0 = free

impl NameTable {
    fn new() -> Self {
        NameTable(zeroed(SLOTS))
    }

    /// The 64-bit key of the `len` bytes at s (64 more readable after them).
    #[inline(always)]
    unsafe fn key(s: *const u8, len: u32) -> u64 {
        const KEEP: [u8; 32] = {
            let (mut k, mut i) = ([0; 32], 0);
            while i < 16 {
                k[i] = 255;
                i += 1;
            }
            k
        };
        let mask = _mm_loadu_si128(KEEP.as_ptr().add(16 - len.min(16) as usize) as *const __m128i);
        let head = _mm_and_si128(_mm_loadu_si128(s as *const __m128i), mask);
        let (lo, hi) = (
            _mm_cvtsi128_si64(head) as u64,
            _mm_extract_epi64::<1>(head) as u64,
        );
        let mut h = (lo.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ len as u64)
            .wrapping_add(hi.wrapping_mul(0xBF58_476D_1CE4_E5B9));
        for i in (16..len).step_by(8) {
            let mut w = load8(s.add(i as usize));
            if len - i < 8 {
                w &= (1u64 << (8 * (len - i))) - 1;
            }
            h = (h ^ w).wrapping_mul(0x94D0_49BB_1331_11EB);
        }
        h = (h ^ h >> 31).wrapping_mul(0xD6E8_FEB8_6659_FD93);
        h ^= h >> 32;
        h + (h == 0) as u64 // 0 marks a free slot
    }

    #[inline(always)]
    fn prefetch(&self, k: u64) {
        unsafe {
            _mm_prefetch::<_MM_HINT_T0>(
                self.0.as_ptr().add((k >> (64 - BITS)) as usize) as *const i8
            )
        }
    }

    /// Slots are probed four at a time, from the aligned group of four the key
    /// hashes to: one 32-byte compare finds the key, or the first free slot,
    /// which is claimed (no key ever sits past a free slot, as none is freed).
    #[inline(always)]
    fn id(&self, k: u64) -> u32 {
        let mut i = (k >> (64 - BITS)) as usize & !3;
        loop {
            let (hit, mut free) = unsafe {
                let v = _mm256_load_si256(self.0.as_ptr().add(i) as *const __m256i);
                let eq =
                    |x| _mm256_movemask_pd(_mm256_castsi256_pd(_mm256_cmpeq_epi64(v, x))) as u32;
                (eq(_mm256_set1_epi64x(k as i64)), eq(_mm256_setzero_si256()))
            };
            if hit != 0 {
                return (i + hit.trailing_zeros() as usize) as u32;
            }
            while free != 0 {
                let j = i + free.trailing_zeros() as usize;
                match self.0[j].compare_exchange(0, k, Relaxed, Relaxed) {
                    Ok(_) => return j as u32,
                    Err(cur) if cur == k => return j as u32,
                    Err(_) => free &= free - 1,
                }
            }
            i = (i + 4) % SLOTS;
        }
    }
}

/// One country's running aggregates (per thread, summed at the end).
#[derive(Clone, Copy)]
struct Totals {
    count: u64,
    id_sum: u64,
    name_bytes: u64,
    note_bytes: u64,
    distinct: u64,
    amount_sum: i64,
    amount_min: i64,
    amount_max: i64,
}

impl Totals {
    const NEW: Totals = Totals {
        count: 0,
        id_sum: 0,
        name_bytes: 0,
        note_bytes: 0,
        distinct: 0,
        amount_sum: 0,
        amount_min: i64::MAX,
        amount_max: i64::MIN,
    };
}

/// Which names each country has: one bitset over name ids per country, shared
/// by all threads. Most marks are first sightings, so they are atomic ORs.
const WORDS: usize = SLOTS / 64;

struct NamesSeen(&'static [AtomicU64]);

impl NamesSeen {
    fn word(&self, c: u32, name: u32) -> &AtomicU64 {
        &self.0[c as usize * WORDS + name as usize / 64]
    }

    /// For writing: the OR needs the line exclusive, and a plain prefetch would
    /// fetch it shared first.
    #[inline(always)]
    fn prefetch(&self, c: u32, name: u32) {
        unsafe { _mm_prefetch::<_MM_HINT_T0>(self.word(c, name) as *const AtomicU64 as *const i8) }
    }

    /// Whether this is the first time the name is marked for the country, so
    /// the distinct counts come out of the marking (an OR returning the old
    /// bit) with no pass over the bitsets afterwards. No test first: the read
    /// would fetch the line shared, then the OR would fetch it again exclusive.
    #[inline(always)]
    fn mark(&self, c: u32, name: u32) -> bool {
        let bit = 1u64 << (name % 64);
        self.word(c, name).fetch_or(bit, Relaxed) & bit == 0
    }
}

/// What the record pass reads, and the shared tables it fills.
struct Cx<'a> {
    country_of: &'a [AtomicU8],
    regions: usize,
    names: &'a NameTable,
    seen: &'a NamesSeen,
}

const LAG: u64 = 16;

#[derive(Clone, Copy)]
struct Pending {
    name: u64,
    region: u32,
    id: u32,
    name_bytes: u32,
    note_bytes: u32,
    cents: i64,
}

/// A record's region row and name slot are prefetched when it is parsed; LAG
/// records later its country is read, its sums go to totals, its name is looked
/// up and that name's bitset word prefetched; LAG records after that the name
/// is marked. So each of the three random reads (region table, name table,
/// bitsets) has a steady lead.
struct Pipe {
    pending: [Pending; LAG as usize],
    marks: [(u32, u32); LAG as usize], // country, name id
    n: u64,                            // records started
    last: u64,                         // all of them, once known
}

impl Pipe {
    /// Retire ring slot n % LAG.
    #[inline(always)]
    fn advance(&mut self, cx: &Cx, totals: &mut [Totals]) {
        let k = (self.n % LAG) as usize;
        if self.n >= 2 * LAG && self.n - 2 * LAG < self.last {
            let (c, id) = self.marks[k];
            totals[c as usize].distinct += cx.seen.mark(c, id) as u64;
        }
        if self.n >= LAG && self.n - LAG < self.last {
            let old = self.pending[k];
            let c = if (old.region as usize) < cx.regions {
                cx.country_of[old.region as usize].load(Relaxed) as u32
            } else {
                0
            };
            let t = &mut totals[c as usize];
            t.count += 1;
            t.id_sum += old.id as u64;
            t.amount_sum += old.cents;
            t.amount_min = t.amount_min.min(old.cents);
            t.amount_max = t.amount_max.max(old.cents);
            t.name_bytes += old.name_bytes as u64;
            t.note_bytes += old.note_bytes as u64;
            let id = cx.names.id(old.name);
            self.marks[k] = (c, id);
            cx.seen.prefetch(c, id);
        }
    }
}

/// Parse every record that starts in [p, end).
unsafe fn parse_records(
    mut p: *const u8,
    end: *const u8,
    file_end: *const u8,
    cx: &Cx,
    totals: &mut [Totals],
) {
    let empty = Pending {
        name: 0,
        region: 0,
        id: 0,
        name_bytes: 0,
        note_bytes: 0,
        cents: 0,
    };
    let mut pipe = Pipe {
        pending: [empty; LAG as usize],
        marks: [(0, 0); LAG as usize],
        n: 0,
        last: u64::MAX,
    };
    while p < end {
        // The file's pages are scattered in memory, and the hardware prefetcher
        // stops at each 4 KB page; this reaches into the next one in time.
        _mm_prefetch::<_MM_HINT_T0>(p.wrapping_add(2048) as *const i8);
        if *p == 0xEF {
            p = p.add(3); // a byte order mark before the id
        }
        let f1 = comma_after(p);
        let id = digits(p, f1);
        p = f1.add(1);
        let name = text(&mut p);
        let note = text(&mut p);
        // The region (at most 7 digits) and the amount (at most 9 bytes) end
        // within the 32 bytes at p, so one load gives both the comma and the
        // LF, if any.
        let lf = match32(p, b'\n');
        let f4 = p.add(match32(p, b',').trailing_zeros() as usize);
        let region = digits(p, f4);
        let amount = f4.add(1);
        let eol = if lf != 0 {
            p.add(lf.trailing_zeros() as usize)
        } else {
            file_end
        };
        let amount_end = eol.sub((*eol.sub(1) == b'\r') as usize);
        let negative = *amount == b'-';
        let cents = money(amount.add(negative as usize), amount_end) as i64;

        // Region ids are below 10^7 (fit u32); one past the table means none.
        let r = if (region as usize) < cx.regions {
            region as u32
        } else {
            cx.regions as u32
        };
        _mm_prefetch::<_MM_HINT_T0>(cx.country_of.as_ptr().wrapping_add(r as usize) as *const i8);
        pipe.advance(cx, totals);
        let key = NameTable::key(name.at, name.raw);
        pipe.pending[(pipe.n % LAG) as usize] = Pending {
            name: key,
            region: r,
            id: id as u32,
            name_bytes: name.length,
            note_bytes: note.length,
            cents: if negative { -cents } else { cents },
        };
        pipe.n += 1;
        cx.names.prefetch(key);
        p = eol.add(1);
    }
    pipe.last = pipe.n;
    for _ in 0..2 * LAG {
        pipe.advance(cx, totals);
        pipe.n += 1;
    }
}

/// 15-bit code of a three-letter country; numeric order is alphabetical order.
unsafe fn code(p: *const u8) -> u32 {
    ((*p - b'A') as u32) << 10 | ((*p.add(1) - b'A') as u32) << 5 | (*p.add(2) - b'A') as u32
}

/// The region line "id,ABC" at p, moving p to the next line. An id has at most
/// 7 digits, so the comma is the lowest zero byte of the line's first 8 bytes
/// XOR ','.
unsafe fn region_line(p: &mut *const u8) -> (u64, u32) {
    let comma = comma_after(*p);
    let line = (digits(*p, comma), code(comma.add(1)));
    *p = comma.add(4);
    while **p != b'\n' {
        *p = p.add(1);
    }
    *p = p.add(1);
    line
}

/// A phase of work in pieces. Every thread claims pieces until none are left,
/// then sleeps until all are done. It waits for the work, not for every thread
/// to arrive: a thread starts milliseconds after the one before it, and the
/// late ones just join in wherever the work has got to.
struct Phase {
    claimed: AtomicUsize,
    done: Mutex<usize>,
    all: Condvar,
    pieces: usize,
}

impl Phase {
    fn new(pieces: usize) -> Self {
        Phase {
            claimed: AtomicUsize::new(0),
            done: Mutex::new(0),
            all: Condvar::new(),
            pieces,
        }
    }

    fn run(&self, mut f: impl FnMut(usize)) {
        loop {
            let k = self.claimed.fetch_add(1, Relaxed);
            if k >= self.pieces {
                break;
            }
            f(k);
            let mut done = self.done.lock().unwrap();
            *done += 1;
            if *done == self.pieces {
                self.all.notify_all();
            }
        }
        let mut done = self.done.lock().unwrap();
        while *done < self.pieces {
            done = self.all.wait(done).unwrap();
        }
    }
}

/// Cuts of [from, size) into n ranges, each starting just after a delimiter.
fn split(data: &[u8], from: usize, n: usize, delim: &[u8]) -> Vec<usize> {
    let size = data.len();
    let mut cut = vec![size; n + 1];
    cut[0] = from;
    for t in 1..n {
        let c = cut[t - 1].max(from + (size - from) * t / n);
        cut[t] = data[c..]
            .windows(delim.len())
            .position(|w| w == delim)
            .map_or(size, |i| c + i + delim.len());
    }
    cut
}

fn put_money(out: &mut String, cents: i64) {
    let a = cents.unsigned_abs();
    out.push_str(if cents < 0 { " -" } else { " " });
    out.push_str(&format!("{}.{:02}", a / 100, a % 100));
}

fn main() {
    // With transparent huge pages, a block written only in part can be backed
    // by a whole 2 MB page; plain pages keep memory to what is written.
    unsafe { prctl(41, 1, 0, 0, 0) }; // PR_SET_THP_DISABLE
    let nt = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, 12);
    let data = std::env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = std::env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());
    let (regions, rsize) = map_file(&format!("{data}/regions.csv"));
    let (records, size) = map_file(&format!("{data}/records.csv"));
    let (rbytes, bytes) = unsafe {
        (
            std::slice::from_raw_parts(regions as *const u8, rsize),
            std::slice::from_raw_parts(records as *const u8, size),
        )
    };

    // regions.csv: a header, then "id,ABC\n" lines in ascending id order, so
    // the last line holds the largest id.
    let last = rbytes[..rsize.saturating_sub(1)]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |i| i + 1);
    let nregions = rbytes[last..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .fold(0usize, |v, &d| v * 10 + (d - b'0') as usize)
        + 1;
    let country_of: &[AtomicU8] = zeroed(nregions); // 0 = no such region
    let header = rbytes
        .iter()
        .position(|&c| c == b'\n')
        .map_or(rsize, |i| i + 1);
    let rcut = split(rbytes, header, nt, b"\n");
    let present: Vec<AtomicU8> = (0..1 << 15).map(|_| AtomicU8::new(0)).collect();
    let numbered: OnceLock<(Vec<u32>, Vec<u8>)> = OnceLock::new(); // country index - 1 -> code, and back
    let number_countries = || {
        let codes: Vec<u32> = (0..present.len() as u32)
            .filter(|&k| present[k as usize].load(Relaxed) != 0)
            .collect();
        if codes.len() > 255 {
            die("too many countries");
        }
        let mut index_of = vec![0u8; present.len()];
        codes
            .iter()
            .enumerate()
            .for_each(|(i, &k)| index_of[k as usize] = i as u8 + 1);
        (codes, index_of)
    };

    // records.csv: skip the BOM and header, then split after CRLFs into eight
    // pieces per thread, claimed in turn, so a thread whose pieces are slow
    // (more first sightings of names) is evened out by the others.
    let start = bytes.iter().position(|&c| c == b'\n').map_or(0, |i| i + 1);
    let pieces = 8 * nt;
    let cut = split(bytes, start, pieces, b"\r\n");
    let names = NameTable::new();
    let seen = NamesSeen(zeroed(256 * WORDS));

    // Populate tables and flag countries; populate bitsets and fill the region
    // table; parse records.
    let (flag, fill, parse) = (Phase::new(nt), Phase::new(nt), Phase::new(pieces));
    let all: Vec<Vec<Totals>> = std::thread::scope(|s| {
        let pool: Vec<_> = (0..nt)
            .map(|_| {
                s.spawn(|| {
                    let mut totals = vec![Totals::NEW; 256];
                    flag.run(|k| {
                        populate_share(country_of, k, nt);
                        populate_share(names.0, k, nt);
                        let mut p = (regions + rcut[k]) as *const u8;
                        while (p as usize) < regions + rcut[k + 1] {
                            present[unsafe { region_line(&mut p) }.1 as usize].store(1, Relaxed);
                        }
                    });
                    let index_of = &numbered.get_or_init(number_countries).1;
                    fill.run(|k| {
                        populate_share(seen.0, k, nt);
                        let mut p = (regions + rcut[k]) as *const u8;
                        while (p as usize) < regions + rcut[k + 1] {
                            let (id, c) = unsafe { region_line(&mut p) };
                            country_of[id as usize].store(index_of[c as usize], Relaxed);
                        }
                    });
                    let cx = Cx {
                        country_of,
                        regions: nregions,
                        names: &names,
                        seen: &seen,
                    };
                    parse.run(|k| unsafe {
                        // Map the piece's pages in one call rather than by a fault
                        // every 16 pages as the parse reaches them.
                        let from = (records + cut[k]) & !4095;
                        madvise(from as *mut c_void, records + cut[k + 1] - from, 22); // MADV_POPULATE_READ
                        parse_records(
                            (records + cut[k]) as *const u8,
                            (records + cut[k + 1]) as *const u8,
                            (records + size) as *const u8,
                            &cx,
                            &mut totals,
                        );
                    });
                    totals
                })
            })
            .collect();
        pool.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let codes = &numbered.get().unwrap().0;
    let mut out = String::new();
    for i in 1..=codes.len() + 1 {
        let c = i % (codes.len() + 1); // countries 1..N, then _UNMATCHED (0)
        let mut sum = Totals::NEW;
        for t in all.iter().map(|per_thread| &per_thread[c]) {
            sum.count += t.count;
            sum.id_sum += t.id_sum;
            sum.name_bytes += t.name_bytes;
            sum.note_bytes += t.note_bytes;
            sum.distinct += t.distinct;
            sum.amount_sum += t.amount_sum;
            sum.amount_min = sum.amount_min.min(t.amount_min);
            sum.amount_max = sum.amount_max.max(t.amount_max);
        }
        if sum.count == 0 {
            continue;
        }
        if c == 0 {
            out.push_str("_UNMATCHED");
        } else {
            let k = codes[c - 1];
            out.extend([
                (b'A' + (k >> 10) as u8) as char,
                (b'A' + (k >> 5 & 31) as u8) as char,
                (b'A' + (k & 31) as u8) as char,
            ]);
        }
        out.push_str(&format!(" {} {}", sum.count, sum.id_sum));
        for v in [sum.amount_sum, sum.amount_min, sum.amount_max] {
            put_money(&mut out, v);
        }
        out.push_str(&format!(
            " {} {} {}\n",
            sum.distinct, sum.name_bytes, sum.note_bytes
        ));
    }
    std::fs::write(format!("{outd}/aggregates.txt"), out)
        .unwrap_or_else(|e| die(&format!("cannot write aggregates.txt: {e}")));
}
