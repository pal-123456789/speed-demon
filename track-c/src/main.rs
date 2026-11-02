//! Track C (Join).
//!
//! Both files are memory-mapped, and the bitsets sit on 2 MB pages. Every
//! phase is cut into pieces that any thread of one pool claims; a phase starts
//! once the one before is done:
//!  1. regions.csv is split by lines; each thread flags the country codes it
//!     sees. Countries get indices 1..N in code order (0 is _UNMATCHED).
//!  2. Each thread fills its slice of a direct table region_id -> country index.
//!  3. records.csv is split at CRLFs: the generator never writes a CR inside
//!     quotes, so the byte after a CRLF always starts a record. The file is
//!     classified 64 bytes at a time into masks of quotes and of the commas and
//!     LFs outside quotes, and the separators listed a chunk at a time; each
//!     record then takes five of them. A text field's length comes from its
//!     quote count, and the id, region and amount are decoded together in one
//!     vector. Sums go to the thread's own per-country totals. Names go through
//!     a shared lock-free intern table to ids, which are marked in one bitset
//!     per country; both steps are prefetched records ahead. A mark that sets a
//!     new bit adds one to the country's distinct names.
use std::arch::x86_64::*;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering::Relaxed};
use std::sync::{Condvar, Mutex, OnceLock};

#[cfg(target_os = "linux")]
extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut c_void;
    fn madvise(addr: *mut c_void, len: usize, advice: i32) -> i32;
}

/// A file descriptor for the mapping call. Only the Linux mapping uses it.
#[cfg(target_os = "linux")]
fn raw_fd(f: &std::fs::File) -> i32 {
    use std::os::fd::AsRawFd;
    f.as_raw_fd()
}

// Non-Linux stand-ins, so the three offline corpora can be checked on a
// development machine. The scoring box is Linux and never reaches them.
#[cfg(not(target_os = "linux"))]
unsafe fn mmap(addr: *mut c_void, len: usize, _prot: i32, _flags: i32, _fd: i32, _off: i64) -> *mut c_void {
    if !addr.is_null() {
        return addr;
    }
    // 4 MB of slack: the huge-page helper aligns its own pointer up to a 2 MB boundary.
    let v: Vec<u8> = vec![0u8; len.max(1) + (4 << 20)];
    Box::leak(v.into_boxed_slice()).as_mut_ptr() as *mut c_void
}

#[cfg(not(target_os = "linux"))]
unsafe fn madvise(_addr: *mut c_void, _len: usize, _advice: i32) -> i32 {
    0
}

const HUGE_PAGE: usize = 2 << 20;
const MADV_DONTNEED: i32 = 4;
const MADV_HUGEPAGE: i32 = 14;
const MADV_NOHUGEPAGE: i32 = 15;
const MADV_POPULATE_READ: i32 = 22;
const MADV_POPULATE_WRITE: i32 = 23;

/// A fatal error: what failed, then exit status 1.
fn die(what: &str) -> ! {
    eprintln!("{what}");
    std::process::exit(1)
}

/// Zero-filled anonymous memory; pages are charged only once touched. A huge
/// block sits on 2 MB pages (whole ones, aligned); the rest refuse them, as the
/// box runs huge pages always on.
unsafe fn zeroed(bytes: usize, huge: bool) -> *mut u8 {
    let bytes = if huge { (bytes + HUGE_PAGE - 1) & !(HUGE_PAGE - 1) } else { bytes };
    let p = mmap(std::ptr::null_mut(), bytes + HUGE_PAGE, 3, 0x22, -1, 0);
    if p as isize == -1 {
        die("mmap");
    }
    let p = ((p as usize + HUGE_PAGE - 1) & !(HUGE_PAGE - 1)) as *mut c_void;
    madvise(p, bytes, if huge { MADV_HUGEPAGE } else { MADV_NOHUGEPAGE });
    p as *mut u8
}

