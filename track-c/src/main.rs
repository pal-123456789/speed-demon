//! Track C — Join, in Rust with no dependencies.
//!
//! Both files are memory-mapped; every phase runs on one pool of threads.
//! 1. regions.csv is cut into line-aligned ranges and parsed in parallel into a
//!    flat `region id -> country` table (verified: ids strictly ascending and
//!    every line well formed, else an exact serial parse of the whole file).
//! 2. records.csv is cut at proven record boundaries: a LF seen after an even
//!    number of `"` bytes ends a record, because every record holds an even
//!    number of quotes and a LF inside a quoted field sits at odd parity. One
//!    parallel pass counts quotes per 256 KB chunk; the cuts follow from it.
//! 3. Each piece is read with 64-byte structural scans (quotes, commas, LFs,
//!    the quote mask by prefix XOR) and each record checked against the exact
//!    shape of the grammar; a record that is not that shape goes through the
//!    literal scalar parser, so the fast path can never change an answer.
//! 4. Names are interned by their raw bytes (equal raw bytes mean equal values)
//!    in one lock-free table whose entries point at the first occurrence in the
//!    file and are always confirmed byte for byte. Each country has a bitset
//!    over name ids. The region, name and bitset reads are prefetched records
//!    ahead. A name table that fills up, or a file too big for its packed
//!    entries, restarts the run on an exact locked map: slower, never wrong.
//! 5. The record parse and the quote-parity pass are compiled twice from the same source:
//!    once for the baseline x86-64 target and once with AVX2, BMI, POPCNT and PCLMULQDQ,
//!    chosen at run time. Both give identical results; only the instructions differ.
use std::arch::x86_64::*;
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "linux")]
use std::ffi::c_void;
use std::process::exit;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;

#[cfg(target_os = "linux")]
extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut c_void;
    fn madvise(addr: *mut c_void, len: usize, advice: i32) -> i32;
    fn prctl(op: i32, a: u64, b: u64, c: u64, d: u64) -> i32;
}

/// A file descriptor for the mapping call. Only the Linux mapping uses it.
#[cfg(target_os = "linux")]
fn raw_fd(f: &std::fs::File) -> i32 {
    use std::os::fd::AsRawFd;
    f.as_raw_fd()
}

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

fn die(msg: &str) -> ! {
    eprintln!("[track-c] {msg}");
    exit(1)
}

/// The file over a spare zero page, so a 64-byte read at any byte stays mapped.
#[cfg(target_os = "linux")]
fn map(path: &str) -> &'static [u8] {
    let file = std::fs::File::open(path).unwrap_or_else(|e| die(&format!("cannot open {path}: {e}")));
    let len = file.metadata().unwrap().len() as usize;
    unsafe {
        let p = mmap(std::ptr::null_mut(), len + 4096, 1, 0x22, -1, 0);
        let ok = len == 0 || mmap(p, len, 1, 0x12, raw_fd(&file), 0) == p;
        if p as isize == -1 || !ok {
            die(&format!("cannot map {path}"));
        }
        std::slice::from_raw_parts(p as *const u8, len)
    }
}

/// The same bytes on a target with no `mmap`, so the offline corpora can still be
/// checked on a development machine. The scoring box is Linux and takes the path
/// above; this exists only so a local verification build links.
#[cfg(not(target_os = "linux"))]
fn map(path: &str) -> &'static [u8] {
    let v = std::fs::read(path).unwrap_or_else(|e| die(&format!("cannot read {path}: {e}")));
    Box::leak(v.into_boxed_slice())
}

/// Lazily zeroed shared arrays (the allocator hands out untouched zero pages).
fn zeroed_u64(n: usize) -> Vec<AtomicU64> {
    let mut v = std::mem::ManuallyDrop::new(vec![0u64; n]);
    unsafe { Vec::from_raw_parts(v.as_mut_ptr() as *mut AtomicU64, v.len(), v.capacity()) }
}
fn zeroed_u16(n: usize) -> Vec<AtomicU16> {
    let mut v = std::mem::ManuallyDrop::new(vec![0u16; n]);
    unsafe { Vec::from_raw_parts(v.as_mut_ptr() as *mut AtomicU16, v.len(), v.capacity()) }
}

/// Thread t of nt faults in its share of the whole pages of v up front.
#[cfg(target_os = "linux")]
fn populate<T>(v: &[T], t: usize, nt: usize) {
    let (base, bytes) = (v.as_ptr() as usize, std::mem::size_of_val(v));
    let (first, last) = ((base + 4095) / 4096, (base + bytes) / 4096);
    if last > first {
        let n = last - first;
        let (a, b) = (first + n * t / nt, first + n * (t + 1) / nt);
        if b > a {
            unsafe { madvise((a * 4096) as *mut c_void, (b - a) * 4096, 23) }; // MADV_POPULATE_WRITE
        }
    }
}

#[inline(always)]
fn prefetch<T>(p: *const T) {
    unsafe { _mm_prefetch::<_MM_HINT_T0>(p as *const i8) }
}

/// Off Linux the pages are faulted in by the threads that touch them, which is
/// what happens on Linux too when `MADV_POPULATE_WRITE` is unavailable.
#[cfg(not(target_os = "linux"))]
fn populate<T>(_v: &[T], _t: usize, _nt: usize) {}

#[inline(always)]
fn load8(f: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(f[i..i + 8].try_into().unwrap())
}

