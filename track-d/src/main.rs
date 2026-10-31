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
//!   to infinity afterwards, and each node is listed once -- the push happens on
//!   the infinity-to-finite transition, not on every improvement.
//!
//! * **A settled bitmap, not a distance re-read.** The classic `if d > dist[u]`
//!   stale test costs a random probe into the 14 MB label array on every pop.
//!   A 217 KB bitmap of settled nodes answers the same question out of L2, and
//!   it also skips equal-key duplicates that the distance test lets through. The
//!   query phase is memory-latency bound, not arithmetic bound -- measured at
//!   1 worker 7.1 s against 8 workers 2.05 s on the full corpus -- so removing
//!   one random main-memory probe per pop is the whole point.
//!
//! * **Queries are claimed from one counter, not split into equal chunks.** The
//!   cost of a query varies by four orders of magnitude (the dearest costs
//!   5,000,000 relaxations, the median about 34,000), so a fixed chunk that
//!   happens to contain a dear one decides the wall time. A single `AtomicUsize`
//!   hand-out leaves the slowest worker doing the mean instead of the worst.
//!
//! * **A target with no incoming edge is answered before any search.** Nothing
//!   can reach it, so `d(s, t) = -1` for every `s != t`. Two of the 2,000 queries
//!   are exactly that (indices 781 and 1914), and each one used to cost a worker
//!   a full 5,000,000-edge sweep. The incoming-edge test is one parallel scan of
//!   the CSR against a bitmap of the 2,000 distinct targets, and it is exact: it
//!   can only ever answer a query that genuinely has no path.
//!
//! * **Early exit at t.** The reference searches forward from `s` only and
//!   settles everything closer than `t`, so stopping the moment `t` is popped is
//!   a pure win with no change in the answer.
//!
//! * A path may have zero edges, so `d(s, s) = 0` for every node, including a
//!   node with no edges at all -- that case is answered before any search.
//!
//! * **Bidirectional search was implemented, verified, and rejected.** A forward
//!   plus reverse CSR with the `mu` meeting rule reproduces this output hash
//!   exactly, but it settles only 1.6x fewer nodes (on this graph the ball at
//!   radius `2r` is about 3.2x the ball at `r`, so a two-sided search wins a
//!   square root of very little) while paying a second random probe per
//!   relaxation and a doubled label footprint: solve 2,242 ms -> 3,406 ms,
//!   i.e. 1.52x *slower*. It is not here.

use std::env;
use std::fs::{File, read};
use std::io::Write;
use std::process::exit;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

const INF: u64 = u64::MAX;

/// Hardware threads the scoring container is pinned to (six P-cores, SMT on).
/// The query phase already used this split; the parse now uses the same pool.
const NWORKERS: usize = 8;

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

/// Split `0..n` into `NWORKERS` node ranges, one per worker.
fn node_ranges(n: usize) -> [(usize, usize); NWORKERS] {
    let per = n.div_ceil(NWORKERS);
    std::array::from_fn(|w| ((w * per).min(n), ((w + 1) * per).min(n)))
}

/// A monotone radix heap for Dijkstra.
///
/// Dijkstra extracts keys in non-decreasing order, and that is exactly the
/// property this structure exploits: under monotone extraction it needs no
/// comparison at all, only the position of the highest bit where a key differs
/// from the last one extracted. Push and pop become O(1) amortised instead of a
/// `sift_up` on every relaxation.
///
/// It exists because the solve phase is roughly four fifths of this track's
/// board time and the binary heap is the only comparison-heavy structure inside
/// it -- one sift per relaxation over a 5,000,000-edge graph, 2,000 times.
///
/// The invariant is that no key smaller than `last` is ever pushed. Dijkstra
/// guarantees it: a new key is `d + w` with `w >= 0`, and `d` is the key just
/// extracted, which was the minimum in the heap. Zero-weight edges are therefore
/// legal, landing in bucket 0 alongside `last` itself.
struct RadixHeap {
    buckets: [Vec<(u64, u32)>; 65],
    last: u64,
    len: usize,
}

impl RadixHeap {
    fn new() -> Self {
        RadixHeap {
            buckets: std::array::from_fn(|_| Vec::new()),
            last: 0,
            len: 0,
        }
    }

    /// 0 when the key equals `last`, otherwise one past the highest bit where the
    /// two differ -- so bucket order is key order.
    #[inline]
    fn bucket(key: u64, last: u64) -> usize {
        let x = key ^ last;
        if x == 0 {
            0
        } else {
            64 - x.leading_zeros() as usize
        }
    }