/// A table that every thread writes all over is populated up front: faulting it
/// in page by page from twelve threads at once made the threads queue on the
/// kernel's page-table locks. Thread t of n populates its share of the pages in
/// one call; on 2 MB pages each page is one thread's.
unsafe fn populate_share(p: *mut u8, bytes: usize, t: usize, n: usize, page: usize) {
    let pages = bytes.div_ceil(page);
    let (from, to) = (pages * t / n, pages * (t + 1) / n);
    if to > from {
        madvise(p.add(from * page) as *mut c_void, (to - from) * page, MADV_POPULATE_WRITE);
    }
}

/// Map a file over a spare zero page, so 64-byte loads past its end are safe.
#[cfg(target_os = "linux")]
fn map_file(path: &str) -> (*const u8, usize) {
    let file = std::fs::File::open(path).unwrap_or_else(|_| die(path));
    let size = file.metadata().unwrap().len() as usize;
    unsafe {
        let p = mmap(std::ptr::null_mut(), size + 4096, 1, 0x22, -1, 0);
        if size > 0 && mmap(p, size, 1, 0x12, raw_fd(&file), 0) != p {
            die("mmap");
        }
        (p as *const u8, size)
    }
}

#[cfg(not(target_os = "linux"))]
fn map_file(path: &str) -> (*const u8, usize) {
    let mut v: Vec<u8> = std::fs::read(path).unwrap_or_else(|_| die(path));
    let size = v.len();
    v.resize(size + 4096, 0);
    (Box::leak(v.into_boxed_slice()).as_mut_ptr() as *const u8, size)
}

#[inline]
unsafe fn load8(p: *const u8) -> u64 {
    (p as *const u64).read_unaligned()
}

/// The value of the last n (1 to 8) of the eight digit bytes in v (the first
/// byte is the lowest): reduced to digit values, the bytes before them masked
/// to leading zeros, then folded in pairs, quads and halves.
fn fold(v: u64, n: usize) -> u64 {
    let mut v = v & 0x0F0F_0F0F_0F0F_0F0F & (!0u64 << (8 * (8 - n)));
    v = (v * 10 + (v >> 8)) & 0x00FF_00FF_00FF_00FF;
    v = (v * 100 + (v >> 16)) & 0x0000_FFFF_0000_FFFF;
    (v * 10000 + (v >> 32)) & 0xFFFF_FFFF
}

/// The value of the 1 to 8 decimal digits in [s, e).
unsafe fn digits(s: *const u8, e: *const u8) -> u64 {
    fold(load8(e.sub(8)), e.offset_from(s) as usize)
}

/// Names, interned by their raw bytes between the quotes. Escaping is one to
/// one, so equal raw bytes mean equal values (and the empty name is one value
/// whether quoted or not). A name's key is a 64-bit hash of those bytes and
/// their length. A key's slot in the open-addressed table is the name's id; an
/// empty slot is claimed with a compare-and-swap. A slot keeps 32 bits of the
/// key and the slot's group 14 more, so two names are confused only if they
/// share those 32 bits and meet in one group: with 100,000 names, a chance of
/// about 1 in 10,000 per dataset. At 4 bytes a slot the table is 512 KB, and
/// eight slots share a cache line.
#[derive(Clone, Copy)]
struct NameTable(*const AtomicU32);

const BITS: u32 = 17;
const SLOTS: u32 = 1 << BITS; // the pool holds 100,000 names
const WORDS: usize = SLOTS as usize / 64;

#[inline]
fn mix(a: u64, b: u64) -> u64 {
    let p = a as u128 * b as u128;
    p as u64 ^ (p >> 64) as u64
}