/// The value of the n (1 to 8) digit bytes at the top of w (last digit in byte 7).
#[inline(always)]
fn fold(v: u64, n: usize) -> u64 {
    let mut v = v & 0x0F0F_0F0F_0F0F_0F0F & (!0u64 << (8 * (8 - n)));
    v = (v * 10 + (v >> 8)) & 0x00FF_00FF_00FF_00FF;
    v = (v * 100 + (v >> 16)) & 0x0000_FFFF_0000_FFFF;
    (v * 10000 + (v >> 32)) & 0xFFFF_FFFF
}

/// Are the n (1 to 8) top bytes of w all ASCII digits?
#[inline(always)]
fn digits_ok(w: u64, n: usize) -> bool {
    let keep = !0u64 << (8 * (8 - n));
    let w = (w & keep) | (0x3030_3030_3030_3030 & !keep);
    w & 0xF0F0_F0F0_F0F0_F0F0 == 0x3030_3030_3030_3030
        && w.wrapping_add(0x0606_0606_0606_0606) & 0xF0F0_F0F0_F0F0_F0F0 == 0x3030_3030_3030_3030
}

/// A fast hash of a name. Only speed depends on it: every match is confirmed byte for byte.
#[inline(always)]
fn hash(b: &[u8]) -> u64 {
    const M: u64 = 0x9E37_79B9_7F4A_7C15;
    const M2: u64 = 0x2545_F491_4F6C_DD1D;
    let n = b.len();
    let h = if n >= 4 {
        let w4 = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as u64;
        let w8 = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let (a, c) = if n >= 8 { (w8(0), w8(n - 8)) } else { (w4(0), w4(n - 4)) };
        let mut h = (a ^ M).wrapping_mul(M) ^ (c ^ n as u64).wrapping_mul(M2);
        if n >= 16 {
            h ^= w8(n / 2 - 4).wrapping_mul(M);
        }
        h ^ (h >> 31)
    } else {
        b.iter().fold(n as u64 | 0x100, |x, &y| (x << 8) | y as u64).wrapping_mul(M)
    };
    let h = h.wrapping_mul(M2);
    h ^ (h >> 31) ^ (h >> 29)
}

/// One country's running aggregate (per thread, merged at the end).
#[derive(Clone, Copy)]
struct Agg {
    count: u64,
    id_sum: i64,
    amount_sum: i64,
    amount_min: i64,
    amount_max: i64,
    name_bytes: u64,
    note_bytes: u64,
    distinct: u64, // names first marked for this country by this thread
}

impl Agg {
    fn new() -> Self {
        Agg { count: 0, id_sum: 0, amount_sum: 0, amount_min: i64::MAX, amount_max: i64::MIN, name_bytes: 0, note_bytes: 0, distinct: 0 }
    }
    fn merge(&mut self, o: &Agg) {
        if o.count == 0 {
            return;
        }
        self.count += o.count;
        self.id_sum += o.id_sum;
        self.amount_sum += o.amount_sum;
        self.amount_min = self.amount_min.min(o.amount_min);
        self.amount_max = self.amount_max.max(o.amount_max);
        self.name_bytes += o.name_bytes;
        self.note_bytes += o.note_bytes;
        self.distinct += o.distinct;
    }
}

/// Exact cents as `-?int.dd`, never `-0.00`.
fn money(cents: i64, out: &mut Vec<u8>) {
    let a = cents.unsigned_abs();
    out.extend_from_slice(format!("{}{}.{:02}", if cents < 0 { "-" } else { "" }, a / 100, a % 100).as_bytes());
}

fn line(out: &mut Vec<u8>, key: &[u8], a: &Agg) {
    out.extend_from_slice(key);
    out.extend_from_slice(format!(" {} {} ", a.count, a.id_sum).as_bytes());
    money(a.amount_sum, out);
    out.push(b' ');
    money(a.amount_min, out);
    out.push(b' ');
    money(a.amount_max, out);
    out.extend_from_slice(format!(" {} {} {}\n", a.distinct, a.name_bytes, a.note_bytes).as_bytes());
}

const STEP: usize = 1 << 18; // bytes per quote-parity chunk

/// The commas and LFs outside quotes, in order, from a start offset onwards,
/// each with the number of `"` bytes before it (counted from the start).
/// The file must be mapped with a zero page after it (see `map`).
struct Scan<'a, const A: bool> {
    f: &'a [u8],
    next: usize,
    bits: u64,
    carry: u64,
    q: u64,
    before: u64,
}

impl<'a, const A: bool> Scan<'a, A> {
    #[inline(always)]
    fn new(f: &'a [u8], p: usize) -> Self {
        Scan { f, next: p, bits: 0, carry: 0, q: 0, before: 0 }
    }

