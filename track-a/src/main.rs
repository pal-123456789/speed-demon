//! Track A — Ingest.
//!
//! Group 3.1M log lines by (hour, endpoint) and emit, per group: the request
//! count, the p50/p95/p99 latency by nearest rank, the 5xx count and the error
//! rate in integer parts per million.
//!
//! Design notes, in the order they matter:
//!
//! * **The hour is the first 13 bytes.** `YYYY-MM-DDTHH` is a prefix of the
//!   RFC 3339 timestamp and the field is fixed width, so byte order is
//!   chronological order and grouping needs no calendar arithmetic at all. This
//!   is why the early design's days-from-civil conversion is not needed.
//!
//! * **No bound on disorder is promised.** The spec says eight shards write in
//!   time order and are interleaved, and that the shard is not a field. So a
//!   "previous line" proves nothing about the next one and no monotonicity
//!   shortcut is used: every line is grouped on its own data.
//!
//! * **Percentiles are selected, not sorted.** Nearest rank `ceil(p*n/100)` is
//!   `(p*n + 99) / 100` in integers, and `select_nth_unstable` puts the k-th
//!   smallest at index k, so three calls give the three ranks in O(n) each
//!   instead of an O(n log n) sort. p99 is taken first, then p95, then p50, so
//!   each call narrows.
//!
//! * **Interning, not per-line allocation.** Endpoints are interned by their
//!   bytes on first sight (1,842 distinct) and hours by their 13-byte key (72
//!   distinct), so the hot loop allocates nothing. Groups are keyed by the packed
//!   (hour id, endpoint id) pair in one u64.
//!
//! * **Streamed, not slurped.** A 1 MiB buffered reader with a reused line
//!   buffer, so peak memory is the latency arena rather than the input file.
//!
//! * **Output order is total.** Rows are sorted by hour then by endpoint as
//!   unsigned bytes, so the answer has exactly one SHA-256 regardless of any hash
//!   iteration order.

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::exit;

/// One (hour, endpoint) group: its latencies and its 5xx count.
struct Group {
    latencies: Vec<u32>,
    errors: u32,
}