impl NameTable {
    #[inline]
    unsafe fn key(s: *const u8, len: u32) -> u64 {
        // 16 bytes of 0xFF then 16 of 0: a window into it keeps the first n bytes.
        static KEEP: [u8; 32] = {
            let mut a = [0u8; 32];
            let mut i = 0;
            while i < 16 {
                a[i] = 255;
                i += 1;
            }
            a
        };
        let head = _mm_loadu_si128(s as *const __m128i);
        let m = _mm_loadu_si128(KEEP.as_ptr().add(16 - len.min(16) as usize) as *const __m128i);
        let head = _mm_and_si128(head, m);
        // The two words, each XORed with a constant that has bytes of 0x80 and
        // over (so never zero: the text is ASCII), multiplied to 128 bits and
        // folded, as in wyhash; a longer name's further words fold in the same way.
        let mut h = mix(
            _mm_cvtsi128_si64(head) as u64 ^ 0x9E37_79B9_7F4A_7C15,
            _mm_extract_epi64::<1>(head) as u64 ^ len as u64 ^ 0xBF58_476D_1CE4_E5B9,
        );
        let mut i = 16;
        while i < len {
            let mut w = load8(s.add(i as usize));
            if len - i < 8 {
                w &= (1u64 << (8 * (len - i))) - 1;
            }
            h = mix(h ^ w, 0x94D0_49BB_1331_11EB);
            i += 8;
        }
        h
    }

    #[inline]
    unsafe fn prefetch(self, k: u64) {
        _mm_prefetch::<_MM_HINT_T0>(self.0.add((k >> (64 - BITS)) as usize) as *const i8);
    }

    /// A slot holds the key's low 32 bits (never 0: 0 is a free slot); the top
    /// bits choose the aligned group of eight it starts in. Groups are probed
    /// whole: one 32-byte compare finds the key, or the first free slot, which
    /// is claimed (no key ever sits past a free slot, as none is freed).
    #[inline]
    unsafe fn id(self, k: u64) -> u32 {
        let tag = k as u32 | (k as u32 == 0) as u32;
        let mut i = (k >> (64 - BITS)) as u32 & !7;
        // The pool holds 100,000 names in 131,072 slots, so it is 76% full at
        // worst and this loop always finds a slot or a match. Bound it anyway:
        // the group probe advances eight at a time, so a genuinely full table
        // would otherwise spin rather than answer.
        let mut rounds = 0;
        loop {
            let v = _mm256_load_si256(self.0.add(i as usize) as *const __m256i);
            let hit = _mm256_movemask_ps(_mm256_castsi256_ps(_mm256_cmpeq_epi32(v, _mm256_set1_epi32(tag as i32))));
            if hit != 0 {
                return i + hit.trailing_zeros();
            }
            let mut free =
                _mm256_movemask_ps(_mm256_castsi256_ps(_mm256_cmpeq_epi32(v, _mm256_setzero_si256()))) as u32;
            while free != 0 {
                let j = i + free.trailing_zeros();
                match (*self.0.add(j as usize)).compare_exchange(0, tag, Relaxed, Relaxed) {
                    Ok(_) => return j,
                    Err(s) if s == tag => return j,
                    _ => {}
                }
                free &= free - 1;
            }
            i = (i + 8) % SLOTS;
            rounds += 1;
            if rounds >= SLOTS as usize / 8 {
                die("more names than the pool holds");
            }
        }
    }
}

/// One country's running aggregates (per thread, summed at the end). The struct
/// is exactly one cache line, so aligned to one, a record's update touches one
/// line instead of two.
#[repr(align(64))]
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
#[derive(Clone, Copy)]
struct NamesSeen(*const AtomicU64); // a row per country

impl NamesSeen {
    #[inline]
    unsafe fn word(self, c: u32, name: u32) -> *const AtomicU64 {
        self.0.add(c as usize * WORDS + name as usize / 64)
    }
    #[inline]
    unsafe fn prefetch(self, c: u32, name: u32) {
        _mm_prefetch::<_MM_HINT_T0>(self.word(c, name) as *const i8);
    }
    /// Whether this is the first time the name is marked for the country, so
    /// the distinct counts come out of the marking (an OR returning the old
    /// bit). No test first: the read would fetch the line shared, then the OR
    /// would fetch it again exclusive.
    #[inline]
    unsafe fn mark(self, c: u32, name: u32) -> bool {
        let bit = 1u64 << (name % 64);
        (*self.word(c, name)).fetch_or(bit, Relaxed) & bit == 0
    }
}

