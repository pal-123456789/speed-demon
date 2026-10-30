//! Speed Demon 2026 — Track B (Index).
//!
//! Strategy
//! --------
//! * Parse `queries.txt` first and intern every term it mentions: exactly the
//!   terms the query set needs, nothing else is indexed.
//! * Stream `docs.txt` once, tokenising each document and appending the
//!   document id to every mentioned term's posting list.  The text itself is
//!   never retained.
//! * BOOL: evaluate strictly left to right on ascending `u32` id vectors
//!   (intersection / union / difference).  Galloping search is used whenever
//!   one side is much smaller than the other.
//! * PHRASE: intersect the terms' id lists (rarest first) for candidates,
//!   collect the `(document, phrase)` pairs, and verify them in a second
//!   sequential pass over `docs.txt` — the pairs are sorted by document id,
//!   so the file is read front to back with no random access at all.
//!
//! Tokenisation is exactly `re.findall(rb"[a-z0-9]+", text.lower())` on bytes:
//! ASCII-lowercase `A-Z` only, then maximal runs of `a-z0-9`; every other byte
//! (including every byte >= 0x80) is a separator.
//!
//! `std` only — no crates, no Cargo.toml.

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
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

/// Token spans (start, len) of the already-lowercased text.
#[inline]
fn tokenize_spans(text: &[u8], spans: &mut Vec<(u32, u32)>) {
    spans.clear();
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
            spans.push((s as u32, (i - s) as u32));
        }
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

/// Byte-string interner: open-addressed table of term ids over a byte arena.
/// Each slot is `(hash tag << 32) | (term id + 1)`; 0 marks an empty slot, so
/// a probe rejects a non-matching slot from one 64-bit load in the common case.
struct TermTable {
    arena: Vec<u8>,
    offs: Vec<u32>,
    lens: Vec<u32>,
    slots: Vec<u64>,
    mask: usize,
}

impl TermTable {
    fn new() -> Self {
        let mut t = Self {
            arena: Vec::new(),
            offs: Vec::new(),
            lens: Vec::new(),
            slots: vec![0u64; 1 << 12],
            mask: (1 << 12) - 1,
        };
        t.slots.shrink_to_fit();
        t
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
        let tag = (h >> 32) as u32;
        let mut i = (h as usize) & self.mask;
        loop {
            let e = self.slots[i];
            if e == 0 {
                return i;
            }
            if (e >> 32) as u32 == tag {
                let t = (e as u32 - 1) as usize;
                let o = self.offs[t] as usize;
                let l = self.lens[t] as usize;
                if l == key.len() && self.arena[o..o + l] == *key {
                    return i;
                }
            }
            i = (i + 1) & self.mask;
        }
    }

    /// Look up bytes exactly as they appear after ASCII-lowercasing.
    #[inline]
    fn get(&self, key: &[u8]) -> Option<u32> {
        let h = hash_bytes(key);
        let e = self.slots[self.slot_for(key, h)];
        if e == 0 {
            None
        } else {
            Some(e as u32 - 1)
        }
    }

    fn intern(&mut self, key: &[u8]) -> u32 {
        let h = hash_bytes(key);
        let i = self.slot_for(key, h);
        let e = self.slots[i];
        if e != 0 {
            return e as u32 - 1;
        }
        let idx = self.offs.len() as u32;
        self.offs.push(self.arena.len() as u32);
        self.lens.push(key.len() as u32);
        self.arena.extend_from_slice(key);
        self.slots[i] = ((h >> 32) << 32) | (idx as u64 + 1);
        if self.offs.len() * 3 >= self.slots.len() * 2 {
            self.grow();
        }
        idx
    }

