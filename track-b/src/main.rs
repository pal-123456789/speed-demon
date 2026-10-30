//! Speed Demon 2026 — Track B (Index).
//!
//! Strategy
//! --------
//! * Parse `queries.txt` first and intern every term it mentions (exactly the
//!   terms the query set needs — nothing else is indexed).
//! * Stream `docs.txt` once: ASCII-lowercase the line, tokenise it, and append
//!   the document id to each mentioned term's posting list.  Only the byte
//!   range of each document's text is retained, never the text itself.
//! * BOOL: evaluate strictly left to right on ascending `u32` id vectors
//!   (intersection / union / difference); galloping search is used when one
//!   side is much smaller than the other.
//! * PHRASE: intersect the terms' id lists (rarest first) and verify the
//!   consecutive token run in each candidate document, re-reading only that
//!   document's bytes through a 1 MiB sliding window.
//!
//! Tokenisation is exactly `re.findall(rb"[a-z0-9]+", text.lower())` on bytes:
//! ASCII-lowercase `A-Z` only, then maximal runs of `a-z0-9`; every other byte
//! (including every byte >= 0x80) is a separator.
//!
//! `std` only — no crates, no Cargo.toml.

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

const TRACK: &str = "B";
const DEFAULT_INPUT: &str = "docs.txt";
const DEFAULT_OUTPUT: &str = "results.txt";
const QUERIES_FILE: &str = "queries.txt";

// ── paths ──────────────────────────────────────────────────────────────

fn data_dir() -> PathBuf {
    env::var("SD_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data"))
}

fn out_dir() -> PathBuf {
    env::var("SD_OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/out"))
}

fn input_path() -> PathBuf {
    let name = env::var(format!("SD_IN_{TRACK}")).unwrap_or_else(|_| DEFAULT_INPUT.to_string());
    data_dir().join(name)
}

fn output_path() -> PathBuf {
    let name = env::var(format!("SD_OUT_{TRACK}")).unwrap_or_else(|_| DEFAULT_OUTPUT.to_string());
    out_dir().join(name)
}

// ── tokenisation ───────────────────────────────────────────────────────

const fn build_is_tok() -> [bool; 256] {
    let mut t = [false; 256];
    let mut b = 0usize;
    while b < 256 {
        t[b] = (b >= 0x61 && b <= 0x7a) || (b >= 0x30 && b <= 0x39);
        b += 1;
    }
    t
}
static IS_TOK: [bool; 256] = build_is_tok();

/// ASCII-lowercase bytes `0x41..=0x5A` in place.  No other byte changes.
#[inline]
fn ascii_lower_in_place(b: &mut [u8]) {
    for x in b.iter_mut() {
        *x |= (((*x).wrapping_sub(0x41) < 26) as u8) << 5;
    }
}

// ── term interning ─────────────────────────────────────────────────────

#[inline]
fn hash_bytes(b: &[u8]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut h: u64 = (b.len() as u64) ^ 0x517c_c1b7_2722_0a95;
    let mut it = b.chunks_exact(8);
    for c in &mut it {
        let w = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
        h = (h ^ w).wrapping_mul(K);
        h ^= h >> 29;
    }
    let mut w = 0u64;
    for (k, &x) in it.remainder().iter().enumerate() {
        w |= (x as u64) << (8 * k);
    }
    h = (h ^ w).wrapping_mul(K);
    h ^= h >> 31;
    h = h.wrapping_mul(0xd6e8_feb8_6659_fd93);
    h ^= h >> 32;
    h
}

/// Byte-string interner: open-addressed table of term ids into a byte arena.
struct TermTable {
    arena: Vec<u8>,
    offs: Vec<u32>,
    lens: Vec<u32>,
    slots: Vec<u32>, // 0 = empty, otherwise term id + 1
    mask: usize,
}

impl TermTable {
    fn new() -> Self {
        let mut t = Self {
            arena: Vec::new(),
            offs: Vec::new(),
            lens: Vec::new(),
            slots: Vec::new(),
            mask: 0,
        };
        t.alloc(1 << 12);
        t
    }