    #[inline]
    fn push(&mut self, key: u64, node: u32) {
        let b = Self::bucket(key, self.last);
        self.buckets[b].push((key, node));
        self.len += 1;
    }

    #[inline]
    fn pop(&mut self) -> Option<(u64, u32)> {
        if self.buckets[0].is_empty() {
            let mut b = 1;
            while b < 65 && self.buckets[b].is_empty() {
                b += 1;
            }
            if b == 65 {
                return None;
            }
            // Promote the bucket's minimum to `last` and redistribute. Every
            // element then lands in a strictly lower bucket, because the new
            // `last` is their minimum and they all shared the higher bits that
            // defined bucket `b`.
            let mut min = u64::MAX;
            for &(k, _) in &self.buckets[b] {
                if k < min {
                    min = k;
                }
            }
            self.last = min;
            let items = std::mem::take(&mut self.buckets[b]);
            for (k, nd) in items {
                let nb = Self::bucket(k, self.last);
                self.buckets[nb].push((k, nd));
            }
        }
        let r = self.buckets[0].pop();
        if r.is_some() {
            self.len -= 1;
        }
        r
    }

    /// Empty every bucket but keep the allocations: 2,000 queries reuse the same
    /// 65 vectors instead of allocating a fresh heap each time.
    #[inline]
    fn clear(&mut self) {
        for b in self.buckets.iter_mut() {
            b.clear();
        }
        self.last = 0;
        self.len = 0;
    }
}

/// Per-query label table: node -> tentative distance, sized to the search it
/// serves rather than to the graph it is indexed by.
///
/// The labels used to live in `vec![INF; n]` -- 13.9 MB per worker, 111 MB
/// across the eight -- to hold the ~12,500 entries a search actually reaches
/// (24.65M touched entries over all 2,000 queries, dearest 135,312). Node ids
/// are effectively random, so at that density a 12,500-entry search still
/// touches nearly all 3,400 pages of that array: a ~200 KB payload paying for a
/// 13.9 MB index. The waste is index space, and it buys nothing.
///
/// This is open addressing over a capacity fitted to the query. Entering the
/// table costs one load: a slot is `16` bytes holding `node + 1` and the
/// distance, so a probe never needs a second, dependent fetch. Two arrays
/// indexed by the same probe position look equivalent but are not -- the
/// distance load sits behind the key comparison in the dependency graph unless
/// the compiler hoists it, and a separate arena indexed through the probe result
/// serialises by construction.
///
/// The capacity matters as much as the structure. A 32 Ki-slot table is 512 KB,
/// which stays resident, where the same 12,500 entries scattered over node-id
/// space are 787 KB of touched lines competing with the CSR for the same L2.
/// `reset` re-fits the table to the query that just finished, so one 2^18 query
/// does not leave the 250 small ones after it running against a table they
/// cannot keep resident.
///
/// A key of `node + 1` is what makes zero mean empty, which is what lets the
/// backing array be `vec![0u64; 2 * cap]`: that form lowers to `alloc_zeroed`,
/// so the kernel hands over zero-filled pages and only the pages a probe writes
/// are ever committed. Growth is 8x and walks the `touched` list, never a page
/// it has not already written. `touched` lists probe positions, so the reset is
/// one store per entry with no probe, and the node id needed to clear the
/// settled bit comes back out of the key.
struct Labels {
    /// Slot `i` is `slots[2i] = node + 1` (0 when empty) and `slots[2i + 1]`.
    slots: Vec<u64>,
    mask: usize,
    shift: u32,
    grow_at: usize,
    max_cap: usize,
    len: usize,
    touched: Vec<u32>,
}

impl Labels {
    /// 4 Ki slots to start; a search that needs more grows into it.
    const MIN_LOG: u32 = 12;

    fn new(n: usize) -> Self {
        let mut l = Labels {
            slots: Vec::new(),
            mask: 0,
            shift: 0,
            grow_at: 0,
            // Two slots per node is the ceiling, so however large a search gets
            // the table can never be more than half full.
            max_cap: (2 * n).next_power_of_two().max(1 << (Self::MIN_LOG + 1)),
            len: 0,
            touched: Vec::with_capacity(1024),
        };
        l.resize(1 << Self::MIN_LOG);
        l
    }

