//! Track A (Ingest).
//!
//! One pool of threads does everything in three phases.
//!
//! Phase 1, parse. The log is memory-mapped and cut into line-aligned pieces
//! that any thread claims. The timestamp is always 27 bytes and the span is
//! fixed at 72 hours, so a line's hour index comes from its day and hour
//! digits alone. One 64-byte scan after the timestamp finds the method's end,
//! the endpoint's end and the line's end. Endpoints are interned in one
//! lock-free table shared by all threads, and each line files a 4-byte record
//! (latency, endpoint id, 5xx flag) under its hour.
//!
//! Between the phases one thread sorts the names, so each endpoint id gets its
//! rank in output order.
//!
//! Phase 2, hours. Threads claim hours in order. An hour's latencies are
//! counting-sorted by endpoint rank into per-thread scratch and three
//! selections per group give the percentiles. Each group's numbers are kept
//! along with the exact size the hour's text will take, since a number's width
//! is known without formatting it.
//!
//! Phase 3, text. One thread turns the sizes into offsets, sizes the output
//! file and maps it; then every thread formats its hours straight into the
//! file, so no hour waits for another and no write call queues on the file's
//! lock, and the output's pages fill only after the records are released.
use std::arch::x86_64::*;
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::{Barrier, Mutex, OnceLock};

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
// which is the MAP_FIXED behaviour the log mapping relies on.
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

const HOURS: usize = 72; // 2026-10-30T18:00Z to 2026-11-02T17:59:59Z
                         // A record packs the latency above a 13-bit key: a 12-bit endpoint id and the
                         // 5xx flag. Latencies of 2^19 us or more do not fit and are kept on a side
                         // list with their key instead.
const KEY_BITS: u32 = 13;
const ERR: u32 = 1 << 12;
const ID_MASK: u32 = ERR - 1;
const PACKED_MAX: u32 = 1 << 19;

fn die(msg: &str) -> ! {
    eprintln!("[track-a] {msg}");
    std::process::exit(1)
}

/// Bit i is set when byte i of the 32 at p equals c.
#[inline(always)]
unsafe fn eq32(p: *const u8, c: u8) -> u32 {
    let v = _mm256_loadu_si256(p as *const __m256i);
    _mm256_movemask_epi8(_mm256_cmpeq_epi8(v, _mm256_set1_epi8(c as i8))) as u32
}

/// The number of decimal digits in x (about bit length x log10(2), corrected by one compare).
#[inline(always)]
fn width(x: u32) -> u32 {
    const P10: [u32; 10] = [
        1, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000, 1000000000,
    ];
    let t = ((32 - (x | 1).leading_zeros()) * 1233) >> 12;
    t + ((x | 1) >= P10[t as usize]) as u32
}

/// The value of the len (1 to 10) digits ending just before end, with no loop:
/// the 16 bytes before end, minus '0' and with the bytes ahead of the number
/// zeroed, are combined 2, 4 and then 8 digits at a time.
#[inline(always)]
unsafe fn decode(end: *const u8, len: u32) -> u32 {
    let mut v = _mm_subs_epu8(
        _mm_loadu_si128(end.sub(16) as *const __m128i),
        _mm_set1_epi8(b'0' as i8),
    );
    let lane = _mm_setr_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
    v = _mm_and_si128(v, _mm_cmpgt_epi8(lane, _mm_set1_epi8((15 - len) as i8)));
    v = _mm_maddubs_epi16(v, _mm_set1_epi16(0x010A)); // pairs (10, 1)
    v = _mm_madd_epi16(v, _mm_set1_epi32(0x0001_0064)); // pairs (100, 1)
    v = _mm_madd_epi16(_mm_packus_epi32(v, v), _mm_set1_epi32(0x0001_2710));
    (_mm_cvtsi128_si32(v) as u32 as u64 * 100_000_000 + _mm_extract_epi32::<1>(v) as u32 as u64)
        as u32
}