    fn alloc(&mut self, cap: usize) {
        self.slots = vec![0u32; cap];
        self.mask = cap - 1;
    }

    #[inline]
    fn len(&self) -> usize {
        self.offs.len()
    }

    #[inline]
    fn key(&self, id: u32) -> &[u8] {
        let o = self.offs[id as usize] as usize;
        let l = self.lens[id as usize] as usize;
        &self.arena[o..o + l]
    }

    #[inline]
    fn slot_for(&self, key: &[u8], h: u64) -> usize {
        let mut i = (h as usize) & self.mask;
        loop {
            let e = self.slots[i];
            if e == 0 {
                return i;
            }
            let t = (e - 1) as usize;
            let o = self.offs[t] as usize;
            let l = self.lens[t] as usize;
            if l == key.len() && self.arena[o..o + l] == *key {
                return i;
            }
            i = (i + 1) & self.mask;
        }
    }

    /// Look up bytes exactly as they appear after lowercasing.
    #[inline]
    fn get(&self, key: &[u8]) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        let h = hash_bytes(key);
        let e = self.slots[self.slot_for(key, h)];
        if e == 0 {
            None
        } else {
            Some(e - 1)
        }
    }

    fn intern(&mut self, key: &[u8]) -> u32 {
        let h = hash_bytes(key);
        let i = self.slot_for(key, h);
        let e = self.slots[i];
        if e != 0 {
            return e - 1;
        }
        let idx = self.offs.len() as u32;
        self.offs.push(self.arena.len() as u32);
        self.lens.push(key.len() as u32);
        self.arena.extend_from_slice(key);
        self.slots[i] = idx + 1;
        if self.offs.len() * 3 >= self.slots.len() * 2 {
            self.grow();
        }
        idx
    }

    fn grow(&mut self) {
        let cap = self.slots.len() * 2;
        let old = std::mem::replace(&mut self.slots, vec![0u32; cap]);
        self.mask = cap - 1;
        for e in old {
            if e == 0 {
                continue;
            }
            let t = (e - 1) as usize;
            let o = self.offs[t] as usize;
            let l = self.lens[t] as usize;
            let h = hash_bytes(&self.arena[o..o + l]);
            let mut i = (h as usize) & self.mask;
            while self.slots[i] != 0 {
                i = (i + 1) & self.mask;
            }
            self.slots[i] = e;
        }
    }
}

// ── queries ────────────────────────────────────────────────────────────

const OP_AND: u8 = 0;
const OP_OR: u8 = 1;
const OP_NOT: u8 = 2;

enum Query {
    /// `terms[0]`, then `ops[i]` combines the running result with `terms[i+1]`.
    Bool { terms: Vec<u32>, ops: Vec<u8> },
    Phrase(Vec<u32>),
}

/// Mirror the reference parser exactly (it splits on single spaces, so a
/// malformed line can only ever produce the same words the reference sees).
fn parse_queries(buf: &[u8], tt: &mut TermTable, out: &mut Vec<Query>) {
    let n = buf.len();
    let mut start = 0usize;
    while start < n {
        let end = match buf[start..].iter().position(|&b| b == b'\n') {
            Some(p) => start + p,
            None => n,
        };
        let line = &buf[start..end];
        start = end + 1;
        if line.is_empty() {
            continue;
        }
        match line.iter().position(|&b| b == b' ') {
            Some(sp) => {
                let kind = &line[..sp];
                let rest = &line[sp + 1..];
                if kind == b"BOOL" {
                    let mut words = rest.split(|&b| b == b' ');
                    let first = words.next().unwrap_or(b"");
                    let mut terms = Vec::with_capacity(4);
                    let mut ops = Vec::with_capacity(3);
                    terms.push(tt.intern(first));
                    loop {
                        let op = match words.next() {
                            Some(o) => o,
                            None => break,
                        };
                        let term = words.next().unwrap_or(b"");
                        ops.push(match op {
                            b"AND" => OP_AND,
                            b"OR" => OP_OR,
                            b"NOT" => OP_NOT,
                            // The reference raises here. Treat anything else as
                            // an operator that cannot match, i.e. keep the
                            // running result: this keeps a malformed line from
                            // taking the whole run down.
                            _ => continue,
                        });
                        terms.push(tt.intern(term));
                    }
                    out.push(Query::Bool { terms, ops });
                } else if kind == b"PHRASE" {
                    let inner = if rest.len() >= 2 && rest[0] == b'"' && rest[rest.len() - 1] == b'"'
                    {
                        &rest[1..rest.len() - 1]
                    } else {
                        rest
                    };
                    let terms: Vec<u32> = inner.split(|&b| b == b' ').map(|t| tt.intern(t)).collect();
                    out.push(Query::Phrase(terms));
                } else {
                    // Unknown kind: the reference raises.  Emit an empty result
                    // (one output line, in order) rather than aborting.
                    out.push(Query::Bool {
                        terms: vec![tt.intern(line)],
                        ops: Vec::new(),
                    });
                }
            }
            None => out.push(Query::Bool {
                terms: vec![tt.intern(line)],
                ops: Vec::new(),
            }),
        }
    }
}