    /// The next separator, or (file length, ..) once there is none.
    #[inline(always)]
    fn next(&mut self) -> (usize, u64) {
        while self.bits == 0 {
            if self.next >= self.f.len() {
                return (self.f.len(), self.before + self.q.count_ones() as u64);
            }
            self.before += self.q.count_ones() as u64;
            let (q, cn, x);
            unsafe {
                let p = self.f.as_ptr().add(self.next);
                if A {
                    // Only reached on a CPU that has AVX2 and PCLMULQDQ (see `native`).
                    let (kq, kc, kn) = (_mm256_set1_epi8(b'"' as i8), _mm256_set1_epi8(b',' as i8), _mm256_set1_epi8(b'\n' as i8));
                    let (v0, v1) = (_mm256_loadu_si256(p as *const __m256i), _mm256_loadu_si256(p.add(32) as *const __m256i));
                    q = (_mm256_movemask_epi8(_mm256_cmpeq_epi8(v0, kq)) as u32 as u64) | (_mm256_movemask_epi8(_mm256_cmpeq_epi8(v1, kq)) as u32 as u64) << 32;
                    cn = (_mm256_movemask_epi8(_mm256_or_si256(_mm256_cmpeq_epi8(v0, kc), _mm256_cmpeq_epi8(v0, kn))) as u32 as u64)
                        | (_mm256_movemask_epi8(_mm256_or_si256(_mm256_cmpeq_epi8(v1, kc), _mm256_cmpeq_epi8(v1, kn))) as u32 as u64) << 32;
                    // Carry-less multiply by all ones is the prefix XOR of the quote bits.
                    x = _mm_cvtsi128_si64(_mm_clmulepi64_si128::<0>(_mm_cvtsi64_si128(q as i64), _mm_set1_epi8(-1))) as u64;
                } else {
                    let (mut qq, mut c, mut n) = (0u64, 0u64, 0u64);
                    let (kq, kc, kn) = (_mm_set1_epi8(b'"' as i8), _mm_set1_epi8(b',' as i8), _mm_set1_epi8(b'\n' as i8));
                    for k in 0..4 {
                        let v = _mm_loadu_si128(p.add(16 * k) as *const __m128i);
                        qq |= (_mm_movemask_epi8(_mm_cmpeq_epi8(v, kq)) as u32 as u64) << (16 * k);
                        c |= (_mm_movemask_epi8(_mm_cmpeq_epi8(v, kc)) as u32 as u64) << (16 * k);
                        n |= (_mm_movemask_epi8(_mm_cmpeq_epi8(v, kn)) as u32 as u64) << (16 * k);
                    }
                    q = qq;
                    cn = c | n;
                    // Bit i of the prefix XOR is set from an opening quote up to its closing quote;
                    // a "" pair flips it twice and so changes nothing.
                    let mut y = q;
                    y ^= y << 1;
                    y ^= y << 2;
                    y ^= y << 4;
                    y ^= y << 8;
                    y ^= y << 16;
                    y ^= y << 32;
                    x = y;
                }
            }
            let inside = x ^ self.carry;
            self.carry = ((inside as i64) >> 63) as u64;
            self.bits = cn & !inside;
            self.q = q;
            self.next += 64;
        }
        let i = self.bits.trailing_zeros();
        self.bits &= self.bits - 1;
        (self.next - 64 + i as usize, self.before + (self.q & ((1u64 << i) - 1)).count_ones() as u64)
    }
}

/// Offset just past the first LF at even quote parity at or after `from`
/// (`parity` = quotes in [0, from) mod 2), or the file length.
fn first_boundary(f: &[u8], from: usize, mut parity: u8) -> usize {
    for (i, &b) in f.iter().enumerate().skip(from) {
        if b == b'"' {
            parity ^= 1;
        } else if b == b'\n' && parity == 0 {
            return i + 1;
        }
    }
    f.len()
}