/// Puts the k-th smallest of a at a[k], the smaller ones before it, without a
/// branch on the data: each step partitions around the median of three by
/// Lomuto's scheme with an unconditional swap, the boundary moving by the
/// comparison's result. Many equal values could make a round shrink the range
/// by only one, so after 40 rounds the library's selection finishes.
fn select(mut a: &mut [u32], mut k: usize) {
    for round in 0.. {
        let n = a.len();
        if n < 2 {
            return;
        }
        if round == 40 {
            a.select_nth_unstable(k);
            return;
        }
        let m = n / 2; // sort a[0], a[m], a[n - 1]; the median is the pivot
        if a[m] < a[0] {
            a.swap(m, 0);
        }
        if a[n - 1] < a[m] {
            a.swap(n - 1, m);
        }
        if a[m] < a[0] {
            a.swap(m, 0);
        }
        a.swap(m, n - 1);
        let (pivot, mut lt) = (a[n - 1], 0);
        for i in 0..n - 1 {
            let x = a[i];
            a[i] = a[lt];
            a[lt] = x;
            lt += (x < pivot) as usize;
        }
        a.swap(lt, n - 1);
        if k == lt {
            return;
        }
        if k < lt {
            a = &mut a[..lt];
        } else {
            k -= lt + 1;
            a = &mut a[lt + 1..];
        }
    }
}

/// The first c at or after s; one is known to follow.
#[inline(always)]
unsafe fn next_byte(mut s: *const u8, c: u8) -> *const u8 {
    loop {
        let m = eq32(s, c);
        if m != 0 {
            return s.add(m.trailing_zeros() as usize);
        }
        s = s.add(32);
    }
}

/// Endpoint names to ids, one table for all threads. Open addressing on a CRC
/// of the name's first and last eight bytes (masked when the name is shorter;
/// the rest of the line keeps both loads in bounds), and a name's id is its
/// slot. A new name is copied into the arena, so the names compared on every
/// line sit together, and then published with one compare-and-swap; a thread
/// that loses the race for the slot compares against the winner's name.
const SLOTS: usize = 1 << 12; // a seed has 1,842 endpoints
const ARENA: usize = SLOTS * 128;

struct Names {
    slot: Vec<AtomicU64>, // length << 32 | arena offset, 0 if free
    used: AtomicU32,
    arena: *mut u8, // 64 spare bytes keep the last compare in bounds
}

unsafe impl Sync for Names {}

impl Names {
    fn new() -> Self {
        let arena = unsafe { mmap(std::ptr::null_mut(), ARENA + 4096, 3, 0x22, -1, 0) };
        if arena as isize == -1 {
            die("cannot map the name arena");
        }
        Names {
            slot: (0..SLOTS).map(|_| AtomicU64::new(0)).collect(),
            used: AtomicU32::new(0),
            arena: arena as *mut u8,
        }
    }

    fn name(&self, v: u64) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.arena.add(v as u32 as usize), (v >> 32) as usize) }
    }

    /// The id of the `len` bytes at s (1 to 125, 64 more readable after them).
    #[inline(always)]
    unsafe fn id(&self, s: *const u8, len: usize) -> u32 {
        let a = (s as *const u64).read_unaligned();
        let (a, b) = if len >= 8 {
            (a, (s.add(len - 8) as *const u64).read_unaligned())
        } else {
            (a & ((1u64 << (8 * len)) - 1), 0)
        };
        let h = _mm_crc32_u64(_mm_crc32_u64(len as u64, a), b) as u32 as usize;
        for i in h..h + SLOTS {
            let at = i % SLOTS;
            let mut v = self.slot[at].load(Acquire);
            if v == 0 {
                let off = self.used.fetch_add(len as u32, Relaxed) as usize;
                if off + len > ARENA {
                    die("name arena full");
                }
                std::ptr::copy_nonoverlapping(s, self.arena.add(off), len);
                match self.slot[at].compare_exchange(
                    0,
                    (len as u64) << 32 | off as u64,
                    AcqRel,
                    Acquire,
                ) {
                    Ok(_) => return at as u32,
                    Err(cur) => v = cur,
                }
            }
            if self.same(v, s, len) {
                return at as u32;
            }
        }
        die("more names than slots")
    }

    /// 64 bytes can always be read from either name (the rest of the line and
    /// the spare page follow one, the arena's spare bytes the other), so a name
    /// of up to 64 bytes (94% of lines) is compared in two steps, not memcmp.
    #[inline(always)]
    unsafe fn same(&self, v: u64, s: *const u8, len: usize) -> bool {
        if (v >> 32) as usize != len {
            return false;
        }
        let x = self.arena.add(v as u32 as usize);
        if len > 64 {
            return std::slice::from_raw_parts(x, len) == std::slice::from_raw_parts(s, len);
        }
        let eq = |i: usize| {
            let (p, q) = (
                _mm256_loadu_si256(x.add(i) as *const __m256i),
                _mm256_loadu_si256(s.add(i) as *const __m256i),
            );
            _mm256_movemask_epi8(_mm256_cmpeq_epi8(p, q)) as u32 as u64
        };
        !(eq(0) | eq(32) << 32) & (!0u64 >> (64 - len)) == 0
    }
}