// ── corpus scan ────────────────────────────────────────────────────────

#[inline]
fn parse_u32(b: &[u8]) -> u32 {
    let mut v: u32 = 0;
    for &c in b {
        if c.is_ascii_digit() {
            v = v.wrapping_mul(10).wrapping_add((c - b'0') as u32);
        } else {
            break;
        }
    }
    v
}

#[inline]
fn index_text(
    text: &[u8],
    tt: &TermTable,
    postings: &mut [Vec<u32>],
    stamp: &mut [u32],
    doc: u32,
) {
    let n = text.len();
    let mut i = 0usize;
    while i < n {
        while i < n && !IS_TOK[text[i] as usize] {
            i += 1;
        }
        let s = i;
        while i < n && IS_TOK[text[i] as usize] {
            i += 1;
        }
        if i > s {
            if let Some(t) = tt.get(&text[s..i]) {
                let ti = t as usize;
                if stamp[ti] != doc {
                    stamp[ti] = doc;
                    postings[ti].push(doc);
                }
            }
        }
    }
}

/// One streaming pass: postings for the mentioned terms + the byte range of
/// every document's text (ids are the parsed line ids; `offs`/`lens` are
/// indexed by that id).
fn build_index(
    path: &PathBuf,
    tt: &TermTable,
    postings: &mut [Vec<u32>],
    stamp: &mut [u32],
    offs: &mut Vec<u32>,
    lens: &mut Vec<u32>,
) -> io::Result<()> {
    let f = File::open(path)?;
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut line: Vec<u8> = Vec::with_capacity(1024);
    let mut pos: u64 = 0;
    let mut doc: u32 = 0;
    loop {
        line.clear();
        let n = r.read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        let mut end = n;
        if line[end - 1] == b'\n' {
            end -= 1;
        }
        let tab = line[..end].iter().position(|&b| b == b'\t');
        let (id, tstart) = match tab {
            Some(t) => (parse_u32(&line[..t]), t + 1),
            None => (doc, end),
        };
        let text = &mut line[tstart..end];
        ascii_lower_in_place(text);
        let idx = id as usize;
        if idx >= offs.len() {
            offs.resize(idx + 1, 0u32);
            lens.resize(idx + 1, 0u32);
        }
        offs[idx] = (pos + tstart as u64) as u32;
        lens[idx] = text.len() as u32;
        index_text(text, tt, postings, stamp, id);
        pos += n as u64;
        doc += 1;
    }
    Ok(())
}

// ── set algebra on ascending id vectors ────────────────────────────────

/// First index `>= from` whose value is `>= x`.
#[inline]
fn gallop(a: &[u32], from: usize, x: u32) -> usize {
    let n = a.len();
    if from >= n {
        return n;
    }
    let mut bound = 1usize;
    while from + bound < n && a[from + bound] < x {
        bound <<= 1;
    }
    let mut lo = from + (bound >> 1);
    let mut hi = (from + bound + 1).min(n);
    while lo < hi {
        let m = lo + (hi - lo) / 2;
        if a[m] < x {
            lo = m + 1;
        } else {
            hi = m;
        }
    }
    lo
}