/// The quote parity of each STEP-byte chunk c0..c1.
#[inline(always)]
fn quote_parity_impl<const A: bool>(f: &[u8], c0: usize, c1: usize) -> Vec<u8> {
    (c0..c1)
        .map(|c| {
            let (a, b) = (c * STEP, ((c + 1) * STEP).min(f.len()));
            let (mut i, mut q) = (a, 0u32);
            if A {
                // Only reached on a CPU that has AVX2 (see `native`).
                unsafe {
                    let kq = _mm256_set1_epi8(b'"' as i8);
                    while i + 32 <= b {
                        let v = _mm256_loadu_si256(f.as_ptr().add(i) as *const __m256i);
                        q += (_mm256_movemask_epi8(_mm256_cmpeq_epi8(v, kq)) as u32).count_ones();
                        i += 32;
                    }
                }
            } else {
                let kq = unsafe { _mm_set1_epi8(b'"' as i8) };
                while i + 16 <= b {
                    let v = unsafe { _mm_loadu_si128(f.as_ptr().add(i) as *const __m128i) };
                    q += unsafe { _mm_movemask_epi8(_mm_cmpeq_epi8(v, kq)) as u32 }.count_ones();
                    i += 16;
                }
            }
            q += f[i..b].iter().filter(|&&x| x == b'"').count() as u32;
            (q & 1) as u8
        })
        .collect()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,bmi1,bmi2,popcnt,lzcnt,pclmulqdq,sse4.2")]
unsafe fn quote_parity_native(f: &[u8], c0: usize, c1: usize) -> Vec<u8> {
    quote_parity_impl::<true>(f, c0, c1)
}

fn quote_parity(f: &[u8], c0: usize, c1: usize, nat: bool) -> Vec<u8> {
    if nat {
        unsafe { quote_parity_native(f, c0, c1) }
    } else {
        quote_parity_impl::<false>(f, c0, c1)
    }
}

/// k line-aligned cuts of f[from..].
fn cuts(f: &[u8], from: usize, k: usize) -> Vec<usize> {
    let mut c = vec![f.len(); k + 1];
    c[0] = from;
    for t in 1..k {
        let mut x = (from + (f.len() - from) * t / k).max(c[t - 1]);
        while x < f.len() && f[x - 1] != b'\n' {
            x += 1;
        }
        c[t] = x;
    }
    c
}

/// Runs f(t) on nt threads and collects what each returns.
fn on_threads<R: Send>(nt: usize, f: impl Fn(usize) -> R + Sync) -> Vec<R> {
    let f = &f;
    std::thread::scope(|s| {
        let hs: Vec<_> = (0..nt).map(|t| s.spawn(move || f(t))).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

/// Country codes (3 bytes, packed) in a small lock-free table; a code's slot is its position.
struct CodeTab {
    t: Vec<AtomicU32>,
    n: AtomicUsize,
}

impl CodeTab {
    fn new() -> Self {
        CodeTab { t: (0..4096).map(|_| AtomicU32::new(0)).collect(), n: AtomicUsize::new(0) }
    }
    fn intern(&self, code: u32) -> u16 {
        let want = code | 1 << 24;
        let mut s = (code.wrapping_mul(0x9E37_79B1) >> 20) as usize;
        loop {
            let e = self.t[s].load(Relaxed);
            if e == want {
                return s as u16;
            }
            if e == 0 {
                match self.t[s].compare_exchange(0, want, Relaxed, Relaxed) {
                    Ok(_) => {
                        if self.n.fetch_add(1, Relaxed) >= 2048 {
                            die("too many countries");
                        }
                        return s as u16;
                    }
                    Err(c) if c == want => return s as u16,
                    Err(_) => {}
                }
            }
            s = (s + 1) & 4095;
        }
    }
}

struct RegOut {
    ok: bool,
    first: u64,
    last: u64,
    n: usize,
    over: Vec<(u64, u16)>, // ids past the flat table, in file order
}

/// Parse the region lines in f[a..b) strictly: `digits , XXX` per line. Any other shape sets !ok.
fn regions_part(f: &[u8], a: usize, b: usize, rarr: &[AtomicU16], codes: &CodeTab) -> RegOut {
    let mut o = RegOut { ok: true, first: 0, last: 0, n: 0, over: Vec::new() };
    let mut p = a;
    while p < b && o.ok {
        let eol = f[p..b].iter().position(|&c| c == b'\n').map_or(b, |k| p + k);
        let line = &f[p..eol];
        let line = if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { line };
        let nd = line.iter().take_while(|c| c.is_ascii_digit()).count();
        if nd == 0 || nd > 18 || line.len() != nd + 4 || line[nd] != b',' || line[nd + 1..].iter().any(|&c| c == b',' || c == b'\r' || c == b'\n') {
            o.ok = false;
            break;
        }
        let rid = line[..nd].iter().fold(0u64, |v, &d| v * 10 + (d - b'0') as u64);
        let code = (line[nd + 1] as u32) << 16 | (line[nd + 2] as u32) << 8 | line[nd + 3] as u32;
        let pos = codes.intern(code) + 1;
        if o.n == 0 {
            o.first = rid;
        } else if rid <= o.last {
            o.ok = false;
        }
        (o.last, o.n) = (rid, o.n + 1);
        match rarr.get(rid as usize) {
            Some(slot) => slot.store(pos, Relaxed),
            None => o.over.push((rid, pos)),
        }
        p = eol + 1;
    }
    o
}

/// The literal scalar parse of regions.csv (last line wins), used when the strict one declines.
fn regions_exact(f: &[u8], rarr: &[AtomicU16], codes: &CodeTab) -> Vec<(u64, u16)> {
    let mut over = Vec::new();
    let mut p = f.iter().position(|&c| c == b'\n').unwrap_or(f.len());
    while p < f.len() {
        p += 1;
        if p >= f.len() {
            break;
        }
        let mut rid = 0u64;
        while p < f.len() && f[p].is_ascii_digit() {
            rid = rid.wrapping_mul(10).wrapping_add((f[p] - b'0') as u64);
            p += 1;
        }
        if p < f.len() && f[p] == b',' {
            p += 1;
        }
        if p + 2 < f.len() {
            let code = (f[p] as u32) << 16 | (f[p + 1] as u32) << 8 | f[p + 2] as u32;
            let pos = codes.intern(code) + 1;
            match rarr.get(rid as usize) {
                Some(slot) => slot.store(pos, Relaxed),
                None => over.push((rid, pos)),
            }
            p += 3;
        }
        while p < f.len() && f[p] != b'\n' {
            p += 1;
        }
    }
    over
}

// ── names ───────────────────────────────────────────────────────────────────
// A table entry packs: occupied (bit 63), 6 hash bits, a 17-bit dense name id,
// the raw name length (8 bits) and the file offset of its first occurrence
// (32 bits). A match is confirmed against the file bytes, never by hash alone.
const TB: u32 = 18; // table slots: 2^18, at most half full
const LIMIT: u32 = 1 << 17; // names the packed table (and the bitsets) can hold
const OCC: u64 = 1 << 63;
const MMASK: u64 = OCC | 0x3F << 57 | 0xFF << 32;

struct Names {
    table: Vec<AtomicU64>,
    next: AtomicU32,
    over: AtomicBool, // the table could not hold the names: restart on the exact map
    rare: Mutex<HashMap<Vec<u8>, u32>>,
    slow: bool, // every name goes through `rare`
}

enum Hit {
    Id(u32),
    Cand(usize, u64),
}

impl Names {
    fn new(slow: bool) -> Self {
        Names {
            table: zeroed_u64(1 << TB),
            next: AtomicU32::new(0),
            over: AtomicBool::new(false),
            rare: Mutex::new(HashMap::new()),
            slow,
        }
    }

    /// Exact intern of any name (long names, slow mode).
    fn rare_intern(&self, name: &[u8]) -> u32 {
        let mut r = self.rare.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&id) = r.get(name) {
            return id;
        }
        let id = self.next.fetch_add(1, Relaxed);
        if id >= LIMIT && !self.slow {
            self.over.store(true, Relaxed);
        }
        r.insert(name.to_vec(), id);
        id
    }

    /// Probe from slot s for a name with hash h: a candidate entry whose tag and
    /// length match (to be confirmed by the caller), or a fresh id if the name is new.
    #[inline(always)]
    fn find(&self, noff: usize, nlen: usize, h: u64, mut s: usize) -> Hit {
        let want = OCC | (h & 0x3F) << 57 | (nlen as u64) << 32;
        loop {
            let mut e = self.table[s].load(Relaxed);
            if e == 0 {
                let id = self.next.fetch_add(1, Relaxed);
                if id >= LIMIT {
                    self.over.store(true, Relaxed);
                    return Hit::Id(0);
                }
                match self.table[s].compare_exchange(0, want | (id as u64) << 40 | noff as u64, Relaxed, Relaxed) {
                    Ok(_) => return Hit::Id(id),
                    Err(c) => e = c, // another thread took the slot first
                }
            }
            if (e ^ want) & MMASK == 0 {
                return Hit::Cand(s, e);
            }
            s = (s + 1) & (self.table.len() - 1);
        }
    }
}

/// Is the name of entry e the same bytes as the nlen bytes at noff?
#[inline(always)]
fn same(f: &[u8], e: u64, noff: usize, nlen: usize) -> bool {
    let eo = (e & 0xFFFF_FFFF) as usize;
    f[eo..eo + nlen] == f[noff..noff + nlen]
}

// One bitset over name ids per country (the last is _UNMATCHED). Ids past the
// bitset (slow mode only) go to an exact set.
const BITCAP: usize = LIMIT as usize;
const BW: usize = BITCAP / 64;

struct Bits {
    w: Vec<AtomicU64>,
    sets: Vec<Mutex<HashSet<u32>>>,
}

impl Bits {
    fn new(nco: usize) -> Self {
        Bits { w: zeroed_u64(nco * BW), sets: (0..nco).map(|_| Mutex::new(HashSet::new())).collect() }
    }
    #[inline(always)]
    fn prefetch(&self, c: usize, id: u32) {
        if (id as usize) < BITCAP {
            prefetch(&self.w[c * BW + (id >> 6) as usize]);
        }
    }
    #[inline(always)]
    /// Marks the name for the country; true if it was not marked before, so the distinct
    /// counts come out of the marking (the OR returns the old bit) with no pass over the bitsets.
    fn mark(&self, c: usize, id: u32) -> bool {
        if (id as usize) < BITCAP {
            // No test first: most marks are first sightings, and a read would fetch the line twice.
            let bit = 1u64 << (id & 63);
            self.w[c * BW + (id >> 6) as usize].fetch_or(bit, Relaxed) & bit == 0
        } else {
            self.sets[c].lock().unwrap_or_else(|e| e.into_inner()).insert(id)
        }
    }
}

// ── records ─────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Default)]
struct Rec {
    rid: u64,
    region: u64,
    cents: i64,
    noff: usize, // the name's raw bytes (between the quotes when quoted) at noff in the file
    nlen: u32,
    nbytes: u32, // its value length (a "" pair counts one)
    tbytes: u32, // the note's value length
}

/// A field's value length given its text length and its quote count.
#[inline(always)]
fn value_len(text: usize, q: u64) -> u32 {
    (text - if q > 0 { q as usize / 2 + 1 } else { 0 }) as u32
}

/// One record in exactly the expected shape, or None (the scalar parser then decides).
/// `qb` is the quote count before this record; the scan has consumed up to its LF.
#[inline(always)]
fn fast<const A: bool>(f: &[u8], sc: &mut Scan<A>, p: usize, end: usize, qb: &mut u64) -> Option<(Rec, usize)> {
    let ((c1, q1), (c2, q2), (c3, q3), (c4, q4), (e, q5)) = (sc.next(), sc.next(), sc.next(), sc.next(), sc.next());
    if e >= end || f.len() < p + 3 || (f[c1], f[c2], f[c3], f[c4], f[e]) != (b',', b',', b',', b',', b'\n') {
        return None;
    }
    if q1 != *qb || q4 != q3 || q5 != q4 {
        return None; // id, region and amount hold no quotes
    }
    let mut s = p;
    if f[s] == 0xEF {
        if f[s + 1] != 0xBB || f[s + 2] != 0xBF {
            return None;
        }
        s += 3;
    }
    let (n1, n3) = (c1.wrapping_sub(s), c4 - c3 - 1);
    if n1 == 0 || n1 > 8 || n3 == 0 || n3 > 8 || c1 < 8 {
        return None;
    }
    let (w1, w3) = (load8(f, c1 - 8), load8(f, c4 - 8));
    if !digits_ok(w1, n1) || !digits_ok(w3, n3) {
        return None;
    }
    // amount: -?digits.dd with at most 7 digits; the eight bytes ending at its end, dot dropped, are one number
    let a0 = c4 + 1;
    let neg = f[a0] == b'-';
    let ae = e - (f[e - 1] == b'\r') as usize;
    let alen = ae.wrapping_sub(a0 + neg as usize);
    if !(4..=8).contains(&alen) || f[ae - 3] != b'.' {
        return None;
    }
    let w = load8(f, ae - 8);
    let w = (w & 0xFF_FFFF_FFFF) << 8 | (w & 0xFFFF_0000_0000_0000);
    if !digits_ok(w, alen - 1) {
        return None;
    }
    let cents = fold(w, alen - 1) as i64;
    // name and note: unquoted, or one quoted run with its two delimiting quotes
    let (qn, qt) = (q2 - q1, q3 - q2);
    let (nt, tt) = (c2 - c1 - 1, c3 - c2 - 1);
    let quoted_ok = |from: usize, text: usize, q: u64| q == 0 || (q % 2 == 0 && text >= 2 && f[from] == b'"' && f[from + text - 1] == b'"');
    if !quoted_ok(c1 + 1, nt, qn) || !quoted_ok(c2 + 1, tt, qt) {
        return None;
    }
    *qb = q5;
    let (noff, nlen) = if qn > 0 { (c1 + 2, nt - 2) } else { (c1 + 1, nt) };
    let rec = Rec {
        rid: fold(w1, n1),
        region: fold(w3, n3),
        cents: if neg { -cents } else { cents },
        noff,
        nlen: nlen as u32,
        nbytes: value_len(nt, qn),
        tbytes: value_len(tt, qt),
    };
    Some((rec, e + 1))
}

/// A field at pos: (raw start, raw length, value length). Quoted: the run up to the
/// closing quote, `""` being one literal quote; unquoted: up to a comma, LF or CR.
#[inline(always)]
fn field(b: &[u8], pos: &mut usize) -> (usize, usize, usize) {
    let s = *pos;
    if s < b.len() && b[s] == b'"' {
        let (mut i, mut v) = (s + 1, 0);
        loop {
            if i >= b.len() {
                *pos = i;
                return (s + 1, i - (s + 1), v);
            }
            if b[i] != b'"' {
                (i, v) = (i + 1, v + 1);
            } else if i + 1 < b.len() && b[i + 1] == b'"' {
                (i, v) = (i + 2, v + 1);
            } else {
                *pos = i + 1;
                return (s + 1, i - (s + 1), v);
            }
        }
    }
    let mut i = s;
    while i < b.len() && b[i] != b',' && b[i] != b'\n' && b[i] != b'\r' {
        i += 1;
    }
    *pos = i;
    (s, i - s, i - s)
}

#[inline(always)]
fn digits(b: &[u8], pos: &mut usize) -> u64 {
    let mut v = 0u64;
    while *pos < b.len() && b[*pos].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((b[*pos] - b'0') as u64);
        *pos += 1;
    }
    v
}

/// The record at p, parsed by the literal rules of the format. The last record of the
/// file may lack its terminator; any other record that does not end in CRLF or LF is
/// not parsed (None), and the rest of its piece with it.
#[inline(always)]
fn slow(f: &[u8], p: usize, end: usize) -> Option<(Rec, usize)> {
    let b = &f[..end];
    let mut pos = p;
    if b[pos..].starts_with(&BOM) {
        pos += 3;
        if pos >= end {
            return None;
        }
    }
    let rid = digits(b, &mut pos);
    let comma = |pos: &mut usize| {
        if b.get(*pos) == Some(&b',') {
            *pos += 1;
        }
    };
    comma(&mut pos);
    let (noff, nlen, nval) = field(b, &mut pos);
    comma(&mut pos);
    let (_, _, tval) = field(b, &mut pos);
    comma(&mut pos);
    let region = digits(b, &mut pos);
    comma(&mut pos);
    let neg = pos < end && b[pos] == b'-';
    pos += neg as usize;
    let mut cents = (digits(b, &mut pos) as i64).wrapping_mul(100);
    if pos + 2 < end && b[pos] == b'.' {
        let (d1, d2) = (b[pos + 1].wrapping_sub(b'0') as i64, b[pos + 2].wrapping_sub(b'0') as i64);
        pos += 3;
        cents = cents.wrapping_add(d1 * 10 + d2);
    }
    if neg {
        cents = -cents;
    }
    if pos < end && b[pos] == b'\r' {
        pos += 1;
    }
    if pos < end && b[pos] == b'\n' {
        pos += 1;
    } else if !(pos == end && end == f.len()) {
        return None;
    }
    let rec = Rec { rid, region, cents, noff, nlen: nlen as u32, nbytes: nval as u32, tbytes: tval as u32 };
    Some((rec, pos))
}

// ── the pipeline ────────────────────────────────────────────────────────────
// Each record passes three stages, LAG records apart, so the random reads of
// each (region row, name slot, name bytes, bitset word) are prefetched ahead:
// 1 country, sums, name probe; 2 confirm the name, prefetch its bit; 3 mark.
const LAG: u64 = 16;

struct Ctx<'a> {
    f: &'a [u8],
    rarr: &'a [AtomicU16],
    big: &'a HashMap<u64, u16>,
    rank: &'a [u16], // region slot (0 = none) -> country index
    names: &'a Names,
    bits: &'a Bits,
}

#[derive(Clone, Copy, Default)]
struct Pend {
    r: Rec,
    h: u64,
    cand: u64,
    slot: u32,
    country: u32,
    nid: u32,
}

struct Pipe<'a> {
    cx: &'a Ctx<'a>,
    aggs: Vec<Agg>,
    ring: Vec<Pend>,
    n: u64,
}