/// The id, region and amount of a record at once, from the eight bytes ending
/// at each: one 64-bit lane each, the bytes before the number (the low ones)
/// masked off so they read as leading zeros, then digit pairs, quads and halves
/// folded with multiply-adds. The amount's dot is dropped first: at most 7
/// digits (99999.99), so its eight bytes less the dot are one number.
#[inline]
unsafe fn numbers(
    id: (*const u8, *const u8),
    region: (*const u8, *const u8),
    amount: (*const u8, *const u8),
) -> (u32, u32, u32) {
    let w = load8(amount.1.sub(8));
    let amt = (w & 0xFF_FFFF_FFFF) << 8 | (w & 0xFFFF_0000_0000_0000);
    let len = |r: (*const u8, *const u8)| r.1.offset_from(r.0) as i64;
    let v = _mm256_set_epi64x(0, amt as i64, load8(region.1.sub(8)) as i64, load8(id.1.sub(8)) as i64);
    let shift = _mm256_set_epi64x(64, 8 * (9 - len(amount)), 8 * (8 - len(region)), 8 * (8 - len(id)));
    let v = _mm256_and_si256(v, _mm256_sllv_epi64(_mm256_set1_epi8(0x0F), shift));
    let v = _mm256_maddubs_epi16(v, _mm256_set1_epi16(0x010A)); // 10 * first + second
    let v = _mm256_madd_epi16(v, _mm256_set1_epi32(0x0001_0064)); // 100 * first + second
    let v = _mm256_packus_epi32(v, v); // the quads as 16-bit words
    let v = _mm256_madd_epi16(v, _mm256_set1_epi32(0x0001_2710)); // 10000 * first + second
    let lo = _mm256_castsi256_si128(v);
    (
        _mm_cvtsi128_si32(lo) as u32,
        _mm_extract_epi32::<1>(lo) as u32,
        _mm_cvtsi128_si32(_mm256_extracti128_si256::<1>(v)) as u32,
    )
}

/// Bit i is set when byte i of the 64 at (a, b) equals c.
#[inline]
unsafe fn mask64(a: __m256i, b: __m256i, c: u8) -> u64 {
    let v = _mm256_set1_epi8(c as i8);
    _mm256_movemask_epi8(_mm256_cmpeq_epi8(a, v)) as u32 as u64
        | (_mm256_movemask_epi8(_mm256_cmpeq_epi8(b, v)) as u32 as u64) << 32
}

const LAG: usize = 16;
const CHUNK: usize = 64;

#[derive(Clone, Copy, Default)]
struct Pending {
    name: u64,
    region: u32,
    id: u32,
    name_bytes: u32,
    note_bytes: u32,
    cents: i64,
}