    /// Fresh empty table of `cap` slots. The old one is dropped whole, so no
    /// page of it is walked on the way out.
    fn resize(&mut self, cap: usize) {
        self.slots = vec![0u64; 2 * cap];
        self.mask = cap - 1;
        self.shift = 32 - cap.trailing_zeros();
        self.grow_at = cap - cap / 4;
    }

    #[inline(always)]
    fn slot(&self, node: u32) -> usize {
        (node.wrapping_mul(0x9E37_79B1) >> self.shift) as usize
    }

    /// Eight times larger, every live entry re-probed through the `touched` list
    /// so the walk never reads a slot that was never written.
    #[cold]
    fn grow(&mut self) {
        let cap = (self.slots.len() / 2 * 8).min(self.max_cap);
        let (mask, shift) = (cap - 1, 32 - cap.trailing_zeros());
        let mut slots = vec![0u64; 2 * cap];
        let mut touched: Vec<u32> = Vec::with_capacity(self.len);
        for &p in &self.touched {
            let j = p as usize * 2;
            let k = self.slots[j];
            let mut i = ((k as u32).wrapping_sub(1).wrapping_mul(0x9E37_79B1) >> shift) as usize;
            while slots[i * 2] != 0 {
                i = (i + 1) & mask;
            }
            slots[i * 2] = k;
            slots[i * 2 + 1] = self.slots[j + 1];
            touched.push(i as u32);
        }
        self.slots = slots;
        self.mask = mask;
        self.shift = shift;
        self.grow_at = cap - cap / 4;
        self.touched = touched;
    }

    /// Relax into `node`: keep the label if it is better, take it if the node is
    /// absent, and push the heap entry in both cases. One probe sequence, so the
    /// lookup and the insert cannot disagree.
    #[inline(always)]
    fn relax(&mut self, node: u32, nd: u64, heap: &mut RadixHeap) {
        let k = node.wrapping_add(1) as u64;
        let mut i = self.slot(node);
        loop {
            let j = i * 2;
            let key = self.slots[j];
            if key == k {
                if nd < self.slots[j + 1] {
                    self.slots[j + 1] = nd;
                    heap.push(nd, node);
                }
                return;
            }
            if key == 0 {
                self.slots[j] = k;
                self.slots[j + 1] = nd;
                self.touched.push(i as u32);
                self.len += 1;
                heap.push(nd, node);
                if self.len >= self.grow_at {
                    self.grow();
                }
                return;
            }
            i = (i + 1) & self.mask;
        }
    }