fn intersect_into(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    if a.is_empty() || b.is_empty() {
        return;
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if small.len() * 16 < large.len() {
        out.reserve(small.len());
        let mut base = 0usize;
        for &v in small {
            let p = gallop(large, base, v);
            if p == large.len() {
                break;
            }
            if large[p] == v {
                out.push(v);
            }
            base = p;
        }
    } else {
        out.reserve(small.len());
        let (mut i, mut j) = (0usize, 0usize);
        while i < a.len() && j < b.len() {
            let (x, y) = (a[i], b[j]);
            if x < y {
                i += 1;
            } else if x > y {
                j += 1;
            } else {
                out.push(x);
                i += 1;
                j += 1;
            }
        }
    }
}

fn union_into(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    if a.is_empty() {
        out.extend_from_slice(b);
        return;
    }
    if b.is_empty() {
        out.extend_from_slice(a);
        return;
    }
    out.reserve(a.len() + b.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i], b[j]);
        if x < y {
            out.push(x);
            i += 1;
        } else if x > y {
            out.push(y);
            j += 1;
        } else {
            out.push(x);
            i += 1;
            j += 1;
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
}

fn difference_into(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    if a.is_empty() {
        return;
    }
    if b.is_empty() {
        out.extend_from_slice(a);
        return;
    }
    if a.len() * 16 < b.len() {
        out.reserve(a.len());
        let mut base = 0usize;
        for &v in a {
            let p = gallop(b, base, v);
            if p == b.len() {
                out.push(v);
                base = p;
                continue;
            }
            if b[p] != v {
                out.push(v);
            }
            base = p;
        }
    } else {
        out.reserve(a.len());
        let mut j = 0usize;
        for &v in a {
            while j < b.len() && b[j] < v {
                j += 1;
            }
            if j == b.len() {
                out.push(v);
            } else if b[j] != v {
                out.push(v);
            }
        }
    }
}

// ── phrase verification ────────────────────────────────────────────────

/// Sliding 1 MiB window over `docs.txt`, so candidate verification costs a
/// handful of reads instead of one `seek`/`read` pair per candidate.
struct DocReader {
    file: File,
    buf: Vec<u8>,
    base: u64,
    valid: usize,
}

impl DocReader {
    fn open(path: &PathBuf) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
            buf: vec![0u8; 1 << 20],
            base: 0,
            valid: 0,
        })
    }

    #[inline]
    fn text(&mut self, off: u32, len: u32) -> io::Result<&[u8]> {
        let off = off as u64;
        let len = len as usize;
        if off < self.base || off + len as u64 > self.base + self.valid as u64 {
            // Windows start on a 1 MiB boundary so consecutive candidates
            // share one read; grow the buffer for the rare oversized document.
            let win = 1u64 << 20;
            let start = off - (off % win);
            let need = (off - start) as usize + len;
            if self.buf.len() < need {
                self.buf.resize(need.next_power_of_two(), 0);
            }
            self.file.seek(SeekFrom::Start(start))?;
            let mut got = 0usize;
            while got < self.buf.len() {
                let n = self.file.read(&mut self.buf[got..])?;
                if n == 0 {
                    break;
                }
                got += n;
            }
            self.base = start;
            self.valid = got;
        }
        let s = (off - self.base) as usize;
        Ok(&self.buf[s..s + len])
    }
}