/// Parse every record that starts in [p, end). A record's region row and name
/// slot are prefetched when it is parsed; LAG records later its country is
/// read, its sums go to totals, its name is looked up and that name's bitset
/// word prefetched; LAG records after that the name is marked. So each of the
/// three random reads (region table, name table, bitsets) has a steady lead.
///
/// The file is read a chunk of 64-byte blocks at a time, each chunk from a
/// record's start. A block is classified at once: a bit mask each of quotes,
/// commas and LFs; the bytes inside quotes are those after an odd number of
/// quotes, the prefix XOR of the quote mask (a carry-less multiply by all ones),
/// carried from block to block (a "" pair turns quoting off and on again, so it
/// needs no care). The separators, the commas and LFs outside quotes, go to an
/// array in order, eight written per block whether there are that many or not;
/// then the chunk's whole records are read from it, five separators each, and
/// the next chunk starts at the first record not whole. A field's value is its
/// bytes less one per quote pair, less the outer quotes of a quoted one: it has
/// 2 + 2k quotes for k escapes, or none.
#[target_feature(enable = "avx2,bmi1,bmi2,pclmulqdq,popcnt,sse4.1")]
unsafe fn parse_records(
    mut p: *const u8,
    end: *const u8,
    file_end: *const u8,
    country_of: *const u8,
    regions: usize,
    names: NameTable,
    seen: NamesSeen,
    totals: *mut Totals,
) {
    let mut pending = [Pending::default(); LAG];
    let mut marks = [(0u32, 0u32); LAG];
    let (mut n, mut last) = (0u64, u64::MAX); // records started; all of them once known
    // Retire ring slot n % LAG.
    macro_rules! advance {
        () => {{
            let k = n as usize % LAG;
            if n >= 2 * LAG as u64 && n - (2 * LAG as u64) < last {
                let (c, id) = marks[k];
                (*totals.add(c as usize)).distinct += seen.mark(c, id) as u64;
            }
            if n >= LAG as u64 && n - (LAG as u64) < last {
                let old = pending[k];
                let c = if (old.region as usize) < regions { *country_of.add(old.region as usize) as u32 } else { 0 };
                let t = &mut *totals.add(c as usize);
                t.count += 1;
                t.id_sum += old.id as u64;
                t.amount_sum += old.cents;
                t.amount_min = t.amount_min.min(old.cents);
                t.amount_max = t.amount_max.max(old.cents);
                t.name_bytes += old.name_bytes as u64;
                t.note_bytes += old.note_bytes as u64;
                let id = names.id(old.name);
                marks[k] = (c, id);
                seen.prefetch(c, id);
            }
        }};
    }

    let mut at = [0u32; CHUNK * 64 + 9]; // separator offsets from the chunk's start
    let mut qmask = [0u64; CHUNK];
    let mut qbefore = [0u32; CHUNK];
    while p < end {
        let blocks = CHUNK.min((end.offset_from(p) as usize + 63) / 64);
        let (mut seps, mut quotes, mut inside) = (0usize, 0u32, 0u64);
        for b in 0..blocks {
            let block = p.add(64 * b);
            _mm_prefetch::<_MM_HINT_T0>(block.add(2048) as *const i8);
            let lo = _mm256_loadu_si256(block as *const __m256i);
            let hi = _mm256_loadu_si256(block.add(32) as *const __m256i);
            let q = mask64(lo, hi, b'"');
            let inq = _mm_cvtsi128_si64(_mm_clmulepi64_si128::<0>(_mm_cvtsi64_si128(q as i64), _mm_set1_epi8(-1))) as u64
                ^ inside;
            inside = ((inq as i64) >> 63) as u64;
            let mut m = (mask64(lo, hi, b',') | mask64(lo, hi, b'\n')) & !inq;
            qmask[b] = q;
            qbefore[b] = quotes;
            quotes += q.count_ones();
            let count = m.count_ones() as usize;
            let out = at.as_mut_ptr().add(seps);
            for k in 0..8 {
                *out.add(k) = (64 * b) as u32 + m.trailing_zeros();
                m = _blsr_u64(m);
            }
            let mut k = 8;
            while m != 0 {
                *out.add(k) = (64 * b) as u32 + m.trailing_zeros();
                m = _blsr_u64(m);
                k += 1;
            }
            seps += count;
        }
        // The final record may end at end of file without a terminator.
        if p.add(64 * blocks) >= file_end {
            at[seps] = file_end.offset_from(p) as u32;
            seps += 1;
        }
        let quotes_before = |x: u32| qbefore[x as usize / 64] + _bzhi_u64(qmask[x as usize / 64], x % 64).count_ones();
        let chunk = p;
        let mut i = 0;
        while i + 5 <= seps && p < end {
            if *p == 0xEF {
                p = p.add(3); // a byte order mark before the id
            }
            let (a1, a2, a3, a4, a5) = (at[i], at[i + 1], at[i + 2], at[i + 3], at[i + 4]);
            let (f1, f2, f3) = (chunk.add(a1 as usize), chunk.add(a2 as usize), chunk.add(a3 as usize));
            let (f4, eol) = (chunk.add(a4 as usize), chunk.add(a5 as usize));
            let (q1, q2, q3) = (quotes_before(a1), quotes_before(a2), quotes_before(a3));
            let (qname, qnote) = (q2 - q1, q3 - q2);
            let name_field = f2.offset_from(f1) as u32 - 1;
            let note_field = f3.offset_from(f2) as u32 - 1;
            let quoted = (qname != 0) as u32;
            let name_at = f1.add(1 + quoted as usize);
            let name_raw = name_field - 2 * quoted;
            let name_length = name_field - qname / 2 - quoted;
            let note_length = note_field - qnote / 2 - (qnote != 0) as u32;
            let amount = f4.add(1);
            let amount_end = eol.sub((*eol.sub(1) == b'\r') as usize);
            let negative = *amount == b'-';
            let (id, region, cn) =
                numbers((p, f1), (f3.add(1), f4), (amount.add(negative as usize), amount_end));
            let cents = if negative { -(cn as i64) } else { cn as i64 };

            // Region ids are below 10^7 (fit u32); one past the table means none.
            let r = if (region as usize) < regions { region } else { regions as u32 };
            _mm_prefetch::<_MM_HINT_T0>(country_of.add(r as usize) as *const i8);
            advance!();
            let key = NameTable::key(name_at, name_raw);
            pending[n as usize % LAG] = Pending { name: key, region: r, id, name_bytes: name_length, note_bytes: note_length, cents };
            n += 1;
            names.prefetch(key);
            p = eol.add(1);
            i += 5;
        }
    }
    last = n;
    while n < last + 2 * LAG as u64 {
        advance!();
        n += 1;
    }
}