    fn grow(&mut self) {
        let cap = self.slots.len() * 2;
        let old = std::mem::replace(&mut self.slots, vec![0u64; cap]);
        self.mask = cap - 1;
        for e in old {
            if e == 0 {
                continue;
            }
            let t = (e as u32 - 1) as usize;
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

/// Mirrors the reference parser: split on single spaces, first word is the
/// first term, then `(op, term)` pairs.  That also reproduces its behaviour on
/// malformed lines (an operator-looking word in term position is just a term
/// that cannot match, because the index keys are lowercase).
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
                    while let Some(op) = words.next() {
                        let term = words.next().unwrap_or(b"");
                        let code = match op {
                            b"AND" => OP_AND,
                            b"OR" => OP_OR,
                            b"NOT" => OP_NOT,
                            // The reference raises here; skip the operation so a
                            // malformed line cannot take the whole run down.
                            _ => continue,
                        };
                        ops.push(code);
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
                    let terms: Vec<u32> =
                        inner.split(|&b| b == b' ').map(|t| tt.intern(t)).collect();
                    out.push(Query::Phrase(terms));
                } else {
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
fn index_text(text: &[u8], tt: &TermTable, postings: &mut [Vec<u32>], stamp: &mut [u32], doc: u32) {
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

/// One streaming pass building the postings.
fn build_index(
    path: &PathBuf,
    tt: &TermTable,
    postings: &mut [Vec<u32>],
    stamp: &mut [u32],
) -> io::Result<()> {
    let f = File::open(path)?;
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut line: Vec<u8> = Vec::with_capacity(2048);
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
        let (id, tstart) = match line[..end].iter().position(|&b| b == b'\t') {
            Some(t) => (parse_u32(&line[..t]), t + 1),
            None => (doc, end),
        };
        let text = &mut line[tstart..end];
        ascii_lower_in_place(text);
        index_text(text, tt, postings, stamp, id);
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
                break;
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
            if j == b.len() || b[j] != v {
                out.push(v);
            }
        }
    }
}

// ── phrase verification ────────────────────────────────────────────────

/// Does `text` (already lowercased) contain `terms` at `terms.len()`
/// consecutive token positions?
fn spans_contain(text: &[u8], spans: &[(u32, u32)], terms: &[&[u8]]) -> bool {
    let k = terms.len();
    if k == 0 {
        return true;
    }
    if spans.len() < k {
        return false;
    }
    let first = terms[0];
    'win: for w in 0..=(spans.len() - k) {
        let (s, l) = spans[w];
        if l as usize != first.len() || &text[s as usize..s as usize + l as usize] != first {
            continue;
        }
        for j in 1..k {
            let (s, l) = spans[w + j];
            let t = terms[j];
            if l as usize != t.len() || &text[s as usize..s as usize + l as usize] != t {
                continue 'win;
            }
        }
        return true;
    }
    false
}

/// Second pass: walk `docs.txt` front to back and verify every
/// `(document, phrase)` pair.  `pairs` is sorted by document id, so this never
/// seeks; it stops as soon as the last pair has been looked at.
fn verify_phrases(
    path: &PathBuf,
    pairs: &[u64],
    queries: &[Query],
    tt: &TermTable,
    hits: &mut [Vec<u32>],
) -> io::Result<()> {
    if pairs.is_empty() {
        return Ok(());
    }
    let f = File::open(path)?;
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut line: Vec<u8> = Vec::with_capacity(2048);
    let mut spans: Vec<(u32, u32)> = Vec::with_capacity(256);
    let mut terms: Vec<&[u8]> = Vec::with_capacity(8);
    let mut pi = 0usize;
    let mut doc: u32 = 0;
    loop {
        if pi >= pairs.len() {
            break;
        }
        line.clear();
        let n = r.read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        let mut end = n;
        if line[end - 1] == b'\n' {
            end -= 1;
        }
        let (id, tstart) = match line[..end].iter().position(|&b| b == b'\t') {
            Some(t) => (parse_u32(&line[..t]), t + 1),
            None => (doc, end),
        };
        doc += 1;
        while pi < pairs.len() && ((pairs[pi] >> 32) as u32) < id {
            pi += 1;
        }
        if pi >= pairs.len() {
            break;
        }
        if (pairs[pi] >> 32) as u32 != id {
            continue;
        }
        let text = &mut line[tstart..end];
        ascii_lower_in_place(text);
        tokenize_spans(text, &mut spans);
        loop {
            let qi = (pairs[pi] & 0xffff_ffff) as usize;
            if let Query::Phrase(ts) = &queries[qi] {
                terms.clear();
                terms.extend(ts.iter().map(|&t| tt.key(t)));
                if spans_contain(text, &spans, &terms) {
                    hits[qi].push(id);
                }
            }
            pi += 1;
            if pi >= pairs.len() || (pairs[pi] >> 32) as u32 != id {
                break;
            }
        }
    }
    Ok(())
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

/// Left-to-right, no precedence: `a OR b AND c` is `(a OR b) AND c`.
#[inline]
fn eval_bool(
    terms: &[u32],
    ops: &[u8],
    postings: &[Vec<u32>],
    cur: &mut Vec<u32>,
    nxt: &mut Vec<u32>,
) {
    cur.clear();
    cur.extend_from_slice(&postings[terms[0] as usize]);
    for (i, &op) in ops.iter().enumerate() {
        let s = &postings[terms[i + 1] as usize];
        match op {
            OP_AND => {
                if cur.is_empty() {
                    continue;
                }
                intersect_into(cur, s, nxt);
            }
            OP_OR => {
                if s.is_empty() {
                    continue;
                }
                union_into(cur, s, nxt);
            }
            _ => {
                if cur.is_empty() || s.is_empty() {
                    continue;
                }
                difference_into(cur, s, nxt);
            }
        }
        std::mem::swap(cur, nxt);
        if cur.is_empty() {
            // Empty survives AND and NOT; only a later OR can fill it again.
            if !ops[i + 1..].iter().any(|&o| o == OP_OR) {
                break;
            }
        }
    }
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
    if let Err(e) = build_index(&docs_path, &tt, &mut postings, &mut stamp) {
        eprintln!("[track-{TRACK}] cannot index {}: {e}", docs_path.display());
        return;
    }
    drop(stamp);
    lap!("build index");

    // Phrase candidates, gathered as `(document << 32) | query index` so the
    // verification pass can walk the corpus in document order.
    let mut pairs: Vec<u64> = Vec::with_capacity(1 << 18);
    let mut hits: Vec<Vec<u32>> = vec![Vec::new(); queries.len()];
    let mut cand: Vec<u32> = Vec::new();
    let mut nxt: Vec<u32> = Vec::new();
    let mut order: Vec<usize> = Vec::new();
    for (qi, q) in queries.iter().enumerate() {
        if let Query::Phrase(terms) = q {
            // Rarest list first; intersection is commutative, so the candidate
            // set is exactly the reference's.
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
            for &d in cand.iter() {
                pairs.push(((d as u64) << 32) | qi as u64);
            }
        }
    }
    lap!("phrase candidates");
    pairs.sort_unstable();

    if let Err(e) = verify_phrases(&docs_path, &pairs, &queries, &tt, &mut hits) {
        eprintln!("[track-{TRACK}] cannot verify phrases: {e}");
    }
    drop(pairs);
    drop(cand);
    drop(nxt);
    drop(order);
    lap!("phrase verify");

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

    let mut line: Vec<u8> = Vec::with_capacity(256);
    let mut cur: Vec<u32> = Vec::new();
    let mut nxt: Vec<u32> = Vec::new();
    let mut io_err = false;

    for (qi, q) in queries.iter().enumerate() {
        let r: &[u32] = match q {
            Query::Bool { terms, ops } => {
                eval_bool(terms, ops, &postings, &mut cur, &mut nxt);
                &cur
            }
            Query::Phrase(_) => &hits[qi],
        };
        format_result(&mut line, r);
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
    lap!("write");
}
