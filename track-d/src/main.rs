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
//! * **One CSR, built in two counting passes.** Forward-star adjacency with
//!   linked lists is cache-hostile; a CSR over `(head, weight)` is one linear
//!   scan per node expansion. Parallel edges and exact duplicate lines need no
//!   special handling: Dijkstra relaxes both and the lighter one wins. Self-loops
//!   never shorten a path for the same reason.
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

const INF: u64 = u64::MAX;

/// Next unsigned decimal integer in `buf` starting at `*pos`, skipping any
/// non-digit bytes. The grammar is guaranteed, so this does not validate.
#[inline]
fn next_int(buf: &[u8], pos: &mut usize) -> u64 {
    while *pos < buf.len() && !buf[*pos].is_ascii_digit() {
        *pos += 1;
    }
    let mut v: u64 = 0;
    while *pos < buf.len() {
        let b = buf[*pos];
        if !b.is_ascii_digit() {
            break;
        }
        v = v * 10 + (b - b'0') as u64;
        *pos += 1;
    }
    v
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

    let mut pos = 0usize;
    let n = next_int(&graph, &mut pos) as usize;
    let m = next_int(&graph, &mut pos) as usize;
    let body = pos; // first edge byte, reused by both counting passes

    // ── pass 1: out-degree per node ─────────────────────────────────────────
    let mut offset = vec![0u32; n + 1];
    {
        let mut p = body;
        for _ in 0..m {
            let u = next_int(&graph, &mut p) as usize;
            let _v = next_int(&graph, &mut p);
            let _w = next_int(&graph, &mut p);
            offset[u + 1] += 1;
        }
    }
    for i in 0..n {
        offset[i + 1] += offset[i];
    }

    // ── pass 2: fill the CSR ────────────────────────────────────────────────
    let mut head = vec![0u32; m];
    let mut weight = vec![0u32; m];
    let mut cursor = offset.clone();
    {
        let mut p = body;
        for _ in 0..m {
            let u = next_int(&graph, &mut p) as usize;
            let v = next_int(&graph, &mut p) as u32;
            let w = next_int(&graph, &mut p) as u32;
            let slot = cursor[u] as usize;
            head[slot] = v;
            weight[slot] = w;
            cursor[u] += 1;
        }
    }

    // The graph bytes are no longer needed; dropping them keeps peak memory at
    // the CSR rather than CSR + 92 MB of input.
    drop(graph);

    // ── queries ─────────────────────────────────────────────────────────────
    let queries = match read(format!("{data}/queries.txt")) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("[track-d] cannot read queries.txt: {e}");
            exit(1);
        }
    };

    let mut dist = vec![INF; n];
    let mut touched: Vec<u32> = Vec::with_capacity(4096);
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::with_capacity(1 << 16);
    let mut out: Vec<u8> = Vec::with_capacity(queries.len() * 4 + 16);

    let mut qp = 0usize;
    while qp < queries.len() {
        // A trailing newline leaves no further query; stop before reading one.
        while qp < queries.len() && !queries[qp].is_ascii_digit() {
            qp += 1;
        }
        if qp >= queries.len() {
            break;
        }
        let s = next_int(&queries, &mut qp) as u32;
        let t = next_int(&queries, &mut qp) as u32;

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
                    let v = head[e];
                    let nd = d + weight[e] as u64;
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

    if let Err(e) = File::create(format!("{outd}/distances.txt"))
        .and_then(|mut f| f.write_all(&out))
    {
        eprintln!("[track-d] cannot write distances.txt: {e}");
        exit(1);
    }
}