impl<'a> Pipe<'a> {
    fn new(cx: &'a Ctx<'a>, nco: usize) -> Self {
        Pipe { cx, aggs: vec![Agg::new(); nco], ring: vec![Pend::default(); 64], n: 0 }
    }

    #[inline(always)]
    fn push(&mut self, r: &Rec) {
        self.advance(self.n);
        let cx = self.cx;
        let h = hash(&cx.f[r.noff..r.noff + r.nlen as usize]);
        let slot = (h >> (64 - TB)) as u32;
        prefetch(&cx.names.table[slot as usize]);
        if (r.region as usize) < cx.rarr.len() {
            prefetch(&cx.rarr[r.region as usize]);
        }
        self.ring[(self.n & 63) as usize] = Pend { r: *r, h, slot, ..Pend::default() };
        self.n += 1;
    }

    #[inline(always)]
    fn advance(&mut self, vn: u64) {
        let (cx, n) = (self.cx, self.n);
        if vn >= 3 * LAG && vn - 3 * LAG < n {
            let p = &self.ring[((vn - 3 * LAG) & 63) as usize];
            self.aggs[p.country as usize].distinct += cx.bits.mark(p.country as usize, p.nid) as u64;
        }
        if vn >= 2 * LAG && vn - 2 * LAG < n {
            let p = &mut self.ring[((vn - 2 * LAG) & 63) as usize];
            if p.cand != 0 {
                let (noff, nlen) = (p.r.noff as usize, p.r.nlen as usize);
                let (mut e, mut s) = (p.cand, p.slot as usize);
                loop {
                    if same(cx.f, e, noff, nlen) {
                        p.nid = (e >> 40) as u32 & (LIMIT - 1);
                        break;
                    }
                    // a tag collision: keep probing, exactly
                    match cx.names.find(noff, nlen, p.h, (s + 1) & (cx.names.table.len() - 1)) {
                        Hit::Id(id) => {
                            p.nid = id;
                            break;
                        }
                        Hit::Cand(cs, ce) => (s, e) = (cs, ce),
                    }
                }
            }
            cx.bits.prefetch(p.country as usize, p.nid);
        }
        if vn >= LAG && vn - LAG < n {
            let (ring, aggs) = (&mut self.ring, &mut self.aggs);
            let p = &mut ring[((vn - LAG) & 63) as usize];
            let rs = match cx.rarr.get(p.r.region as usize) {
                Some(s) => s.load(Relaxed),
                None if cx.big.is_empty() => 0,
                None => cx.big.get(&p.r.region).copied().unwrap_or(0),
            };
            let c = cx.rank[rs as usize] as usize;
            p.country = c as u32;
            let a = &mut aggs[c];
            a.count += 1;
            a.id_sum = a.id_sum.wrapping_add(p.r.rid as i64);
            a.amount_sum += p.r.cents;
            a.amount_min = a.amount_min.min(p.r.cents);
            a.amount_max = a.amount_max.max(p.r.cents);
            a.name_bytes += p.r.nbytes as u64;
            a.note_bytes += p.r.tbytes as u64;
            let (noff, nlen) = (p.r.noff as usize, p.r.nlen as usize);
            if cx.names.slow || nlen > 255 {
                p.nid = cx.names.rare_intern(&cx.f[noff..noff + nlen]);
            } else {
                match cx.names.find(noff, nlen, p.h, p.slot as usize) {
                    Hit::Id(id) => p.nid = id,
                    Hit::Cand(s, e) => {
                        (p.cand, p.slot) = (e, s as u32);
                        prefetch(cx.f.as_ptr().wrapping_add((e & 0xFFFF_FFFF) as usize));
                    }
                }
            }
        }
    }