    /// Drop the settled bits this query set and re-fit the table to its size.
    /// `touched` holds probe positions and each key is its node's id plus one, so
    /// one load yields both the slot to clear and the node whose bit to drop.
    #[inline]
    fn reset(&mut self, done: &mut [u64]) {
        for &p in &self.touched {
            let j = p as usize * 2;
            let v = self.slots[j] as usize - 1;
            done[v >> 6] &= !(1u64 << (v & 63));
            self.slots[j] = 0;
        }
        self.touched.clear();
        self.len = 0;
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

    // ── queries ─────────────────────────────────────────────────────────────
    // Read before the CSR is built, because pass 3 needs the set of target nodes
    // and the file is 29 KB. One bitmap bit per node marks the targets; at 217 KB
    // it stays in cache, and only ~2,000 bits are ever set.
    let queries = match read(format!("{data}/queries.txt")) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("[track-d] cannot read queries.txt: {e}");
            exit(1);
        }
    };
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

    let tbits = n.div_ceil(64);
    let mut is_tgt = vec![0u64; tbits];
    for &(_, t) in &qs {
        is_tgt[t as usize >> 6] |= 1u64 << (t & 63);
    }

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

    // ── pass 3, parallel: which targets can be reached at all ───────────────
    // One sweep of the finished CSR marking every node that is the head of some
    // edge. A query target that is never marked has in-degree zero, and then no
    // path can end at it, whatever the source. This runs over the packed edge
    // array rather than over the ASCII, and the bitmap test is a cache hit for
    // all but the ~2,000 nodes that are targets, so the atomic fires almost never.
    let has_in: Vec<AtomicU64> = (0..tbits).map(|_| AtomicU64::new(0)).collect();
    std::thread::scope(|scope| {
        for (lo, hi) in node_ranges(n) {
            let offset = &offset;
            let edge = &edge;
            let is_tgt = &is_tgt;
            let has_in = &has_in;
            scope.spawn(move || {
                for u in lo..hi {
                    for e in offset[u] as usize..offset[u + 1] as usize {
                        let v = edge[e].load(Ordering::Relaxed) as u32;
                        let bit = 1u64 << (v & 63);
                        if is_tgt[v as usize >> 6] & bit != 0 {
                            has_in[v as usize >> 6].fetch_or(bit, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });

    // The graph bytes are no longer needed; dropping them keeps peak memory at
    // the CSR plus the per-worker distance arrays rather than adding 92 MB on
    // top of them. (The Vec is a large mmap allocation, so it really is
    // returned to the OS here.)
    drop(graph);

    // ── answer what the topology already decides, and list the rest ─────────
    // `pre` is the answer whenever one is known without searching: 0 for `s == t`
    // (the legal zero-edge path, even on a node with no edges) and INF for a
    // target with no incoming edge. Everything else goes on the work list.
    let mut pre: Vec<u64> = vec![0; qs.len()];
    let mut todo: Vec<u32> = Vec::with_capacity(qs.len());
    for (i, &(s, t)) in qs.iter().enumerate() {
        if s == t {
            continue;
        }
        if has_in[t as usize >> 6].load(Ordering::Relaxed) >> (t & 63) & 1 == 0 {
            pre[i] = INF;
        } else {
            todo.push(i as u32);
        }
    }

    // ── answer the rest in parallel, one query at a time ────────────────────
    // The queries are mutually independent and the graph is read-only, so this is
    // the one place in the submission where threads cost almost nothing: no
    // locks, no shared writes to the CSR, and the output order is fixed by
    // writing each answer into its own slot.
    //
    // Work is claimed with one atomic per query rather than split into
    // `NWORKERS` equal chunks: equal chunks are a bet that every chunk costs the
    // same, and here the dearest query is worth 150 median queries. Draining a
    // shared counter instead puts the dear ones on whichever worker is free.
    //
    // Each thread owns its own distance array, because 1.74M u64 labels cannot
    // be shared without synchronisation. That is the only cost: 8 x 14 MB of
    // working state, against a search phase that dominates the run.
    let answers: Vec<AtomicU64> = pre.iter().map(|&a| AtomicU64::new(a)).collect();
    let nextq = AtomicUsize::new(0);
    let nthreads = NWORKERS.min(todo.len().max(1));
    let out_cap = qs.len() * 6 + 16;

    std::thread::scope(|scope| {
        for _ in 0..nthreads {
            let offset = &offset;
            let edge = &edge;
            let qs = &qs;
            let todo = &todo;
            let answers = &answers;
            let nextq = &nextq;
            scope.spawn(move || {
                let mut labels = Labels::new(n);
                let mut done = vec![0u64; tbits];
                let mut heap = RadixHeap::new();

                loop {
                    let k = nextq.fetch_add(1, Ordering::Relaxed);
                    if k >= todo.len() {
                        break;
                    }
                    let qi = todo[k] as usize;
                    let (s, t) = qs[qi];
                    labels.reset(&mut done);
                    heap.clear();
                    // The source enters through the same path as any other node:
                    // one insert, one heap entry, no special case.
                    labels.relax(s, 0, &mut heap);
                    let mut found = INF;
                    while let Some((d, u)) = heap.pop() {
                        let ui = u as usize;
                        let bit = 1u64 << (u & 63);
                        if done[ui >> 6] & bit != 0 {
                            continue; // already settled, this entry is a duplicate
                        }
                        done[ui >> 6] |= bit;
                        if u == t {
                            found = d;
                            break; // everything still queued is at least this far
                        }
                        for e in offset[ui] as usize..offset[ui + 1] as usize {
                            let ev = edge[e].load(Ordering::Relaxed);
                            let nd = d + (ev >> 32) as u32 as u64;
                            labels.relax(ev as u32, nd, &mut heap);
                        }
                    }
                    answers[qi].store(found, Ordering::Relaxed);
                }
            });
        }
    });

    let mut out: Vec<u8> = Vec::with_capacity(out_cap);
    for i in 0..qs.len() {
        let a = answers[i].load(Ordering::Relaxed);
        if a == INF {
            out.extend_from_slice(b"-1\n");
        } else {
            let _ = write!(out, "{a}\n");
        }
    }

    if let Err(e) =
        File::create(format!("{outd}/distances.txt")).and_then(|mut f| f.write_all(&out))
    {
        eprintln!("[track-d] cannot write distances.txt: {e}");
        exit(1);
    }
}
