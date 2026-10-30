//! Track D — Traverse.
//!
//! A directed weighted graph (5,000,000 edges, ~1.74M nodes) and 2,000
//! source-target shortest-path queries. Preprocessing is inside the timed run.
//!
//! Design notes:
//!
//! * **u64 distances, not u32.** The weights run to `2^32 - 1` and the spec's own
//!   worked example answers `8589934594`. A u32 label would be correct only if
//!   `(n - 1) * w_max < 2^32`, i.e. `w_max <= 2468` at n = 1,740,512 -- and the
//!   generator deliberately injects weights near `2^32`. So labels are u64, and
//!   every sum is computed in u64. A u32 label is a silent wrong answer on this
//!   dataset, which is the worst failure mode available.
//!
//! * **CSR, built by the whole thread pool.** Forward-star adjacency with
//!   linked lists is cache-hostile; a CSR is one linear scan per node expansion.
//!   The graph file is strictly regular -- a header, then exactly `m` lines of
//!   `u SP v SP w LF` -- so it splits at line boundaries into `NWORKERS` byte
//!   windows and both build passes run in parallel. Parallel edges and exact
//!   duplicate lines need no special handling: Dijkstra relaxes both and the
//!   lighter one wins. Self-loops never shorten a path for the same reason.
//!
//! * **One 8-byte edge record, not two 4-byte arrays.** `v` and `w` share a
//!   single `u64` (`v` low, `w` high). Same 40 MB as `head[] + weight[]`, but an
//!   expansion touches one cache line per edge instead of two, in both the fill
//!   pass and every Dijkstra relaxation.
//!
//! * **The parallel build does not depend on edge order.** Workers claim slots
//!   inside a node's block with `fetch_sub`, so the adjacency order within a
//!   block is whichever worker got there first -- not fixed from run to run. It
//!   does not matter: shortest-path *distances* are a function of the edge
//!   multiset alone, and Dijkstra is correct for every adjacency order. Only
//!   distances are written out, so the output is byte-identical across runs --
//!   which the SHA-256 gate checks on every run rather than assuming.
//!
//! * **Reset by touched list, never by memset.** Clearing 1.74M distances per
//!   query would dominate the run; only the nodes actually relaxed are restored
//!   to infinity afterwards.
//!
//! * **Early exit at t.** The reference searches forward from `s` only and
//!   settles everything closer than `t`, so stopping the moment `t` is popped is
//!   a pure win with no change in the answer.
//!
//! * A path may have zero edges, so `d(s, s) = 0` for every node, including a
//!   node with no edges at all -- that case is answered before any search.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::env;
use std::fs::{File, read};
use std::io::Write;
use std::process::exit;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

const INF: u64 = u64::MAX;

/// Hardware threads the scoring container is pinned to (six P-cores, SMT on).
/// The query phase already used this split; the parse now uses the same pool.
const NWORKERS: usize = 12;

/// Next unsigned decimal integer in `buf` starting at `*pos`, skipping any
/// non-digit bytes but never reading at or beyond `lim`. The grammar is
/// guaranteed, so this does not validate.
#[inline(always)]
fn next_int(buf: &[u8], pos: &mut usize, lim: usize) -> u64 {
    let mut p = *pos;
    while p < lim && !buf[p].is_ascii_digit() {
        p += 1;
    }
    let mut v: u64 = 0;
    while p < lim {
        let d = buf[p].wrapping_sub(b'0');
        if d > 9 {
            break;
        }
        v = v * 10 + d as u64;
        p += 1;
    }
    *pos = p;
    v
}

/// Byte offsets at which the workers' parse windows start. `starts[0]` is the
/// first edge byte and `starts[NWORKERS]` is end of file, so the windows
/// partition the `m` edge lines: a line belongs to the worker whose window
/// holds its first byte, and no line is parsed twice or skipped. Every interior
/// boundary is the first byte after a `LF`, which is what makes a window safe
/// to parse in isolation.
fn chunk_bounds(graph: &[u8], body: usize) -> [usize; NWORKERS + 1] {
    let len = graph.len();
    let mut starts = [len; NWORKERS + 1];
    starts[0] = body;
    for c in 1..NWORKERS {
        let mut s = len * c / NWORKERS;
        if s < body {
            s = body;
        }
        while s < len && graph[s] != b'\n' {
            s += 1;
        }
        if s < len {
            s += 1;
        }
        if s < starts[c - 1] {
            s = starts[c - 1]; // defensive: boundaries are non-decreasing
        }
        starts[c] = s;
    }
    starts
}

