//! Track C — Join.
//!
//! Parse a hostile CSV (quoted commas, `""` escapes, embedded newlines, mixed
//! CRLF/LF terminators, U+FEFF at some record boundaries), join each record
//! against a region lookup, and emit one sorted line of aggregates per country.
//!
//! The organizers call this the correctness track and say most disqualifications
//! happen here, so the parser follows spec section 2.1 literally and does nothing
//! clever:
//!
//! * A field starting with `"` ends at the next `"` that is not part of a `""`
//!   pair. Inside quotes every other byte is data, including `,`, LF and U+FEFF.
//! * A U+FEFF at the very start of the file, or immediately after a record
//!   terminator, is a byte order mark and is skipped. Anywhere else it is data.
//!   (An unquoted field can never contain one, because a mid-file BOM always sits
//!   at the start of a record.)
//! * A record terminator is a CRLF or a bare LF **outside quotes**.
//! * Money is exact integer cents. `-5.25` is -525. No float ever touches the
//!   value or the output, so `-0.00` cannot be produced even by accident.
//! * `names_distinct` compares exact byte strings after unescaping, so `Betts` and
//!   `Betts ` are two values, and the empty unquoted name and `""` are one.
//!
//! Memory note: the whole file is held, because the parse is a linear scan and
//! re-reading costs more than the 202 MB it occupies. Names are only copied when
//! they are new to their country's set, so distinct-name storage is bounded by the
//! ~100,000-value pool rather than by 3.32M records.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{File, read};
use std::io::{Read, Write};
use std::process::exit;

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Per-country running aggregate.
#[derive(Default)]
struct Agg {
    count: u64,
    id_sum: i64,
    amount_sum: i64,
    amount_min: i64,
    amount_max: i64,
    names: HashSet<Vec<u8>>,
    name_bytes: u64,
    note_bytes: u64,
}

impl Agg {
    fn new() -> Self {
        Agg {
            amount_min: i64::MAX,
            amount_max: i64::MIN,
            ..Default::default()
        }
    }
}

/// Read one field at `*pos` into `out`, honouring the quoting rules.
fn field(buf: &[u8], pos: &mut usize, out: &mut Vec<u8>) {
    out.clear();
    if *pos < buf.len() && buf[*pos] == b'"' {
        *pos += 1;
        while *pos < buf.len() {
            let b = buf[*pos];
            if b == b'"' {
                if *pos + 1 < buf.len() && buf[*pos + 1] == b'"' {
                    out.push(b'"'); // `""` is one literal quote
                    *pos += 2;
                    continue;
                }
                *pos += 1; // closing quote
                break;
            }
            out.push(b);
            *pos += 1;
        }
    } else {
        while *pos < buf.len() {
            let b = buf[*pos];
            if b == b',' || b == b'\n' || b == b'\r' {
                break;
            }
            out.push(b);
            *pos += 1;
        }
    }
}

/// Unsigned decimal, no leading zeros in the data.
#[inline]
fn digits(buf: &[u8], pos: &mut usize) -> i64 {
    let mut v: i64 = 0;
    while *pos < buf.len() && buf[*pos].is_ascii_digit() {
        v = v * 10 + (buf[*pos] - b'0') as i64;
        *pos += 1;
    }
    v
}

/// Exact cents as `-?int.dd`, never `-0.00`.
fn money(cents: i64, out: &mut Vec<u8>) {
    let neg = cents < 0;
    let a = cents.unsigned_abs();
    if neg {
        out.push(b'-');
    }
    let _ = write!(out, "{}.{:02}", a / 100, a % 100);
}