/// One hour's records from one thread, in a chain of 16 KB chunks carved in
/// order from a region the thread maps up front (one mmap, so no lock is
/// shared). The output file's pages count toward peak memory as it is written,
/// so chunks are handed back while it grows: see `Parser::release`.
const CAP: usize = (16384 - 16) / 4;

#[repr(C)]
struct Chunk {
    prev: *mut Chunk,
    n: u32,
    hour: u32,
    rec: [u32; CAP],
}

struct Parser {
    last: [*mut Chunk; HOURS],
    wide: Vec<Vec<(u32, u32)>>, // (latency, key) of the long ones
    next: *mut Chunk,           // where the next chunk is carved
    kept: AtomicUsize,          // first chunk not yet released
}

unsafe impl Send for Parser {}
unsafe impl Sync for Parser {}

impl Parser {
    /// Room for a log of `size` bytes: a line is at least 40 bytes, and each
    /// hour may leave one chunk part full. Pages are charged only once written.
    fn new(size: usize) -> Self {
        let bytes = (size / 40 / CAP + 1 + HOURS) * std::mem::size_of::<Chunk>();
        let region = unsafe { mmap(std::ptr::null_mut(), bytes, 3, 0x22, -1, 0) } as *mut Chunk;
        if region as isize == -1 {
            die("cannot map the record chunks");
        }
        Parser {
            last: [std::ptr::null_mut(); HOURS],
            wide: vec![Vec::new(); HOURS],
            next: region,
            kept: AtomicUsize::new(region as usize),
        }
    }

    unsafe fn run(&mut self, mut p: *const u8, end: *const u8, names: &Names) {
        while p < end {
            // The log's pages are scattered in memory and the hardware prefetcher
            // stops at each 4 KB page; this reaches into the next one in time.
            _mm_prefetch::<_MM_HINT_T0>(p.wrapping_add(2048) as *const i8);
            // YYYY-MM-DDTHH: month's last digit at 6, day at 8-9, hour at 11-12.
            let digit = |i: usize| (*p.add(i) - b'0') as i32;
            let (day, hour) = (digit(8) * 10 + digit(9), digit(11) * 10 + digit(12));
            let h =
                ((if *p.add(6) == b'0' { day - 30 } else { day + 1 }) * 24 + hour - 18) as usize;

            // Spaces and newlines in the 64 bytes after the timestamp. Only an
            // endpoint near its 125-byte maximum pushes past the window.
            let m = p.add(28);
            let sp = eq32(m, b' ') as u64 | (eq32(m.add(32), b' ') as u64) << 32;
            let nl = eq32(m, b'\n') as u64 | (eq32(m.add(32), b'\n') as u64) << 32;
            let name = m.add(sp.trailing_zeros() as usize + 1);
            let name_end = if nl != 0 {
                m.add((sp & (sp - 1)).trailing_zeros() as usize)
            } else {
                next_byte(name, b' ')
            };
            let eol = if nl != 0 {
                m.add(nl.trailing_zeros() as usize)
            } else {
                next_byte(name_end.add(5), b'\n')
            };
            let key = names.id(name, name_end.offset_from(name) as usize)
                | if *name_end.add(1) == b'5' { ERR } else { 0 };
            let latency = decode(eol, eol.offset_from(name_end) as u32 - 5);
            p = eol.add(1);

            if latency >= PACKED_MAX {
                self.wide[h].push((latency, key));
                continue;
            }
            let mut c = self.last[h];
            if c.is_null() || (*c).n as usize == CAP {
                let nx = self.next;
                (*nx).prev = c;
                (*nx).n = 0;
                (*nx).hour = h as u32;
                (self.last[h], self.next, c) = (nx, nx.add(1), nx);
            }
            (*c).rec[(*c).n as usize] = latency << KEY_BITS | key;
            (*c).n += 1;
        }
    }