/// Walk the edge lines of window `[lo, hi)`, calling `body(u, v, w)` once per
/// line. Bytes at or after `hi` are never read, so a window cannot see into its
/// neighbour's first line.
#[inline(always)]
fn for_each_edge<F: FnMut(usize, u32, u32)>(graph: &[u8], lo: usize, hi: usize, mut body: F) {
    let mut p = lo;
    while p < hi {
        let u = next_int(graph, &mut p, hi) as usize;
        let v = next_int(graph, &mut p, hi) as u32;
        let w = next_int(graph, &mut p, hi) as u32;
        body(u, v, w);
        // Consume the rest of the line, including its LF.
        while p < hi && graph[p] != b'\n' {
            p += 1;
        }
        p += 1;
    }
}

fn main() {
    let data = env::var("SD_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let outd = env::var("SD_OUT_DIR").unwrap_or_else(|_| "/out".to_string());

    let graph = match read(format!("{data}/graph.txt")) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("[track-d] cannot read graph.txt: {e}");
            exit(1);
        }
    };

    let len = graph.len();
    let mut pos = 0usize;
    let n = next_int(&graph, &mut pos, len) as usize;
    let m = next_int(&graph, &mut pos, len) as usize;
    // `body` must be the first byte of the first edge line, not the LF that ends
    // the header. It doubles as the first parse-window boundary below, and
    // `next_int` tolerates a leading LF -- but a window that both starts *and*
    // ends on that one byte would parse a phantom `0 0 0` edge out of it.
    let mut body = pos;
    while body < len && graph[body] != b'\n' {
        body += 1;
    }
    if body < len {
        body += 1;
    }
    let starts = chunk_bounds(&graph, body);

    // ── pass 1, parallel: out-degree per node ───────────────────────────────
    // One counter per node, incremented by whichever worker parses the line.
    // The counters are reused as the CSR write cursors in pass 2, so the build
    // needs no second array and no per-worker degree tables.
    let degree: Vec<AtomicU32> = (0..n).map(|_| AtomicU32::new(0)).collect();
    std::thread::scope(|scope| {
        for w in 0..NWORKERS {
            let (lo, hi) = (starts[w], starts[w + 1]);
            let graph = &graph;
            let degree = &degree;
            scope.spawn(move || {
                for_each_edge(graph, lo, hi, |u, _v, _w| {
                    degree[u].fetch_add(1, Ordering::Relaxed);
                });
            });
        }
    });

    // ── prefix sum: CSR start offset per node ───────────────────────────────
    let mut offset = vec![0u32; n + 1];
    let mut acc = 0u32;
    for i in 0..n {
        offset[i] = acc;
        acc += degree[i].load(Ordering::Relaxed);
    }
    offset[n] = acc;
    let total = acc as usize;

    // The windows partition the file's lines exactly, so the counter total is
    // the header's `m`. If that ever stopped holding, the CSR would be built
    // from a truncated edge set and the answers would be quietly wrong: fail
    // loudly instead. It is a two-integer comparison on the hot path of nothing.
    if total != m {
        eprintln!("[track-d] parsed {total} edges but graph.txt declares {m}");
        exit(1);
    }

    // ── pass 2, parallel: fill the CSR ──────────────────────────────────────
    // The degree counter doubles as the write cursor and counts *down*: the
    // first worker to reach node `u` gets `offset[u + 1] - c`, the last gets
    // `offset[u]`, so every edge lands inside `u`'s own block and workers never
    // collide. Counting down is what lets pass 1 keep the counter, so the prefix
    // sum stays a plain load-and-add loop over 1.74M nodes instead of an atomic
    // read-modify-write per node.
    let edge: Vec<AtomicU64> = (0..total).map(|_| AtomicU64::new(0)).collect();
    std::thread::scope(|scope| {
        for w in 0..NWORKERS {
            let (lo, hi) = (starts[w], starts[w + 1]);
            let graph = &graph;
            let degree = &degree;
            let offset = &offset;
            let edge = &edge;
            scope.spawn(move || {
                for_each_edge(graph, lo, hi, |u, v, wt| {
                    let slot =
                        offset[u + 1] as usize - degree[u].fetch_sub(1, Ordering::Relaxed) as usize;
                    edge[slot].store(v as u64 | (wt as u64) << 32, Ordering::Relaxed);
                });
            });
        }
    });

    // The graph bytes are no longer needed; dropping them keeps peak memory at
    // the CSR plus the per-worker distance arrays rather than adding 92 MB on
    // top of them. (The Vec is a large mmap allocation, so it really is
    // returned to the OS here.)
    drop(graph);

    // ── queries ─────────────────────────────────────────────────────────────
    let queries = match read(format!("{data}/queries.txt")) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("[track-d] cannot read queries.txt: {e}");
            exit(1);
        }
    };

    // ── parse the queries up front (2,000 of them: trivial to hold) ─────────
    let mut qs: Vec<(u32, u32)> = Vec::with_capacity(2200);
    {
        let qlen = queries.len();
        let mut qp = 0usize;
        while qp < qlen {
            while qp < qlen && !queries[qp].is_ascii_digit() {
                qp += 1;
            }
            if qp >= qlen {
                break;
            }
            let s = next_int(&queries, &mut qp, qlen) as u32;
            let t = next_int(&queries, &mut qp, qlen) as u32;
            qs.push((s, t));
        }
    }
    drop(queries);

    // ── answer them in parallel ─────────────────────────────────────────────
    // The queries are mutually independent and the graph is read-only, so this
    // is the one place in the submission where threads cost almost nothing: no
    // locks, no shared writes, and the output order is preserved by collecting
    // each chunk's bytes in order. The container is pinned to twelve hardware
    // threads on six P-cores, which is exactly the split below.
    //
    // Each thread owns its own distance array, because 1.74M u64 labels cannot
    // be shared without synchronisation. That is the only cost: 12 x 14 MB of
    // working state, against a search phase that was 97% of the reference's time.
    let nthreads = NWORKERS.min(qs.len().max(1));
    let chunk = qs.len().div_ceil(nthreads).max(1);
    let out_cap = qs.len() * 6 + 16;

    let mut parts: Vec<Vec<u8>> = Vec::with_capacity(nthreads);
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(nthreads);
        for part in qs.chunks(chunk) {
            let offset = &offset;
            let edge = &edge;
            handles.push(scope.spawn(move || {
                let mut dist = vec![INF; n];
                let mut touched: Vec<u32> = Vec::with_capacity(4096);
                let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::with_capacity(1 << 14);
                let mut out: Vec<u8> = Vec::with_capacity(part.len() * 6);

                for &(s, t) in part {
                    let answer: u64 = if s == t {
                        0 // zero-edge path, even on a node with no edges
                    } else {
                        dist[s as usize] = 0;
                        touched.push(s);
                        heap.clear();
                        heap.push(Reverse((0u64, s)));
                        let mut found = INF;
                        while let Some(Reverse((d, u))) = heap.pop() {
                            if d > dist[u as usize] {
                                continue; // stale entry
                            }
                            if u == t {
                                found = d;
                                break; // everything still queued is at least this far
                            }
                            let lo = offset[u as usize] as usize;
                            let hi = offset[u as usize + 1] as usize;
                            for e in lo..hi {
                                let ev = edge[e].load(Ordering::Relaxed);
                                let v = ev as u32;
                                let nd = d + (ev >> 32) as u32 as u64;
                                if nd < dist[v as usize] {
                                    dist[v as usize] = nd;
                                    touched.push(v);
                                    heap.push(Reverse((nd, v)));
                                }
                            }
                        }
                        for &x in &touched {
                            dist[x as usize] = INF;
                        }
                        touched.clear();
                        found
                    };

                    if answer == INF {
                        out.extend_from_slice(b"-1\n");
                    } else {
                        let _ = write!(out, "{answer}\n");
                    }
                }
                out
            }));
        }
        for h in handles {
            parts.push(h.join().unwrap_or_default());
        }
    });

    let mut out: Vec<u8> = Vec::with_capacity(out_cap);
    for p in parts {
        out.extend_from_slice(&p);
    }

    if let Err(e) =
        File::create(format!("{outd}/distances.txt")).and_then(|mut f| f.write_all(&out))
    {
        eprintln!("[track-d] cannot write distances.txt: {e}");
        exit(1);
    }
}