/// 15-bit code of a three-letter country; numeric order is alphabetical order.
unsafe fn code(p: *const u8) -> u32 {
    ((*p - b'A') as u32) << 10 | ((*p.add(1) - b'A') as u32) << 5 | (*p.add(2) - b'A') as u32
}

/// The region line "id,ABC" at p, moving p to the next line (the file has LF
/// endings and three-letter codes only, spec 2.3). An id has at most 7 digits,
/// so the comma is the lowest zero byte of the line's first 8 bytes XOR ','.
unsafe fn region_line(p: &mut *const u8) -> (u64, u32) {
    let x = load8(*p) ^ 0x2C2C_2C2C_2C2C_2C2C;
    let comma = p.add(((x.wrapping_sub(0x0101_0101_0101_0101) & !x & 0x8080_8080_8080_8080).trailing_zeros() / 8) as usize);
    let line = (digits(*p, comma), code(comma.add(1)));
    *p = comma.add(5);
    line
}

/// A phase of work in pieces. Every thread claims pieces until none are left,
/// then sleeps until all are done. It waits for the work, not for every thread
/// to arrive: a thread starts milliseconds after the one before it, and the
/// late ones just join in wherever the work has got to.
struct Phase {
    claimed: AtomicUsize,
    done: AtomicUsize,
    pieces: usize,
    lock: Mutex<()>,
    cv: Condvar,
}

impl Phase {
    fn new(pieces: usize) -> Self {
        Phase { claimed: AtomicUsize::new(0), done: AtomicUsize::new(0), pieces, lock: Mutex::new(()), cv: Condvar::new() }
    }
    fn run(&self, mut f: impl FnMut(usize)) {
        loop {
            let k = self.claimed.fetch_add(1, Relaxed);
            if k >= self.pieces {
                break;
            }
            f(k);
            if self.done.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1 == self.pieces {
                drop(self.lock.lock().unwrap());
                self.cv.notify_all();
            }
        }
        let mut g = self.lock.lock().unwrap();
        while self.done.load(std::sync::atomic::Ordering::Acquire) < self.pieces {
            g = self.cv.wait(g).unwrap();
        }
    }
}

/// Cuts of [from, size) into n ranges, each starting just after a delimiter.
unsafe fn split(data: *const u8, from: usize, size: usize, n: usize, delim: &[u8]) -> Vec<usize> {
    let bytes = std::slice::from_raw_parts(data, size);
    let mut cut = vec![size; n + 1];
    cut[0] = from;
    for t in 1..n {
        let c = cut[t - 1].max(from + (size - from) * t / n);
        cut[t] = bytes[c..].windows(delim.len()).position(|w| w == delim).map_or(size, |h| c + h + delim.len());
    }
    cut
}