/// Index of the next space at or after `from`, or `None`.
#[inline]
fn next_space(buf: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < buf.len() {
        if buf[i] == b' ' {
            return Some(i);
        }
        i += 1;
    }
    None
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

fn main() {
    let data = env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());
    let input = format!("{data}/requests.log");
    let output = format!("{outd}/summary.txt");

    // ── intern tables ───────────────────────────────────────────────────────
    let mut hours: Vec<[u8; 13]> = Vec::with_capacity(72);
    let mut hour_ids: HashMap<[u8; 13], u32> = HashMap::with_capacity(72);
    let mut endpoints: Vec<Vec<u8>> = Vec::with_capacity(2048);
    let mut endpoint_ids: HashMap<Vec<u8>, u32> = HashMap::with_capacity(2048);
    let mut groups: HashMap<u64, Group> = HashMap::with_capacity(4096);

    let file = match File::open(&input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[track-a] cannot open {input}: {e}");
            exit(1);
        }
    };

    // ── one pass ────────────────────────────────────────────────────────────
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut line: Vec<u8> = Vec::with_capacity(192);
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("[track-a] read error: {e}");
                exit(1);
            }
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        // The grammar promises exactly 27 bytes of timestamp, then a space.
        if line.len() < 32 {
            continue;
        }

        // Field boundaries: SP after the method, then SP after the endpoint.
        let s1 = match next_space(&line, 28) {
            Some(i) => i,
            None => continue,
        };
        let s2 = match next_space(&line, s1 + 1) {
            Some(i) => i,
            None => continue,
        };
        if line.len() < s2 + 5 {
            continue;
        }

        let st = &line[s2 + 1..s2 + 4];
        let status =
            (st[0] - b'0') as u32 * 100 + (st[1] - b'0') as u32 * 10 + (st[2] - b'0') as u32;
        let latency = parse_u32(&line[s2 + 5..]);

        let mut hour_key = [0u8; 13];
        hour_key.copy_from_slice(&line[0..13]);
        let hour_id = match hour_ids.get(&hour_key) {
            Some(v) => *v,
            None => {
                let v = hours.len() as u32;
                hours.push(hour_key);
                hour_ids.insert(hour_key, v);
                v
            }
        };

        let endpoint_bytes = &line[s1 + 1..s2];
        let endpoint_id = match endpoint_ids.get(endpoint_bytes) {
            Some(v) => *v,
            None => {
                let v = endpoints.len() as u32;
                endpoints.push(endpoint_bytes.to_vec());
                endpoint_ids.insert(endpoint_bytes.to_vec(), v);
                v
            }
        };

        let key = ((hour_id as u64) << 32) | endpoint_id as u64;
        let group = groups.entry(key).or_insert_with(|| Group {
            latencies: Vec::new(),
            errors: 0,
        });
        group.latencies.push(latency);
        if (500..=599).contains(&status) {
            group.errors += 1;
        }
    }

    // ── statistics, then a total order ──────────────────────────────────────
    // Rows hold INTERNED IDS, never moved-out byte vectors. An earlier version
    // did `mem::take` on the endpoint table, so the first group to touch an
    // endpoint emptied it and every later group for that endpoint -- it appears
    // once per hour, up to 72 times -- was written with an empty endpoint. Which
    // group got the bytes depended on HashMap iteration order, which is seeded
    // randomly per process, so the binary produced a DIFFERENT answer on every
    // run: five runs, five hashes, a hard gate failure. The intern tables are the
    // single source of truth for the bytes and are only read here.
    let mut rows: Vec<(u32, u32, u64, u32, u32, u32, u32, u64)> = Vec::with_capacity(groups.len());

    for (key, mut group) in groups {
        let hour_id = (key >> 32) as u32;
        let endpoint_id = (key & 0xffff_ffff) as u32;
        let n = group.latencies.len() as u64;

        // Nearest rank, 1-indexed: rank(p) = ceil(p*n/100) = (p*n + 99)/100.
        let rank0 = |p: u64| -> usize {
            let r = (p * n + 99) / 100;
            (if r < 1 { 1 } else { r }) as usize - 1
        };
        let (t50, t95, t99) = (rank0(50), rank0(95), rank0(99));
        let p99 = *group.latencies.select_nth_unstable(t99).1;
        let p95 = *group.latencies.select_nth_unstable(t95).1;
        let p50 = *group.latencies.select_nth_unstable(t50).1;

        let ppm = group.errors as u64 * 1_000_000 / n;
        rows.push((hour_id, endpoint_id, n, p50, p95, p99, group.errors, ppm));
    }

    rows.sort_unstable_by(|a, b| {
        hours[a.0 as usize]
            .cmp(&hours[b.0 as usize])
            .then_with(|| endpoints[a.1 as usize].cmp(&endpoints[b.1 as usize]))
    });

    // ── write exactly one file ──────────────────────────────────────────────
    let out_file = match File::create(&output) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[track-a] cannot create {output}: {e}");
            exit(1);
        }
    };
    let mut w = BufWriter::with_capacity(1 << 20, out_file);
    for (hour_id, endpoint_id, count, p50, p95, p99, errors, ppm) in &rows {
        let hour = &hours[*hour_id as usize];
        let endpoint = &endpoints[*endpoint_id as usize];
        let r = w
            .write_all(&hour[..])
            .and_then(|_| w.write_all(b":00:00Z "))
            .and_then(|_| w.write_all(endpoint))
            .and_then(|_| write!(w, " {count} {p50} {p95} {p99} {errors} {ppm}\n"));
        if let Err(e) = r {
            eprintln!("[track-a] write error: {e}");
            exit(1);
        }
    }
    if let Err(e) = w.flush() {
        eprintln!("[track-a] flush error: {e}");
        exit(1);
    }
}