/// Does `text` contain `terms` at `terms.len()` consecutive token positions?
fn phrase_match(
    text: &[u8],
    terms: &[&[u8]],
    lc: &mut Vec<u8>,
    spans: &mut Vec<(u32, u32)>,
) -> bool {
    let k = terms.len();
    if k == 0 {
        return true;
    }
    lc.clear();
    lc.extend_from_slice(text);
    ascii_lower_in_place(lc);
    spans.clear();
    let n = lc.len();
    let mut i = 0usize;
    while i < n {
        while i < n && !IS_TOK[lc[i] as usize] {
            i += 1;
        }
        let s = i;
        while i < n && IS_TOK[lc[i] as usize] {
            i += 1;
        }
        if i > s {
            spans.push((s as u32, (i - s) as u32));
        }
    }
    if spans.len() < k {
        return false;
    }
    let first = terms[0];
    'win: for w in 0..=(spans.len() - k) {
        let (s, l) = spans[w];
        if l as usize != first.len() || &lc[s as usize..s as usize + l as usize] != first {
            continue;
        }
        for j in 1..k {
            let (s, l) = spans[w + j];
            let t = terms[j];
            if l as usize != t.len() || &lc[s as usize..s as usize + l as usize] != t {
                continue 'win;
            }
        }
        return true;
    }
    false
}

// ── output ─────────────────────────────────────────────────────────────

#[inline]
fn push_u64(out: &mut Vec<u8>, mut v: u64) {
    if v == 0 {
        out.push(b'0');
        return;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    while v > 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&tmp[i..]);
}

/// `<count> <sum> <id_1> ... <id_10>` + `\n`, exactly as the reference writes.
#[inline]
fn format_result(out: &mut Vec<u8>, r: &[u32]) {
    out.clear();
    push_u64(out, r.len() as u64);
    out.push(b' ');
    let mut sum: u64 = 0;
    for &v in r {
        sum += v as u64;
    }
    push_u64(out, sum);
    for &v in r.iter().take(10) {
        out.push(b' ');
        push_u64(out, v as u64);
    }
    out.push(b'\n');
}

// ── main ───────────────────────────────────────────────────────────────