    fn finish(mut self) -> Vec<Agg> {
        for vn in self.n..self.n + 3 * LAG {
            self.advance(vn);
        }
        self.aggs
    }
}

/// Parse every record that starts in [start, end), both being record boundaries.
#[inline(always)]
fn parse_piece_impl<const A: bool>(cx: &Ctx, pipe: &mut Pipe, start: usize, end: usize) {
    let f = cx.f;
    let (mut p, mut sc, mut qb) = (start, Scan::<A>::new(f, start), 0u64);
    while p < end {
        // The mapped file's pages are scattered; reach into the next one ahead of the parse.
        prefetch(f.as_ptr().wrapping_add(p + 2048));
        if pipe.n & 0xFFF == 0 && cx.names.over.load(Relaxed) {
            return;
        }
        match fast(f, &mut sc, p, end, &mut qb) {
            Some((r, np)) => {
                pipe.push(&r);
                p = np;
            }
            None => match slow(f, p, end) {
                Some((r, np)) => {
                    pipe.push(&r);
                    (p, sc, qb) = (np, Scan::<A>::new(f, np), 0);
                }
                None => return,
            },
        }
    }
}

/// The same parse compiled for the CPU features the scoring box has (popcnt, BMI, PCLMUL, AVX2),
/// chosen at run time; any other CPU runs the identical source built for the baseline target.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,bmi1,bmi2,popcnt,lzcnt,pclmulqdq,sse4.2")]
unsafe fn parse_piece_native(cx: &Ctx, pipe: &mut Pipe, start: usize, end: usize) {
    parse_piece_impl::<true>(cx, pipe, start, end)
}