fn put_money(out: &mut String, cents: i64) {
    let a = cents.unsigned_abs();
    out.push_str(if cents < 0 { " -" } else { " " });
    out.push_str(&format!("{}.{}{}", a / 100, a % 100 / 10, a % 10));
}

/// What the countries decide, known once the region codes are flagged.
struct Numbered {
    codes: Vec<u32>, // country index - 1 -> code
    index_of: Vec<u8>,
    seen: NamesSeen,
    totals: *mut Totals, // 256 per thread, after the bitsets in one block
    block: *mut u8,
    block_bytes: usize,
}

struct Ctx {
    nthreads: usize,
    regions: *const u8,
    records: *const u8,
    size: usize,
    nregions: usize,
    country_of: *mut u8,
    rcut: Vec<usize>,
    cut: Vec<usize>,
    present: Vec<AtomicU8>,
    names: NameTable,
    numbered: OnceLock<Numbered>,
    flag: Phase,
    fill: Phase,
    parse: Phase,
}
unsafe impl Sync for Ctx {}
unsafe impl Sync for Numbered {}
unsafe impl Send for Numbered {}

unsafe fn worker(cx: &Ctx, t: usize) {
    let nt = cx.nthreads;
    cx.flag.run(|k| {
        populate_share(cx.country_of, cx.nregions, k, nt, 4096);
        populate_share(cx.names.0 as *mut u8, SLOTS as usize * 4, k, nt, 4096);
        let mut p = cx.regions.add(cx.rcut[k]);
        while p < cx.regions.add(cx.rcut[k + 1]) {
            cx.present[region_line(&mut p).1 as usize].store(1, Relaxed);
        }
    });
    let num = cx.numbered.get_or_init(|| {
        let mut codes = Vec::new();
        let mut index_of = vec![0u8; 1 << 15];
        for (k, f) in cx.present.iter().enumerate() {
            if f.load(Relaxed) != 0 {
                codes.push(k as u32);
                index_of[k] = codes.len() as u8;
            }
        }
        if codes.len() > 255 {
            die("more than 255 countries");
        }
        // On 2 MB pages: for 240 countries the bitsets and totals fill two of
        // them almost exactly (4,048 of 4,096 KB). A record's mark lands at
        // random in 3.9 MB, and on 4 KB pages nearly every one missed the
        // first-level TLB.
        let rows = (codes.len() + 1) * WORDS;
        let block_bytes = rows * 8 + nt * 256 * std::mem::size_of::<Totals>();
        let block = zeroed(block_bytes, true);
        Numbered {
            codes,
            index_of,
            seen: NamesSeen(block as *const AtomicU64),
            totals: block.add(rows * 8) as *mut Totals,
            block,
            block_bytes,
        }
    });
    cx.fill.run(|k| {
        populate_share(num.block, num.block_bytes, k, nt, HUGE_PAGE);
        let mut p = cx.regions.add(cx.rcut[k]);
        while p < cx.regions.add(cx.rcut[k + 1]) {
            let (id, c) = region_line(&mut p);
            *cx.country_of.add(id as usize) = num.index_of[c as usize];
        }
    });
    let mine = num.totals.add(t * 256);
    for c in 0..256 {
        mine.add(c).write(Totals::NEW);
    }
    cx.parse.run(|k| {
        // Map the piece's pages in one call rather than by a fault every 16
        // pages as the parse reaches them.
        let from = (cx.records as usize + cx.cut[k]) & !4095;
        madvise(from as *mut c_void, cx.records as usize + cx.cut[k + 1] - from, MADV_POPULATE_READ);
        parse_records(
            cx.records.add(cx.cut[k]),
            cx.records.add(cx.cut[k + 1]),
            cx.records.add(cx.size),
            cx.country_of,
            cx.nregions,
            cx.names,
            num.seen,
            mine,
        );
        // The piece is not read again: its pages are unmapped now, by this
        // thread, instead of all of the file's at exit by one.
        let to = (cx.records as usize + cx.cut[k + 1]) & !4095;
        if to > from {
            madvise(from as *mut c_void, to - from, MADV_DONTNEED);
        }
    });
}