fn main() {
    let docs_path = input_path();
    let queries_path = data_dir().join(QUERIES_FILE);
    let out_path = output_path();
    let timing = env::var("SD_TIME").is_ok();
    let t0 = std::time::Instant::now();
    macro_rules! lap {
        ($name:expr) => {
            if timing {
                eprintln!("[t] {:<16} {:>8.3}s", $name, t0.elapsed().as_secs_f64());
            }
        };
    }

    let mut tt = TermTable::new();
    let mut queries: Vec<Query> = Vec::new();
    match fs::read(&queries_path) {
        Ok(buf) => parse_queries(&buf, &mut tt, &mut queries),
        Err(e) => {
            eprintln!("[track-{TRACK}] cannot read {}: {e}", queries_path.display());
            return;
        }
    }
    lap!("parse queries");

    let nterms = tt.len();
    let mut postings: Vec<Vec<u32>> = vec![Vec::new(); nterms];
    let mut stamp: Vec<u32> = vec![u32::MAX; nterms];
    let mut offs: Vec<u32> = Vec::with_capacity(1 << 18);
    let mut lens: Vec<u32> = Vec::with_capacity(1 << 18);
    if let Err(e) = build_index(&docs_path, &tt, &mut postings, &mut stamp, &mut offs, &mut lens) {
        eprintln!("[track-{TRACK}] cannot index {}: {e}", docs_path.display());
        return;
    }
    drop(stamp);
    lap!("build index");

    if let Err(e) = fs::create_dir_all(out_dir()) {
        eprintln!("[track-{TRACK}] cannot create output dir: {e}");
    }
    let file = match File::create(&out_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[track-{TRACK}] cannot create {}: {e}", out_path.display());
            return;
        }
    };
    let mut w = BufWriter::with_capacity(1 << 20, file);

    let mut cur: Vec<u32> = Vec::new();
    let mut nxt: Vec<u32> = Vec::new();
    let mut line: Vec<u8> = Vec::with_capacity(256);
    let mut lc: Vec<u8> = Vec::new();
    let mut spans: Vec<(u32, u32)> = Vec::new();
    let mut doc_reader: Option<DocReader> = None;
    let mut cand: Vec<u32> = Vec::new();
    let mut hits: Vec<u32> = Vec::new();
    let mut order: Vec<usize> = Vec::new();
    let mut io_err = false;
    let mut t_bool = std::time::Duration::ZERO;
    let mut t_cand = std::time::Duration::ZERO;
    let mut t_ver = std::time::Duration::ZERO;

    for q in &queries {
        match q {
            Query::Bool { terms, ops } => {
                let ts = std::time::Instant::now();
                cur.clear();
                cur.extend_from_slice(&postings[terms[0] as usize]);
                for (i, &op) in ops.iter().enumerate() {
                    let s = &postings[terms[i + 1] as usize];
                    match op {
                        OP_AND => {
                            if cur.is_empty() {
                                continue;
                            }
                            intersect_into(&cur, s, &mut nxt);
                        }
                        OP_OR => {
                            if s.is_empty() {
                                continue;
                            }
                            union_into(&cur, s, &mut nxt);
                        }
                        _ => {
                            if cur.is_empty() || s.is_empty() {
                                continue;
                            }
                            difference_into(&cur, s, &mut nxt);
                        }
                    }
                    std::mem::swap(&mut cur, &mut nxt);
                    if cur.is_empty() {
                        // Stay empty unless a later OR can fill it again.
                        let more_or = ops[i + 1..].iter().any(|&o| o == OP_OR);
                        if !more_or {
                            break;
                        }
                    }
                }
                format_result(&mut line, &cur);
                t_bool += ts.elapsed();
            }
            Query::Phrase(terms) => {
                let ts = std::time::Instant::now();
                // Intersect rarest list first; intersection is commutative, so
                // the result is the reference's candidate set.
                order.clear();
                order.extend(0..terms.len());
                order.sort_by_key(|&i| postings[terms[i] as usize].len());
                cand.clear();
                if let Some(&first) = order.first() {
                    cand.extend_from_slice(&postings[terms[first] as usize]);
                }
                for &i in order.iter().skip(1) {
                    if cand.is_empty() {
                        break;
                    }
                    intersect_into(&cand, &postings[terms[i] as usize], &mut nxt);
                    std::mem::swap(&mut cand, &mut nxt);
                }
                hits.clear();
                if !cand.is_empty() {
                    t_cand += ts.elapsed();
                    let ts2 = std::time::Instant::now();
                    let tbytes: Vec<&[u8]> = terms.iter().map(|&t| tt.key(t)).collect();
                    if doc_reader.is_none() {
                        doc_reader = match DocReader::open(&docs_path) {
                            Ok(d) => Some(d),
                            Err(e) => {
                                eprintln!("[track-{TRACK}] cannot reopen docs: {e}");
                                None
                            }
                        };
                    }
                    if let Some(dr) = doc_reader.as_mut() {
                        for &d in cand.iter() {
                            let di = d as usize;
                            if di >= offs.len() {
                                continue;
                            }
                            match dr.text(offs[di], lens[di]) {
                                Ok(text) => {
                                    if phrase_match(text, &tbytes, &mut lc, &mut spans) {
                                        hits.push(d);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[track-{TRACK}] read error: {e}");
                                    break;
                                }
                            }
                        }
                    }
                    t_ver += ts2.elapsed();
                } else {
                    t_cand += ts.elapsed();
                }
                format_result(&mut line, &hits);
            }
        }
        if w.write_all(&line).is_err() {
            io_err = true;
            break;
        }
    }

    if !io_err && w.flush().is_err() {
        io_err = true;
    }
    if io_err {
        eprintln!("[track-{TRACK}] write error on {}", out_path.display());
    }
    if timing {
        eprintln!("[t] {:<16} {:>8.3}s", "bool eval", t_bool.as_secs_f64());
        eprintln!("[t] {:<16} {:>8.3}s", "phrase cand", t_cand.as_secs_f64());
        eprintln!("[t] {:<16} {:>8.3}s", "phrase verify", t_ver.as_secs_f64());
        eprintln!("[t] {:<16} {:>8.3}s", "total", t0.elapsed().as_secs_f64());
    }
}