    /// The log is in arrival order, so chunks are carved roughly hour by hour
    /// and hours finish in the same order: the run of chunks at the front whose
    /// hours are all finished grows steadily. Hand it back once it reaches 64
    /// KB, in one madvise, instead of one call (and TLB shootdown) per chunk.
    fn release(&self, finished: &[AtomicBool]) {
        let kept = self.kept.load(Relaxed) as *mut Chunk;
        let mut c = kept;
        unsafe {
            while c < self.next && finished[(*c).hour as usize].load(Acquire) {
                c = c.add(1);
            }
            if c.offset_from(kept) >= 4 || (c == self.next && c > kept) {
                madvise(
                    kept as *mut c_void,
                    c.offset_from(kept) as usize * std::mem::size_of::<Chunk>(),
                    4,
                ); // MADV_DONTNEED
                self.kept.store(c as usize, Relaxed);
            }
        }
    }

    /// Calls f(endpoint rank, key, latency) for every record of hour h.
    fn each(&self, h: usize, rank: &[u32], mut f: impl FnMut(usize, u32, u32)) {
        let mut c = self.last[h];
        while !c.is_null() {
            let chunk = unsafe { &*c };
            for &r in &chunk.rec[..chunk.n as usize] {
                f(
                    rank[(r & ID_MASK) as usize] as usize,
                    r & ERR,
                    r >> KEY_BITS,
                );
            }
            c = chunk.prev;
        }
        for &(latency, key) in &self.wide[h] {
            f(rank[(key & ID_MASK) as usize] as usize, key & ERR, latency);
        }
    }

    fn count(&self, h: usize) -> usize {
        let mut n = self.wide[h].len();
        let mut c = self.last[h];
        while !c.is_null() {
            n += unsafe { (*c).n } as usize;
            c = unsafe { (*c).prev };
        }
        n
    }
}

/// Everything the phases after the parse share.
struct Ranked<'a> {
    parsers: Vec<Parser>,
    names: Vec<&'a [u8]>, // by rank
    rank: Vec<u32>,       // endpoint id -> rank
    most: usize,          // records in the largest hour
}

struct Line {
    e: u32,
    n: u32,
    p: [u32; 3],
    errors: u32,
}

impl Line {
    fn numbers(&self) -> [u32; 6] {
        [
            self.n,
            self.p[0],
            self.p[1],
            self.p[2],
            self.errors,
            (self.errors as u64 * 1_000_000 / self.n as u64) as u32,
        ]
    }
}

/// Runs f on one thread while the rest wait, so what it sets up is there for all.
fn once(b: &Barrier, f: impl FnOnce()) {
    if b.wait().is_leader() {
        f();
    }
    b.wait();
}