fn native() -> bool {
    is_x86_feature_detected!("avx2")
        && is_x86_feature_detected!("bmi1")
        && is_x86_feature_detected!("bmi2")
        && is_x86_feature_detected!("popcnt")
        && is_x86_feature_detected!("lzcnt")
        && is_x86_feature_detected!("pclmulqdq")
        && is_x86_feature_detected!("sse4.2")
}

fn parse_piece(cx: &Ctx, pipe: &mut Pipe, start: usize, end: usize, nat: bool) {
    if nat {
        unsafe { parse_piece_native(cx, pipe, start, end) }
    } else {
        parse_piece_impl::<false>(cx, pipe, start, end)
    }
}

fn main() {
    // Plain pages: a table touched in part must not be charged as whole 2 MB pages.
    #[cfg(target_os = "linux")]
    unsafe { prctl(41, 1, 0, 0, 0) }; // PR_SET_THP_DISABLE
    let data = std::env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = std::env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());
    let nt = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 12);
    let nat = native();
    let rf = map(&format!("{data}/regions.csv"));
    let f = map(&format!("{data}/records.csv"));

    // 1. regions.csv in parallel, and the quote parity of every chunk of records.csv.
    let rstart = rf.iter().position(|&c| c == b'\n').map_or(rf.len(), |i| i + 1);
    // The last line holds the largest id when ids ascend; any other id is still handled exactly.
    let tail = rf.strip_suffix(b"\n").unwrap_or(rf);
    let tail = tail.strip_suffix(b"\r").unwrap_or(tail);
    let ls = tail.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
    let hint = tail[ls..].iter().take_while(|c| c.is_ascii_digit()).take(8).fold(0usize, |v, &d| v * 10 + (d - b'0') as usize);
    let rarr = zeroed_u16((hint + 1).min(1 << 23));
    let codes = CodeTab::new();
    let rcut = cuts(rf, rstart.min(rf.len()), nt);
    let nchunk = f.len().div_ceil(STEP);
    let per = nchunk.div_ceil(nt).max(1);
    let got = on_threads(nt, |t| {
        let r = regions_part(rf, rcut[t], rcut[t + 1], &rarr, &codes);
        (r, quote_parity(f, (t * per).min(nchunk), ((t + 1) * per).min(nchunk), nat))
    });
    let mut ok = got.iter().all(|g| g.0.ok);
    let mut prev: Option<u64> = None;
    for g in got.iter().filter(|g| g.0.n > 0) {
        ok &= prev.map_or(true, |l| l < g.0.first);
        prev = Some(g.0.last);
    }
    let mut over: Vec<(u64, u16)> = Vec::new();
    let mut par = Vec::with_capacity(nchunk + 1);
    for (r, p) in got {
        over.extend(r.over);
        par.extend(p);
    }
    if !ok {
        // not strictly ascending, or a line of another shape: the literal parse decides
        rarr.iter().for_each(|s| s.store(0, Relaxed));
        over = regions_exact(rf, &rarr, &codes);
    }
    let big: HashMap<u64, u16> = over.into_iter().collect();
    let mut present: Vec<(u32, usize)> = (0..4096).filter_map(|s| match codes.t[s].load(Relaxed) {
        0 => None,
        e => Some((e & 0xFF_FFFF, s)),
    }).collect();
    present.sort_unstable();
    let ncodes = present.len();
    let mut rank = vec![ncodes as u16; 4097]; // 0 = no region row: _UNMATCHED
    for (k, &(_, s)) in present.iter().enumerate() {
        rank[s + 1] = k as u16;
    }

    // 2. Piece boundaries: LFs at even quote parity, found from the chunk parities.
    let mut acc = 0u8;
    let before: Vec<u8> = par.iter().map(|&p| { let b = acc; acc ^= p; b }).collect();
    let start = first_boundary(f, 0, 0);
    let npieces = 8 * nt;
    let mut cut = vec![f.len(); npieces + 1];
    cut[0] = start;
    for i in 1..npieces {
        let c = ((start + (f.len() - start) * i / npieces) / STEP).min(nchunk.saturating_sub(1));
        cut[i] = cut[i - 1].max(first_boundary(f, c * STEP, before.get(c).copied().unwrap_or(0)));
    }

    // 3. Parse the pieces. A run that needs the exact name map is repeated with it.
    let nco = ncodes + 1;
    let run = |slow: bool| {
        let (names, bits) = (Names::new(slow), Bits::new(nco));
        let cx = Ctx { f, rarr: &rarr, big: &big, rank: &rank, names: &names, bits: &bits };
        let next = AtomicUsize::new(0);
        let parts = on_threads(nt, |t| {
            populate(&names.table, t, nt);
            populate(&bits.w, t, nt);
            let mut pipe = Pipe::new(&cx, nco);
            loop {
                let k = next.fetch_add(1, Relaxed);
                if k >= npieces {
                    break;
                }
                parse_piece(&cx, &mut pipe, cut[k], cut[k + 1], nat);
            }
            pipe.finish()
        });
        (parts, names.over.load(Relaxed))
    };
    let (mut parts, over) = run(f.len() >= u32::MAX as usize);
    if over {
        parts = run(true).0;
    }

    // 4. Merge and write: countries in code order, then _UNMATCHED.
    let mut out: Vec<u8> = Vec::new();
    for c in 0..nco {
        let mut a = Agg::new();
        parts.iter().for_each(|p| a.merge(&p[c]));
        if a.count == 0 {
            continue;
        }
        if c < ncodes {
            let k = present[c].0;
            line(&mut out, &[(k >> 16) as u8, (k >> 8) as u8, k as u8], &a);
        } else {
            line(&mut out, b"_UNMATCHED", &a);
        }
    }
    if let Err(e) = std::fs::write(format!("{outd}/aggregates.txt"), out) {
        die(&format!("cannot write aggregates.txt: {e}"));
    }
}