fn main() {
    let data = std::env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = std::env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());
    let nthreads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 12);
    let (regions, rsize) = map_file(&format!("{data}/regions.csv"));
    let (records, size) = map_file(&format!("{data}/records.csv"));
    let rtext = unsafe { std::slice::from_raw_parts(regions, rsize) };
    let rtail = rtext[..rsize.saturating_sub(1)].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    // regions.csv: a header, then "id,ABC\n" lines in ascending id order, so
    // the last line holds the largest id.
    let nregions = if rsize > 0 && rtail > 0 {
        rtext[rtail..].iter().take_while(|b| b.is_ascii_digit()).fold(0usize, |a, &b| a * 10 + (b - b'0') as usize) + 1
    } else {
        1
    };
    let hdr = rtext.iter().position(|&b| b == b'\n').map_or(rsize, |i| i + 1);
    // records.csv: skip the BOM and header, then split after CRLFs into eight
    // pieces per thread, claimed in turn, so a thread whose pieces are slow
    // (more first sightings of names) is evened out by the others.
    let start = if size > 0 {
        unsafe { std::slice::from_raw_parts(records, size) }.iter().position(|&b| b == b'\n').map_or(size, |i| i + 1)
    } else {
        0
    };
    let pieces = 8 * nthreads;
    let cx = unsafe {
        Ctx {
            nthreads,
            regions,
            records,
            size,
            nregions,
            country_of: zeroed(nregions, false),
            rcut: split(regions, hdr, rsize, nthreads, b"\n"),
            cut: split(records, start, size, pieces, b"\r\n"),
            present: (0..1 << 15).map(|_| AtomicU8::new(0)).collect(),
            names: NameTable(zeroed(SLOTS as usize * 4, false) as *const AtomicU32),
            numbered: OnceLock::new(),
            flag: Phase::new(nthreads),
            fill: Phase::new(nthreads),
            parse: Phase::new(pieces),
        }
    };
    std::thread::scope(|s| {
        for t in 0..nthreads {
            let cx = &cx;
            // Small stacks: an 8 MB one could be backed by a whole 2 MB page
            // once touched, with transparent huge pages always on.
            std::thread::Builder::new().stack_size(256 << 10).spawn_scoped(s, move || unsafe { worker(cx, t) }).unwrap();
        }
    });

    let num = cx.numbered.get().unwrap();
    let mut out = String::new();
    let ncodes = num.codes.len();
    for i in 1..=ncodes + 1 {
        let c = i % (ncodes + 1); // countries 1..N, then _UNMATCHED (0)
        let mut sum = Totals::NEW;
        for t in 0..nthreads {
            let x = unsafe { *num.totals.add(t * 256 + c) };
            sum.count += x.count;
            sum.id_sum += x.id_sum;
            sum.name_bytes += x.name_bytes;
            sum.note_bytes += x.note_bytes;
            sum.distinct += x.distinct;
            sum.amount_sum += x.amount_sum;
            sum.amount_min = sum.amount_min.min(x.amount_min);
            sum.amount_max = sum.amount_max.max(x.amount_max);
        }
        if sum.count == 0 {
            continue;
        }
        if c == 0 {
            out.push_str("_UNMATCHED");
        } else {
            let k = num.codes[c - 1];
            out.extend([(b'A' + (k >> 10) as u8) as char, (b'A' + (k >> 5 & 31) as u8) as char, (b'A' + (k & 31) as u8) as char]);
        }
        out.push_str(&format!(" {} {}", sum.count, sum.id_sum));
        for v in [sum.amount_sum, sum.amount_min, sum.amount_max] {
            put_money(&mut out, v);
        }
        out.push_str(&format!(" {} {} {}\n", sum.distinct, sum.name_bytes, sum.note_bytes));
    }
    std::fs::write(format!("{outd}/aggregates.txt"), out).unwrap_or_else(|_| die("aggregates.txt"));
    std::process::exit(0);
}