fn main() {
    // With transparent huge pages, a chunk region written only in part could be
    // backed by whole 2 MB pages and count against peak memory.
    unsafe { prctl(41, 1, 0, 0, 0) }; // PR_SET_THP_DISABLE
    let nt = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, 12);
    let data = std::env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = std::env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());
    let input = std::fs::File::open(format!("{data}/requests.log"))
        .unwrap_or_else(|e| die(&format!("cannot open the log: {e}")));
    let out = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(format!("{outd}/summary.txt"));
    let out = out.unwrap_or_else(|e| die(&format!("cannot create summary.txt: {e}")));

    // Map the log over one spare zero page, so the 64-byte scans stay in bounds.
    let size = input.metadata().map_or(0, |m| m.len() as usize);
    #[cfg(target_os = "linux")]
    let log = unsafe { mmap(std::ptr::null_mut(), size + 4096, 1, 0x22, -1, 0) };
    #[cfg(target_os = "linux")]
    if size > 0 && unsafe { mmap(log, size, 1, 0x12, raw_fd(&input), 0) } as isize == -1 {
        die("cannot map the log");
    }
    // Off Linux there is no mmap: read the whole log with a spare page and leak it, so the offline
    // corpora can still be checked here. The scoring box is Linux and takes the mapping above.
    #[cfg(not(target_os = "linux"))]
    let log = {
        let mut v: Vec<u8> = std::fs::read(format!("{data}/requests.log")).unwrap_or_default();
        v.resize(size + 4096, 0);
        Box::leak(v.into_boxed_slice()).as_mut_ptr() as *mut c_void
    };
    let (log, bytes) = (log as usize, unsafe {
        std::slice::from_raw_parts(log as *const u8, size)
    });
    // Line-aligned pieces, eight per thread, claimed in order: a thread that
    // starts late or shares its core takes fewer.
    let pieces = 8 * nt;
    let mut cut = vec![size; pieces + 1];
    cut[0] = 0;
    for k in 1..pieces {
        cut[k] = cut[k - 1].max(size * k / pieces);
        while cut[k] < size && bytes[cut[k] - 1] != b'\n' {
            cut[k] += 1;
        }
    }

    let names = Names::new();
    let (parsed, shared) = (Mutex::new(Vec::new()), OnceLock::new());
    let (ranked, sized) = (Barrier::new(nt), Barrier::new(nt));
    let finished: Vec<AtomicBool> = (0..HOURS).map(|_| AtomicBool::new(false)).collect();
    let releasing = AtomicBool::new(false);
    let (claim_piece, claim, claim_text) = (
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    );
    let lines: Vec<Mutex<Vec<Line>>> = (0..HOURS).map(|_| Mutex::new(Vec::new())).collect();
    let at: Vec<AtomicU64> = (0..=HOURS).map(|_| AtomicU64::new(0)).collect(); // an hour's text size, then its offset
    let text = OnceLock::new();

    std::thread::scope(|s| {
        for _ in 0..nt {
            s.spawn(|| {
                let mut parser = Parser::new(size);
                loop {
                    let k = claim_piece.fetch_add(1, Relaxed);
                    if k >= pieces {
                        break;
                    }
                    unsafe {
                        parser.run(
                            (log + cut[k]) as *const u8,
                            (log + cut[k + 1]) as *const u8,
                            &names,
                        )
                    };
                }
                parsed.lock().unwrap().push(parser);

                // Between the phases, endpoints are ranked in unsigned byte order
                // (a proper prefix first), which is the order lines are printed in
                // an hour.
                once(&ranked, || {
                    let parsers = std::mem::take(&mut *parsed.lock().unwrap());
                    let most = (0..HOURS)
                        .map(|h| parsers.iter().map(|p| p.count(h)).sum())
                        .max()
                        .unwrap_or(0);
                    let mut by_name: Vec<(&[u8], usize)> = (0..SLOTS)
                        .filter_map(|id| match names.slot[id].load(Relaxed) {
                            0 => None,
                            v => Some((names.name(v), id)),
                        })
                        .collect();
                    by_name.sort_unstable();
                    let mut rank = vec![0u32; SLOTS];
                    for (r, &(_, id)) in by_name.iter().enumerate() {
                        rank[id] = r as u32;
                    }
                    let names = by_name.iter().map(|&(name, _)| name).collect();
                    let _ = shared.set(Ranked {
                        parsers,
                        names,
                        rank,
                        most,
                    });
                });
                let sh = shared.get().unwrap();

                {
                    // The scratch goes before the text phase, where the output counts.
                    let ne = sh.names.len();
                    // Sized once: a buffer that grows is remapped, which stalls every
                    // thread's page faults. Not zeroed: only the pages used count.
                    let mut latency: Vec<u32> = Vec::with_capacity(sh.most);
                    let (mut start, mut errors) = (vec![0u32; ne + 1], vec![0u32; ne]);
                    loop {
                        let h = claim.fetch_add(1, Relaxed);
                        if h >= HOURS {
                            break;
                        }
                        // Counting sort of the hour's latencies by endpoint rank: count
                        // into start[e + 1], prefix, then place each latency at its
                        // group's cursor.
                        start.fill(0);
                        errors.fill(0);
                        for ps in &sh.parsers {
                            ps.each(h, &sh.rank, |e, err, _| {
                                start[e + 1] += 1;
                                errors[e] += (err != 0) as u32;
                            });
                        }
                        for e in 0..ne {
                            start[e + 1] += start[e];
                        }
                        let mut cursor = start[..ne].to_vec();
                        let slots = latency.as_mut_ptr();
                        for ps in &sh.parsers {
                            ps.each(h, &sh.rank, |e, _, l| {
                                unsafe { *slots.add(cursor[e] as usize) = l };
                                cursor[e] += 1;
                            });
                        }
                        unsafe { latency.set_len(start[ne] as usize) };
                        // An hour is finished once its records are copied out; one
                        // thread at a time then releases whatever chunks that frees.
                        finished[h].store(true, Release);
                        if !releasing.swap(true, Acquire) {
                            sh.parsers.iter().for_each(|ps| ps.release(&finished));
                            releasing.store(false, Release);
                        }

                        let (mut size, mut out) = (0u64, Vec::new());
                        for e in 0..ne {
                            let (from, to) = (start[e] as usize, start[e + 1] as usize);
                            let n = to - from;
                            if n == 0 {
                                continue;
                            }
                            // Nearest rank p is index ceil(p * n / 100) - 1. A selection
                            // leaves only larger values after its rank, so each higher
                            // rank is selected among those. (An equal rank is already
                            // in place.)
                            let a = &mut latency[from..to];
                            let r = [
                                (50 * n + 99) / 100 - 1,
                                (95 * n + 99) / 100 - 1,
                                (99 * n + 99) / 100 - 1,
                            ];
                            let mut next = 0;
                            for &r in &r {
                                if r >= next {
                                    select(&mut a[next..], r - next);
                                    next = r + 1;
                                }
                            }
                            let l = Line {
                                e: e as u32,
                                n: n as u32,
                                p: r.map(|r| a[r]),
                                errors: errors[e],
                            };
                            // Label, name, six numbers each after a space, newline.
                            size += 21 + sh.names[e].len() as u64 + 7;
                            size += l.numbers().iter().map(|&x| width(x) as u64).sum::<u64>();
                            out.push(l);
                        }
                        at[h].store(size, Relaxed);
                        *lines[h].lock().unwrap() = out;
                    }
                }
                once(&sized, || {
                    // Every hour is finished: whatever chunks the last releases left go
                    // back now, before the output's pages start to count.
                    sh.parsers.iter().for_each(|ps| ps.release(&finished));
                    let mut total = 0;
                    for h in 0..HOURS {
                        total += at[h].swap(total, Relaxed);
                    }
                    at[HOURS].store(total, Relaxed);
                    let mut base = 0usize;
                    if total > 0 {
                        out.set_len(total)
                            .unwrap_or_else(|e| die(&format!("cannot size summary.txt: {e}")));
                        #[cfg(target_os = "linux")]
                        {
                            base = unsafe {
                                mmap(
                                    std::ptr::null_mut(),
                                    total as usize,
                                    3,
                                    1,
                                    raw_fd(&out),
                                    0,
                                )
                            } as usize;
                            if base as isize == -1 {
                                die("cannot map summary.txt");
                            }
                        }
                        // Off Linux, format into a heap buffer that is written at the end.
                        #[cfg(not(target_os = "linux"))]
                        {
                            let v: Vec<u8> = vec![0u8; total as usize];
                            base = Box::leak(v.into_boxed_slice()).as_mut_ptr() as usize;
                        }
                    }
                    let _ = text.set(base);
                });

                let base = *text.get().unwrap() as *mut u8;
                loop {
                    let h = claim_text.fetch_add(1, Relaxed);
                    if h >= HOURS {
                        break;
                    }
                    let day = (h + 18) / 24;
                    let label = format!(
                        "2026-{:02}-{:02}T{:02}:00:00Z ",
                        if day < 2 { 10 } else { 11 },
                        if day < 2 { 30 + day } else { day - 1 },
                        (h + 18) % 24
                    );
                    let mut o = unsafe { base.add(at[h].load(Relaxed) as usize) };
                    for l in std::mem::take(&mut *lines[h].lock().unwrap()) {
                        let name = sh.names[l.e as usize];
                        unsafe {
                            std::ptr::copy_nonoverlapping(label.as_ptr(), o, 21);
                            std::ptr::copy_nonoverlapping(name.as_ptr(), o.add(21), name.len());
                            o = o.add(21 + name.len());
                            for mut x in l.numbers() {
                                // One width() per number, not two: this phase formats about 800,000
                                // numbers across 72 hours, and width is a leading-zero count, a
                                // multiply and a table compare. The digits loop then walks exactly
                                // the w bytes the advance reserved.
                                let w = width(x) as usize;
                                *o = b' ';
                                o = o.add(1 + w);
                                for i in 1..=w {
                                    *o.sub(i) = b'0' + (x % 10) as u8;
                                    x /= 10;
                                }
                            }
                            *o = b'\n';
                            o = o.add(1);
                        }
                    }
                }
            });
        }
    });

    // Off Linux the mapped output was a heap buffer; write what was formatted into it.
    #[cfg(not(target_os = "linux"))]
    {
        let base = *text.get().unwrap() as *const u8;
        let total = at[HOURS].load(Relaxed) as usize;
        if total > 0 {
            let bytes = unsafe { std::slice::from_raw_parts(base, total) };
            std::fs::write(format!("{outd}/summary.txt"), bytes)
                .unwrap_or_else(|e| die(&format!("cannot write summary.txt: {e}")));
        }
    }
}