fn main() {
    let data = env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());

    // ── the lookup table ────────────────────────────────────────────────────
    let regions = match read(format!("{data}/regions.csv")) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[track-c] cannot read regions.csv: {e}");
            exit(1);
        }
    };
    let mut lookup: HashMap<i64, [u8; 3]> = HashMap::with_capacity(200_000);
    {
        let mut p = 0usize;
        // header
        while p < regions.len() && regions[p] != b'\n' {
            p += 1;
        }
        while p < regions.len() {
            p += 1; // step over the newline
            if p >= regions.len() {
                break;
            }
            let rid = digits(&regions, &mut p);
            if p < regions.len() && regions[p] == b',' {
                p += 1;
            }
            if p + 2 < regions.len() {
                lookup.insert(rid, [regions[p], regions[p + 1], regions[p + 2]]);
                p += 3;
            }
            while p < regions.len() && regions[p] != b'\n' {
                p += 1;
            }
        }
    }

    // ── the hostile file, STREAMED ──────────────────────────────────────────
    // Never held whole. Reading the 202 MB file into a heap buffer costs 202 MB
    // of cgroup memory for a file the host has already put in page cache, and
    // the board's M column showed it: 422 MB reported against ~220 MB of live
    // data, worse than the naive Python reference's 367 MB. Chunked reads keep
    // the peak at one chunk plus the aggregates.
    // 32 MiB. The buffer is the only thing standing between a 202 MB heap copy
    // and the aggregates, so it is the whole memory win; making it small bought
    // nothing and cost real time (4 MiB measured 6.58 s against 4.42 s for the
    // single slurp), because every chunk pays a read syscall and a memmove. At
    // 32 MiB there are seven chunks for a 202 MB file and the parse loop streams
    // at full speed.
    const CHUNK: usize = 32 << 20;
    let mut file = match File::open(format!("{data}/records.csv")) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[track-c] cannot read records.csv: {e}");
            exit(1);
        }
    };
    let mut buf: Vec<u8> = vec![0u8; CHUNK + (1 << 20)];
    let mut have = 0usize;
    let mut pos = 0usize;
    let mut eof = false;
    let mut header_done = false;

    let mut groups: HashMap<[u8; 3], Agg> = HashMap::with_capacity(256);
    let mut unmatched = Agg::new();
    let mut saw_unmatched = false;

    let mut name: Vec<u8> = Vec::with_capacity(64);
    let mut note: Vec<u8> = Vec::with_capacity(64);

    loop {
        // carry the unparsed tail to the front, then refill
        if pos > 0 {
            buf.copy_within(pos..have, 0);
            have -= pos;
            pos = 0;
        }
        if !eof {
            match file.read(&mut buf[have..]) {
                Ok(0) => eof = true,
                Ok(k) => have += k,
                Err(e) => {
                    eprintln!("[track-c] read error: {e}");
                    exit(1);
                }
            }
        }

        if !header_done {
            if have == 0 && eof {
                break;
            }
            let mut i = 0usize;
            if buf[..have].starts_with(&BOM) {
                i = 3;
            }
            while i < have && buf[i] != b'\n' {
                i += 1;
            }
            if i < have {
                header_done = true;
                pos = i + 1;
            } else if eof {
                break; // no header line at all
            } else {
                continue; // the header straddles the chunk boundary
            }
        }

        // Single pass. Parsing optimistically and rolling back the ONE record
        // that straddles the chunk edge is cheaper than scanning every byte
        // twice to find a safe boundary first -- and the arithmetic is why it
        // matters: T carries a 0.5 exponent and M only 0.2, so a 44% rise in T
        // costs more than a 48% fall in M gains. Measured: the two-pass version
        // went 4.42 s -> 7.06 s on T, a net loss on K.
        let recs: &[u8] = &buf[..have];
        loop {
            if pos >= have {
                break;
            }
            let start = pos;

            // a BOM immediately after a terminator belongs to the record
            if recs[pos..].starts_with(&BOM) {
                if pos + 3 > have && !eof {
                    pos = start; // cannot tell yet: the BOM straddles the edge
                    break;
                }
                pos += 3;
                if pos >= have {
                    pos = start;
                    break;
                }
            }

            let id = digits(recs, &mut pos);
            if pos < have && recs[pos] == b',' {
                pos += 1;
            }
            field(recs, &mut pos, &mut name);
            if pos < have && recs[pos] == b',' {
                pos += 1;
            }
            field(recs, &mut pos, &mut note);
            if pos < have && recs[pos] == b',' {
                pos += 1;
            }
            let region = digits(recs, &mut pos);
            if pos < have && recs[pos] == b',' {
                pos += 1;
            }
            let mut neg = false;
            if pos < have && recs[pos] == b'-' {
                neg = true;
                pos += 1;
            }
            let mut cents = digits(recs, &mut pos) * 100;
            if pos + 1 < have && recs[pos] == b'.' {
                pos += 1;
                let d1 = (recs[pos] - b'0') as i64;
                let d2 = (recs[pos + 1] - b'0') as i64;
                pos += 2;
                cents += d1 * 10 + d2;
            }
            if neg {
                cents = -cents;
            }

            // The terminator is the proof that the record was complete. Without
            // one the buffer ended mid-record -- in a field, in a quoted value,
            // or between the CR and its LF -- so discard the partial parse and
            // wait for the next chunk. Truncation anywhere always lands here,
            // because every other way of ending a record sets this flag.
            let mut terminated = false;
            if pos < have && recs[pos] == b'\r' {
                pos += 1;
            }
            if pos < have && recs[pos] == b'\n' {
                pos += 1;
                terminated = true;
            }
            if !terminated && !eof {
                pos = start;
                break;
            }

            // ── join and accumulate ─────────────────────────────────────────
            let agg = match lookup.get(&region) {
                Some(code) => groups.entry(*code).or_insert_with(Agg::new),
                None => {
                    saw_unmatched = true;
                    &mut unmatched
                }
            };
            agg.count += 1;
            agg.id_sum += id;
            agg.amount_sum += cents;
            if cents < agg.amount_min {
                agg.amount_min = cents;
            }
            if cents > agg.amount_max {
                agg.amount_max = cents;
            }
            agg.name_bytes += name.len() as u64;
            agg.note_bytes += note.len() as u64;
            if !agg.names.contains(&name[..]) {
                agg.names.insert(name.clone());
            }
        }

        if eof && pos >= have {
            break;
        }
        // Progress is impossible only when a FULL buffer holds no terminator at
        // all, which would mean one record larger than the buffer.
        if !eof && pos == 0 && have == buf.len() {
            eprintln!(
                "[track-c] no record terminator in a full {} byte buffer — a single record \
                 cannot be that large; the input is outside the contract",
                buf.len()
            );
            exit(1);
        }
    }

    // ── output, sorted bytewise, `_UNMATCHED` last by construction ──────────
    let mut keys: Vec<[u8; 3]> = groups.keys().copied().collect();
    keys.sort_unstable();

    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    for k in keys {
        let a = &groups[&k];
        out.extend_from_slice(&k);
        let _ = write!(
            out,
            " {} {} ",
            a.count, a.id_sum
        );
        money(a.amount_sum, &mut out);
        out.push(b' ');
        money(a.amount_min, &mut out);
        out.push(b' ');
        money(a.amount_max, &mut out);
        let _ = write!(
            out,
            " {} {} {}\n",
            a.names.len(),
            a.name_bytes,
            a.note_bytes
        );
    }
    if saw_unmatched {
        out.extend_from_slice(b"_UNMATCHED ");
        let a = &unmatched;
        let _ = write!(out, "{} {} ", a.count, a.id_sum);
        money(a.amount_sum, &mut out);
        out.push(b' ');
        money(a.amount_min, &mut out);
        out.push(b' ');
        money(a.amount_max, &mut out);
        let _ = write!(
            out,
            " {} {} {}\n",
            a.names.len(),
            a.name_bytes,
            a.note_bytes
        );
    }

    if let Err(e) =
        File::create(format!("{outd}/aggregates.txt")).and_then(|mut f| f.write_all(&out))
    {
        eprintln!("[track-c] cannot write aggregates.txt: {e}");
        exit(1);
    }
}
