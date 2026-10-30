> ## SUPERSEDED — read this before using anything below
>
> An **early design exploration** (Oct 26–28), kept for its reasoning, not for its
> prescriptions. It was written before the frozen specs and harness were read, so
> its numbers are estimates where the real ones are now published, and several of
> its prescriptions were replaced.
>
> **Authority, in order:** the frozen track spec and harness under
> `reference/official/`, then `verification/contract.toml`, then
> `docs/MODULE-PLAN.md`. Where this file disagrees with any of them, this file is
> wrong.
>
> **Stale facts in this file, corrected:**
>
> | This file says | Correct |
> |---|---|
> | M_ref / S_ref **DEFERRED** | **Published.** M_ref = 892.915712 MB, S_ref = 43 lines. T_ref = 72.692655 s, timeout 726.9 s. |
> | implicit assumption that distances fit 32 bits | **They do not, and this is a trap.** Weights run to `2^32 - 1` and the spec's own worked example answers `8589934594`. **u32 distance labels are wrong on this dataset.** The bound for u32 labels would be `(n-1) * w_max < 2^32`, i.e. `w_max <= 2468` at n = 1,740,512 — and the generator deliberately injects weights near `2^32`. Use u64 labels (or one generic u64 instantiation), and compute `mu` sums in u64. |
> | contraction hierarchies / ALT as candidate indexes | Legal but **not worth it here**: queries are drawn at a bounded search distance, so a heavy index has little time to repay its build. |
> | reset distances per query with `memset` | Use a **touched list or generation stamps**; a full clear per query is pure waste at 1.74M nodes. |
---

# TRACK D · TRAVERSE — Design Specification

Status: pre-kickoff design document. No solution code, no function bodies, no implementation language.
Track window: baseline H0–H17 (2h charged to Track D), optimization block H33–H43 (10h charged).
Reference: T 72.69s (highest reference time in the event) · Mref/Sref DEFERRED to Oct 28 frozen spec · timeout 727s.
Input: graph.txt — header line "n m", then m lines "u v w" as uint32 ×3 (tail, head, weight), not sorted by u; node ids dense, 0-indexed; directed; a reverse edge, where it exists, is listed separately.
Output contract (distance-only vs full path) and the unreachable spelling: **DEFERRED to Oct 28 frozen spec**.
Scoring reminder: K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15; speed_score = 50·log10(K), clamped [0,100]. The X axis is gone; only M is capped at 50×.

---

## 1. Reference Teardown

**What the reference does (from the frozen brief).** Reads the graph, then answers 2,000 source-target shortest-path queries. The reference's 72.4s is the highest reference time in the event, which tells us where the time is: **2,000 shortest-path queries over a 5,000,000-edge graph is the workload, and doing them one at a time without preprocessing is expensive.**

**Measured reference axes.** T = 72.4s. Mref, Sref, Xref are **DEFERRED to Oct 28 frozen spec**.

**Structural facts from the brief (these drive every decision below).**
- 5,000,000 edges, 180MB on disk → ~36 bytes per edge line (11 digits + 2 spaces + newline = 14 bytes per edge for 3×uint32... let's not over-fit; the arithmetic that matters is 180MB / 5M edges = 36 bytes/edge including a possible trailing structure; a realistic uint32 triple is ≤ 34 bytes with wide values, so the file is essentially "three decimal numbers per line, space-separated").
- Node count `n` is in the header. The published sample header is "1740512 5000000", i.e. a node count around 1.74M with 5M edges — but the sample is a fragment, so **n is DEFERRED; the program must read it from the header and must not assume any density**. At n ≈ 1.74M and m = 5M, the average out-degree is ≈ 2.9. If n is instead 5M, the average out-degree is 1.0. Both regimes are plausible and they change the algorithm's cost profile materially. **The design must read n and adapt.**
- Node ids are dense and 0-indexed → a direct-indexed array (CSR offsets) is possible with no hash table and no indirection. This is the single most valuable structural fact in the track.
- Weights are uint32 → distances are uint64 (paths can be long), but with uint32 weights and a 1.74M-node path the maximum simple-path distance is ≈ 7.5e14, which fits in u64. Use u64 distances with a u32 "settled" marker scheme.
- The edge list is **not sorted by u** → a counting-sort (bucket) pass is required to build CSR. That pass is O(m) and is the price of admission.
- Directed, with reverse edges listed separately → we do **not** get a free reverse graph; if we want bidirectional search we must build the reverse CSR ourselves (another O(m) pass) or accept one-directional search.

**Why the reference takes 72.4s — structural reading (hypothesis, to be measured at H0–H2).**
- 2,000 queries, each requiring a shortest-path search. If the reference runs a plain Dijkstra per query over a 1.74M-node / 5M-edge graph, one full Dijkstra costs roughly 10–30M heap operations; 2,000 of them is 2×10^10 operations — far more than 72.4s allows on 12 threads, so the reference is probably already doing something smarter than naive full Dijkstra (or its graph is sparser than the worst case, or it uses a plain BFS-like relaxation without a heap). The exact decomposition is a measurement, not a guess.
- The plausible reference shape: a forward-star adjacency (linked lists through a `next` array), which is cache-hostile (random pointer chasing per edge), plus a binary heap of `(dist, node)` pairs, plus a `HashMap` or a reset-by-stamp array for distances, plus 2,000 sequential (or 2,000 × per-query) searches.
- The reference's memory: forward-star costs (to, weight, next) × 5M = 60MB plus a heap; the 2,940MB-scale memory of Track A's reference is not applicable here, but Mref is unpublished and our M target (8–15×) implies Mref is on the order of hundreds of MB to a couple of GB. DEFERRED.

**The central decision this track poses: the build-vs-query trade, where the build is inside the timed run.**
- "Preprocessing is EXPLICITLY ALLOWED and counted INSIDE the timed run." So any index we build is paid for in T.
- 2,000 queries is a *small* query population. The amortization factor is 2,000. A preprocessing structure costing `P` seconds and saving `s` seconds per query pays off if `P < 2000·s`.
- **Contraction hierarchies:** the brief names CH as "legal and expensive to get right in 72h". CH preprocessing on a 1.74M-node / 5M-edge graph takes on the order of 30–120s (repeated hub selection, shortcut generation, degree reduction) — and it must be paid *inside* the timed run. Even at 60s of preprocessing, that alone consumes 83% of our Commit budget (2.90s). **CH is a trap in this event's scoring model, and rejecting it with an explicit calculation is a high-value judge-facing decision.** The rejection note belongs in PERF.md.
- **The alternative that fits the amortization:** a cheap, low-preprocessing-cost structure that improves each query's search by a large factor. The natural candidate is a *landmark/ALT potential* set: a small number of full-graph Dijkstras (one per landmark) from which we can derive an admissible, consistent heuristic (the standard triangle-inequality bound over landmark distances), turning each query into an A* search. Landmark distances are computed with the same full Dijkstra we'd run per query, so the cost is `k` full Dijkstras for `k` landmarks — with k = 8, that is 8 full searches to pay for 2,000 A* searches. Even if each A* is 5× faster than a plain bidirectional Dijkstra, the amortization is 8/2000 = 0.4% overhead against a 5× per-query win. That is the right kind of trade at 2,000 queries.
- A second cheap structure: **group the queries by source**. If the 2,000 queries share sources (likely, in a benchmark generated from a small set of hubs), a single Dijkstra per distinct source answers all of that source's queries. The number of distinct sources is **DEFERRED** (it is a property of the query file, which we have at run time). The program should count distinct sources and distinct targets at run time and choose the strategy accordingly.

**Gate implications for this track.**
- Gate 1: the output must be byte-exact. The output contract (distance-only vs full path) and the unreachable spelling are DEFERRED. Distance-only makes tie-breaks moot, and the brief hints that distance-only "lowers Contraction-Hierarchy risk" — implying the full-path variant carries tie-break rules we would have to match exactly. **Decision: if the spec offers a choice, take distance-only.** If the spec mandates full paths, tie-breaks become a correctness project of their own.
- Gate 2: speed floor; our anchor is 25–40×.
- Gate 3: 724s timeout, 10× the reference. A hang (a Dijkstra with a broken decrease-key) is the only realistic failure.
- Gate 4: no network, read-only dataset. The graph file is 180MB — we may not write a preprocessed graph to disk during the run either... we *could* write to /out, but the output directory is checked and the task expects one output file; staging there is a bad idea. **In-memory only, scratchless.**

---

## 2. Axis Budget

**Anchor rows.**

| Row | T ratio | M ratio | S ratio | D ratio | X ratio |
|---|---|---|---|---|---|
| Commit | 25× | 8× | 0.55× | 1.00× | 1.05× |
| Stretch | 40× | 12× | 0.58× | 1.00× | 1.10× |
| Exceptional | 50× | 15× | 0.50× | 1.00× | 1.15× |

**Full arithmetic.**

- Commit: T 25× ^0.40 = 3.6239; M 8× ^0.20 = 1.5157; S 0.55× ^0.15 = 0.9142; D 1.00× ^0.15 = 1.0000; X 1.05× ^0.10 = 1.0049.
  K = 3.6239 × 1.5157 × 0.9142 × 1.0000 × 1.0049 = **5.0462**.
  speed = 100·ln(5.0462)/ln(25) = 100 × 1.61874/3.21888 = **50.29**.
  Absolute read: T = 72.4/25 = 2.896s.
- Stretch: T 40× ^0.40 = 4.3734; M 12× ^0.20 = 1.6438; S 0.58× ^0.15 = 0.9215; D 1.00×; X 1.10× ^0.10 = 1.0096.
  K = 4.3734 × 1.6438 × 0.9215 × 1.0000 × 1.0096 = **6.6883**.
  speed = 100·ln(6.6883)/ln(25) = 100 × 1.90030/3.21888 = **59.04**.
  Absolute read: T = 72.4/40 = 1.810s.
- Exceptional: T 50× ^0.40 = 4.7818; M 15× ^0.20 = 1.7188; S 0.50× ^0.15 = 0.9013; D 1.00×; X 1.15× ^0.10 = 1.0141.
  K = 4.7818 × 1.7188 × 0.9013 × 1.0000 × 1.0141 = **7.5114**.
  speed = 100·ln(7.5114)/ln(25) = 100 × 2.01666/3.21888 = **62.64**.
  Absolute read: T = 72.4/50 = 1.448s.

**Why the Commit row carries a 2.9s budget and what has to fit in it.** This is the track's central arithmetic, so it is laid out in full:

| Component | Budget (Stretch 1.81s) | Basis |
|---|---|---|
| Parse 180MB of edge text into (u, v, w) triples | 0.30s | 180MB at ~600MB/s with a custom numeric scanner; a general reader is 3–5× slower |
| Build CSR (counting sort of 5M edges by tail) | 0.25s | Two O(m) passes over 5M edges (60MB of u32), parallelizable |
| Build reverse CSR (only if bidirectional search is used) | 0.25s | Same cost again; the reverse graph is NOT free (the edge list is directed) |
| Landmark preprocessing (k = 8 full Dijkstras) | 0.40s | 8 full-graph searches; each ≈ 50ms at 1.74M nodes with a radix heap |
| Query phase: 2,000 A*/bidirectional searches | 0.50s | Parallel across 12 workers; 0.25ms per query |
| Output | 0.11s | 2,000 result lines — trivial; the budget is slack for the format |
| **Total** | **1.81s** | 40× anchor |
| **Contraction hierarchy alternative** | **60–120s** | Alone 33–66× over the Stretch budget → **rejected** |

**The CH rejection, stated as a calculation for PERF.md:** preprocessing cost `P` is amortized over 2,000 queries. A per-query saving of `s` seconds justifies `P ≤ 2000·s`. CH preprocessing at 60s requires a per-query saving of 30ms; a plain bidirectional Dijkstra on this graph is likely already in the 1–5ms range for typical queries, so CH's *marginal* benefit over a good bidirectional baseline is far below 30ms per query — while its preprocessing cost is 60s. **CH loses by two orders of magnitude inside the timed run.** Landmarks, by contrast, cost 0.4s and only need to save 0.2ms per query to break even, which a 5× A* speedup trivially does.

**Memory budget and the M axis.** Mref is DEFERRED, so the design holds an *absolute* peak target and converts later.

| Structure | Bytes |
|---|---|
| CSR forward: 5M × (head u32 + weight u32) = 40MB, plus offsets (n+1) × 4B ≈ 7MB | 47 MB |
| CSR reverse (if used): same shape | 47 MB |
| Landmark distances: k × n × u32 (saturating at u32::MAX is wrong; use u64 or store the *difference* potentials) | 8 × 1.74M × 8B = 111MB (u64) or 8 × 1.74M × 4B = 56MB (u32, with a saturating convention) |
| Distance/labels array: n × u64 | 14 MB |
| Priority-queue heap arena (per worker, bounded) | 12 × 4MB = 48 MB |
| Edge-text scratch during the parse (bounded, streamed) | 8 MB |
| **Total** | **≈ 210–280 MB** |

**The landmark memory is the M risk.** 8 landmarks × n × 8B = 111MB is over half the budget. Mitigations: (a) store landmark distances as u32 with a saturating "infinity" convention and handle overflow conservatively (a saturated distance can only weaken the heuristic, never break admissibility if handled with care — this needs proof, so it is a rung with a dedicated test); (b) use fewer landmarks (4) and rely on the query-phase savings; (c) store landmark distances only for the nodes that matter — no, that breaks the heuristic's validity on unseen nodes; (d) **store the landmark distances as u32 with the per-landmark maximum subtracted, so the stored range fits** — this is the standard trick and it keeps the triangle-inequality bound valid with a per-landmark offset. **Decision: u32 with a per-landmark offset, k = 6 to 8, chosen by measurement.**
If Mref is 1.5GB, 8× = 187MB and our 210MB is close; if Mref is 2.5GB, 8× = 312MB and we are comfortably inside. DEFERRED — but the design's absolute target is ≤ 256MB, and the first thing the optimization ladder measures is memory.peak.

**S axis.** A CSR builder, a custom numeric scanner, a radix heap, an ALT-potential A*, and a bidirectional search is a lot of code — the S target of 0.55 means our normalized source is 1.82× the reference's. The exchange rate: doubling S costs 2.1 points; the T gain from the same engineering effort is worth far more. **Decision: source size is an accepted cost; spend it on the algorithm.**

**D axis.** D = 1 → std-only, no crates. A radix heap, a bucket queue, an open-addressed table, and a thread pool are all hand-rolled. Note that `core::arch` intrinsics remain available and do not affect D.

---

## 3. Algorithm & Data Layout

**Stage 1 — parse the edge text (0.30s budget).** The file is a header line "n m" followed by m lines of three space-separated uint32 decimals. The parse is a streaming byte scanner: skip the header, then repeatedly read three integers with a digit loop that accumulates into u32. No allocation, no string, no `split`. Reading: a 4–8MB buffer, sequential. **The output format is a byte hash, so the parse must produce exactly the right numbers; overflow behavior on a malformed number must be a loud error, not a wrap.**

**Stage 2 — build the CSR (0.25s).** Because node ids are dense and 0-indexed, a counting sort is exact and allocation-free beyond two arrays:
- Count the out-degree of every node into an n+1 array.
- Prefix-sum into the CSR offset array.
- Scatter each edge (head, weight) into its tail's slot. The scatter is the random-access phase (5M random writes into a 40MB array — that is a cache-miss storm, roughly 5M × ~50ns = 0.25s single-threaded, and it parallelizes only awkwardly because the destination order depends on the count phase).
- **Parallelism for the scatter:** partition the edge array into 12 contiguous ranges; each worker computes partial counts, we prefix-sum the per-worker per-node counts (12 × n u32 = 84MB — too much memory!), then each worker scatters into its own offset-shifted region. The 12 × n count matrix is the memory problem. **Alternative: a two-phase radix partition of the edges by tail (bucket by the high bits of the tail id), then sort each bucket in place.** A 2-pass MSB radix partition of 5M (u32,u32,u32) triples is 5M × 24B × 2 = 240MB of traffic — heavy but linear and cache-friendly. **Alternative 2: single-threaded count + parallel scatter with per-worker reservation counters (atomic increments) — 5M atomic increments is ~0.05–0.1s, acceptable.** **Alternative 3 (preferred): do the count phase in parallel with 12 partial count arrays of size n but reuse them as a single array of 12 × n u16... no, counts exceed u16.** Practical answer: if n ≈ 1.74M, a 12 × n u32 count matrix is 84MB — too much on top of a 47MB CSR. **Use a hybrid: parallel count into 12 partial arrays of n u32 is out; instead, do a sequential count pass (1.74M increments, ~5ms) and a parallel scatter where each worker first computes its own local offsets by re-counting only its range (a second local count over 5M/12 edges), which is cheap, and then a global prefix-sum per worker over the n nodes is 12 × n adds = 21M adds, ~20ms.** That yields a deterministic parallel scatter with no atomics and a bounded memory cost of one extra n-sized array per worker (12 × 7MB = 84MB — still heavy). **Simplification: use 4 workers for the scatter, or accept a single-threaded scatter at 0.25s.** The ladder will settle this by measurement; the important thing is that we do not silently use a hash map for adjacency, which is what would blow both the M and T budgets.

**Stage 3 — the reverse graph.** Built with the same counting-sort pass, or avoided. Bidirectional search needs the reverse graph. **Decision: build it if the measured single-direction search is not fast enough; the reverse CSR costs 47MB and 0.25s, which is 14% of the Stretch budget — a meaningful fraction that must be earned, not assumed.** A cheaper alternative: since a "reverse edge, where it exists, is listed separately", some edges exist in both directions; that does not give us a free reverse index, but it does mean a reverse-CSR build can copy the forward one for those pairs. Not worth special-casing.

**Stage 4 — the search core.**
- **Priority queue:** a **radix heap** (monotone keys, i.e. Dijkstra's) is the right structure: O(m + n log C) with 64 buckets, no comparisons, cache-friendly, and it beats a binary heap by 2–3× on graph workloads. A binary heap of (u64 dist, u32 node) pairs is the fallback. Dial's bucket queue is an alternative when weights are small — but weights are uint32 and unbounded, so a radix heap (or an "any decrease" priority queue) is the safe choice. **Decision: radix heap, hand-rolled, with a binary-heap fallback for testing.**
- **Distance storage:** a flat u64 array of size n, reset between queries by a "stamp" trick: store (stamp, distance) packed, or keep a separate u32 stamp array and only reset the touched entries. With 2,000 queries on a 1.74M-node graph, a full reset is 1.74M × 2,000 = 3.5B writes — way too slow. **The stamp trick is mandatory, not optional.**
- **Bidirectional search:** the brief calls it "table stakes, not a strategy". We implement it: forward and backward Dijkstra meet in the middle, with a termination rule on the best-known path length. Termination with the "stop when the two frontier minima sum to ≥ the best known" rule requires a specific meeting condition (the classic "min_f + min_b ≥ μ" test). Getting this exactly right (so that the returned distance is the true shortest) is a correctness project; a wrong termination rule returns a suboptimal distance and fails the hash. **Mitigation: validate against a brute-force Dijkstra on a small subgraph and on a sample of the real queries.**
- **ALT / landmark potentials:** with k landmarks, for each landmark L we have d_L(v). The potential for edge (u,v) is p(u,v) = max over L of |d_L(u) − d_L(v)|. This is admissible and consistent. A* with this potential expands far fewer nodes. Computing d_L requires k full Dijkstras (one per landmark) — with a radix heap, a full Dijkstra over 1.74M nodes / 5M edges is on the order of 30–80ms; 8 landmarks ≈ 0.25–0.65s. **This is the big T win, and it is affordable because 2,000 queries amortize it 250:1.** Landmark selection: pick nodes that are far apart (e.g. by a rough BFS/eccentricity heuristic) — a simple, defensible choice; the alternative (a full "maximum coverage" selection) costs a lot for little gain.
- **Combining bidirectional and A*:** bidirectional A* with a potential is possible but is the hardest version to get right. **Decision ladder: (1) plain bidirectional Dijkstra; (2) A* with ALT potentials, one direction; (3) bidirectional with potentials only if (2) shows a clear win and the termination rule is provably correct.** In a 72-hour sprint with a byte-hash gate, simplicity is a correctness asset. This is exactly the kind of decision the judges' "evidence of decisions" criterion is looking for.

**Stage 5 — query batching (a runtime-adaptive decision).** At run time, read the query file, count distinct sources and distinct targets, and choose:
- If distinct sources are few (say ≤ 200), run one Dijkstra (or one A* per source) per source and answer all its queries from that tree. This converts 2,000 searches into a handful — a potential order-of-magnitude win.
- If sources are many, use per-query bidirectional search.
- If distinct targets are few, a **backward** batched search (from the targets) is equivalent; choose the side with fewer distinct vertices.
- The choice is measured on the actual query file at run time, and the measurement is logged. **This is the single highest-leverage adaptive decision in the track and it costs nothing to implement.**

**Data layout summary.**
- CSR forward (and reverse if justified): contiguous arrays, offsets in one array, edges in one array. No pointer chasing, no per-node allocations. Cache behavior: a node's neighbors are contiguous.
- The radix heap: a flat array of 64 bucket heads plus a per-entry list; entries are (key u64, value u32) packed into 16 bytes, allocated from a per-worker arena that is reset per query.
- Distances: n × u64 in one array + n × u32 stamps.
- Landmark distances: k arrays of n u32 with per-landmark offsets.
- Output: 2,000 lines written streaming to /out.

---

## 4. Hash-Gate Correctness & Determinism

**Determinism rule.** The distance for a query is a mathematical property of the graph, so the only risks are (a) an incorrect search that returns a non-optimal distance, (b) an incorrect unreachable spelling, (c) an incorrect output format, and (d) tie-break differences if full paths are required.

| Hazard | Elimination |
|---|---|
| Non-optimal distance from a wrong bidirectional termination rule | The termination rule is a single, explicitly specified condition; it is validated against a brute-force Dijkstra on a sampled subgraph and on sampled real queries (V3) |
| Landmark heuristic breaking admissibility | The potential is computed as a maximum over landmarks of a difference of true distances (never a heuristic estimate) → admissible by construction. Saturation/overflow in the u32 storage is handled by per-landmark offsets so that the stored differences are exact |
| Tie-breaks (only if full paths are required) | If the spec requires full paths, the tie-break rule must be read from the spec and implemented exactly — a dedicated risk with a dedicated budget. **Preferred: distance-only, which the brief says "lowers Contraction-Hierarchy risk"** |
| Unreachable spelling | DEFERRED to the Oct 28 spec. The value is a single constant in one module; candidate spellings (`-1`, `INF`, `unreachable`, a sentinel like `18446744073709551615`) are switchable, and the sample hash selects one |
| Output formatting (a distance as u64 decimal) | Hand-rolled integer-to-decimal emission; no locale, no float. The output is integer-only, so there is no float-formatting risk in this track |
| Distance overflow (u32 weights, long paths) | u64 distances; a path of 1.74M edges × max u32 weight ≈ 7.5e14 fits comfortably |
| Distances left stale between queries | The stamp trick; a per-query stamp is incremented and a node is "unvisited" unless its stamp matches. A forgotten stamp reset is a correctness bug, so the stamp array is checked by the brute-force comparison in V3 |
| Heap corruption / lost decrease-key | A radix heap must support decrease-key correctly. The standard trick is to allow duplicates (lazy deletion) and to filter on pop by comparing against the current distance — that is simple and safe, at the cost of more heap entries. **Decision: lazy-deletion heap, with the popped distance compared to the array value; entries that are stale are discarded.** This is the design's main protection against a subtle wrong-distance bug |
| Parallel query evaluation writing results out of order | Each query writes to a fixed slot indexed by its query number; the output is written in query order |
| Floating point | None. All arithmetic is integer. A* potentials and Dijkstra distances are exact integers |
| Build determinism (CSR ordering) | The CSR build is a deterministic function of the edge file: the scatter order within a node's slot is the input order of that node's edges. If the reference emits paths, the tie-break may depend on this order — DEFERRED with the output contract |

**Verification ladder.**
- **V1 (baseline, H0–H2):** the 10% sample ships with its expected hash. The baseline (a straightforward CSR + per-query Dijkstra, no landmarks, no bidirectional) must reproduce it.
- **V2 (brute-force cross-check):** an independent, deliberately simple implementation — a plain binary-heap Dijkstra over an adjacency structure built with a completely different method (e.g. per-node lists, no CSR) — is kept as an oracle. Every optimized search must agree with it on the sample.
- **V3 (random-subgraph cross-check):** take random subgraphs of the real graph (e.g. 1% of the nodes and their edges, plus the query endpoints) and compare the optimized search against the brute-force oracle on all query pairs *within* that subgraph. This is a cheap, high-yield way to validate a bidirectional termination rule, since a sub-1,000-node subgraph can be brute-forced many times. **This is the most important verification activity in the track.**
- **V4 (landmark validity):** for every edge (u,v) and every landmark L, assert |d_L(u) − d_L(v)| ≤ w(u,v). This directly proves the potential is admissible and consistent, and it is a cheap O(m·k) check. A violation is a build bug caught before the gate.
- **V5 (heuristic soundness):** assert that the A* returned distance equals the oracle's distance for every query (never merely ≥ it).
- **V6 (repeat runs and worker counts):** identical hashes across 1, 6, 12 workers and across 5 runs. The search itself is per-query independent, so parallelism is at the query level and cannot change a distance.
- **V7 (adversarial graphs):** a synthetic test set: a disconnected graph (unreachable), a single-node graph, a graph with parallel edges, a graph with a zero-weight edge, a graph where the shortest path revisits a node, a graph with a unique path (tie-break check if full paths are required), a graph with an edge weight of u32::MAX.

**Gate 4 compliance.** No network. The dataset mount is read-only; we never write to it. The root filesystem is read-only; the 180MB graph is never staged to /tmp (1GB tmpfs, and tmpfs pages are charged against the 8GB limit and against `memory.peak`, which would destroy the M axis). **All preprocessing happens in process memory.** The only file written is the output to /out. No answers are precomputed into the image: every distance is computed at run time from the mounted graph.

**Gate 3 compliance.** 724s against a 1.4–2.9s target. A hang could come from a heap bug (an entry that is never popped). Mitigation: every search has a hard iteration budget (a bound on pops) and aborts loudly if exceeded; the query phase is embarrassingly parallel, so one bad query cannot stall the others.

---

## 5. Optimization Ladder

| # | Rung | Hypothesis / expected effect | Risk | Acceptance |
|---|---|---|---|---|
| L0 | Baseline: parse + CSR forward + per-query binary-heap Dijkstra with the stamp trick, single-threaded | Oracle + V1 hash | Low | Sample hash matches |
| L1 | Radix heap replacing the binary heap | 2–3× on the search phase; the search dominates the reference-style cost | Medium (heap bugs) | V2/V3/V5 equality; no distance differs |
| L2 | Bidirectional search (forward + reverse CSR) | The brief's "table stakes": roughly 2× on the search phase, more on long-path queries | Medium (termination rule) | V3 random-subgraph cross-check on thousands of query pairs |
| L3 | ALT landmark potentials (k = 6–8) + A* | The big win: the potential prunes the explored subgraph; expected 3–10× on the query phase, at a preprocessing cost of ~0.4s | Medium (admissibility) | V4 admissibility assertion on every edge; V5 equality on every query |
| L4 | Batched search by distinct source/target, chosen at run time | If the 2,000 queries share sources, this collapses the query count — potentially the largest single win in the track | Low (semantics: a single-source Dijkstra answers all of that source's queries exactly) | V5 equality; log the distinct-source/target counts |
| L5 | Parallel query evaluation across 12 workers (one query per worker task, fixed result slots) | 2–4× on the query phase; no contention (the CSR is read-only) | Low | V6 worker-count invariance |
| L6 | Parallel parse + parallel CSR count/scatter with a deterministic per-worker offset scheme | 0.3–0.5s off the build phase | Medium (offset arithmetic bugs produce a corrupted CSR) | V2: the CSR built with 1/6/12 workers yields identical query answers; plus a CSR self-check (sum of degrees = m) |
| L7 | Reverse-CSR avoidance when L4 or L3 makes bidirectional unnecessary | Saves 0.25s and 47MB | Low | Keep only if the query phase is not degraded |
| L8 | Landmark set pruning: use only the landmarks that reduce expansions most (measured per landmark) | Reduces the 111MB landmark memory and the preprocessing cost | Medium (the heuristic quality drops) | V4 + V5 still pass; measure the expansion-count change |
| L9 | Edge-weight special cases: a "max weight" bound used to prune, zero-weight handling, self-loop handling | Small but free correctness-preserving wins | Low | V7 adversarial graphs |
| L10 | Output path: preallocated buffer, hand-rolled u64-to-decimal | Removes the last formatting ambiguity | Low | Hash unchanged |
| L11 | Stretch attempts: tighter landmark count, a second-level "bounded A*" (BLS) instead of full ALT, bucket-based reordering | Diminishing | Medium | ≥3% T gain each, no memory regression, all of V2–V6 re-run |

**Explicitly rejected rungs (for PERF.md).**
- **Contraction hierarchies:** rejected with the amortization calculation (§2). The note must show the numbers: preprocessing 60–120s inside the timed run versus a 1.8–2.9s total budget, amortized over only 2,000 queries. The brief calls CH "legal and expensive to get right in 72h" — our analysis says it is also uneconomic here, because the timed run contains the preprocessing. **This rejection, with the arithmetic, is the centerpiece of the PERF.md for this track.**
- **Forward-star / linked adjacency (the reference-style layout):** pointer chasing per edge; CSR is strictly better on both T and M. Rejected on measurement.
- **A full path output when distance-only is available:** full paths add a tie-break contract and a predecessor array (another n × 4B) for zero scoring benefit. If the spec mandates paths, it becomes a separate, scheduled work package (see R-D3).
- **Resetting the distance array between queries (instead of the stamp trick):** 3.5B writes over 2,000 queries. Rejected on arithmetic alone.
- **Storing a materialized graph on disk in /out to "preprocess once":** forbidden in spirit by the sandbox diff, and the timed run includes preprocessing anyway, so it would not help the score.
- **A comparison-based priority queue with decrease-key:** slower and harder to get right than the radix heap with lazy deletion.

**PERF.md content plan.** The amortization table (structure cost vs per-query saving vs break-even query count) with the CH line computed explicitly; the budget breakdown table from §2; a per-rung table with build time, preprocessing time, query time, memory.peak, nodes expanded per query, and kept/reverted; the V3/V4 validation results; and the failed experiments above.

---

## 6. X-Scaling Safeguard

**What X measures.** X = (runtime at 10× input) ÷ (runtime at 1×) versus the reference's own ratio. Weight 0.10. Targets 1.05 / 1.10 / 1.15 for Commit / Stretch / Exceptional.

**Scaling analysis.**
1. *Parse.* Linear in the file size: 10× the bytes, 10× the time. The 180MB → 1.8GB read at 10× is 10× the I/O; page cache will not hold 1.8GB comfortably alongside an 8GB cap that also holds our index — so the 10× probe becomes I/O-bound in a way the 1× run is not. **Expect the 10× run to be modestly superlinear on the parse (ratio > 1.0) because of cache pressure.** Mitigation: sequential, large-block reads; no random access during the parse.
2. *CSR build.* O(m) with a random-access scatter: at 10×, the scatter's working set (10× the edges, 400MB) exceeds L3, so the scatter slows superlinearly. **This is the largest X hazard.** Mitigation: a cache-friendly layout — partition the edges by tail into buckets (radix partition) before scattering, so the scatter writes are sequential within a bucket. This costs an extra pass but converts a random-write scatter into a linear one. Decide by measurement at 1× (it may already be a win at 1×).
3. *Landmark preprocessing.* k full Dijkstras over a 10× bigger graph: each Dijkstra is O(m log n) → 10× the edges and 10× the nodes → ~10× the time (plus log factors). The 10× graph's landmark set would be 10× the memory (u32 × 6–8 landmarks × 17M nodes ≈ 550MB) — inside the 8GB cap but the dominant allocation. **Mitigation: the landmark count must not grow with the input.** k is a constant.
4. *Query phase.* The number of queries is fixed at 2,000 (the query file presumably does not grow 10×; DEFERRED). If the queries are the same, the per-query cost grows with the graph size — the search explores more nodes in a bigger graph (typically by a constant factor, not 10×, for a fixed random graph: the explored ball radius grows like log n, so the explored *count* grows slowly). **Expect the query phase to be nearly flat in the input size** → X is *helped* by the query phase. But note the comparison is against the *reference's* own 10× ratio, and the reference's 2,000 full searches would also grow slowly, so the comparison is roughly neutral.
5. *Memory.* Every structure scales linearly with the graph: CSR 400MB, reverse 400MB, landmarks 550MB, distances 140MB → a peak near 1.6GB at 10×, inside the 8GB cap. **The reverse CSR and the landmark array are the two items to watch.**
6. *The stamp array.* n × u32 scales linearly: 70MB at 10×. Fine.

**Safeguards.**
- **S1.** All per-node structures scale linearly and are budgeted: total 10× peak ≤ 4GB.
- **S2.** The landmark count k is a constant, never a function of n.
- **S3.** The distance/stamp arrays are reset by stamp, not by clearing — clearing cost scales with n per query, which at 10× would be 10× worse per query. (This is also an X-correctness of performance point: a clearing implementation would show X ≈ 10 × the per-query cost.)
- **S4.** Prefer the cache-friendly radix-partition-then-scatter layout for the CSR if measurement shows the random scatter is a significant fraction of the build time.
- **S5.** Avoid the reverse CSR if the query strategy does not need it — it is 400MB at 10× for nothing.
- **S6.** The query phase is parallel over a fixed 12 workers regardless of the graph size, so it does not add superlinearity.
- **S7.** Probe construction is DEFERRED (the organizers' 10× input is unpublished). We build our own 10× probe by concatenating the graph file ten times into dev scratch on the host volume (never /tmp: 1.8GB exceeds the 1GB tmpfs limit outright). **Note the semantic effect:** concatenating the graph produces a graph with 10× the nodes (the node ids repeat, so the graph becomes 10 disjoint copies if we re-index, or a malformed graph if we do not). The honest probe is: re-index the concatenated file so that node ids are unique per copy, preserving edge structure. That doubles as a test of the "not sorted by u" and "n from the header" assumptions. Document the probe construction in PERF.md so the number is interpretable.
- **S8.** Acceptance: X_local ≤ 1.15 and 10× peak ≤ 4GB. If X_local > 1.20, ablate: parse / CSR build / landmark preprocessing / query phase — identified within one hour, since the expected culprit is the CSR scatter.
- **S9.** Never trade a verified T win for an X fix: the 0.10 weight makes a 0.15 X excess cost 0.10·ln(1.15) = 0.014 on ln K (K ×0.986, ≈ −0.3 points) against 3+ points for a 20% T gain.

---

## 7. Hour Budget

Total charged to Track D: **12h = 2h baseline + 10h optimization**. The baseline is written inside the H0–H17 window; the optimization block is H33–H43, the earliest of the late blocks, because this track has the highest ceiling and the most implementation risk.

**Baseline block (2h, charged), H0–H2 nominal.**

| Hour | Activity | Exit criterion |
|---|---|---|
| H0–H0.5 | Read the Oct 28 frozen spec: the output contract (distance-only vs full path), the unreachable spelling, the output format, the exact file layout | Spec sheet; the two output unknowns are answered or flagged DEFERRED with a plan |
| H0.5–H1.5 | Baseline implementation: streaming parse, CSR build, per-query binary-heap Dijkstra with the stamp trick, single-threaded, spec-driven output | Runs end to end on the 10% sample |
| H1.5–H2 | V1: sample hash matches; measure T, memory.peak, the 5-run spread; count distinct query sources/targets (the L4 input) | Sample hash reproduced; the source/target cardinality is known |

**Optimization block (10h, charged, H33–H43).**

| Hour | Activity | Exit criterion |
|---|---|---|
| H33 | L1 (radix heap) + V2/V3 cross-check harness (random subgraphs) | Distances identical to the oracle on the sample and on the subgraph suite |
| H34 | L2 (bidirectional search) + the reverse CSR | V3 passes on thousands of subgraph query pairs; query time measured |
| H35 | L3 (ALT landmarks, k = 6) + V4 admissibility assertions | Every edge satisfies the potential bound; V5 equality on all sample queries |
| H36 | L3 continued: landmark count tuning (k = 4/6/8/12) against expansion counts and memory | The k that minimizes total time is chosen; memory.peak recorded |
| H37 | L4 (batched search by distinct source/target) — a runtime-adaptive choice | V5 equality; the distinct-source/target counts and the resulting query count are logged |
| H38 | L5 (parallel query evaluation) | V6 worker-count invariance; query time measured |
| H39 | L6 (parallel parse + parallel CSR build) + a CSR self-check (Σ degrees = m) | The CSR is identical across worker counts; build time measured |
| H40 | L7 (drop the reverse CSR if justified) + L9 (adversarial graph suite) + L10 (output path) | Adversarial suite passes; hash unchanged |
| H41 | **Verification day:** V3 at scale, V4 on the full edge set, V5 on every query, V6 (5 runs × 3 worker counts) | All green; this hour is not traded for speed |
| H42 | L8 (landmark pruning) + L11 stretch attempt 1 + the 10× X probe | X_local ≤ 1.15; 10× peak ≤ 4GB |
| H43 | Freeze: re-anchor the axis table to the batch-local reference; recompute K; write PERF.md (amortization table, budget breakdown, per-rung table, failed experiments); measure normalized source bytes | Submission frozen |

**Budget hygiene.**
- **H41 is a full verification hour.** A wrong distance in this track is a zero (Gate 1) and the search algorithms (bidirectional termination, ALT potentials) are exactly where subtle wrongness lives. The V3 random-subgraph cross-check is the highest-yield test and it is scheduled early (H33) so that a broken bidirectional rule is discovered in the hour it is built, not at H42.
- The CH rejection decision is made in the spec sheet at H0 and written up in PERF.md at H43 — it costs 15 minutes of analysis and is worth a 12% "technique" score if the judges agree with the reasoning (and the reasoning is verifiable arithmetic, not opinion).
- If the track is behind at H41, the remaining stretch work is cut and the verification hour is preserved. **Never cut H41 to buy H42.**
- If the spec turns out to require full paths, that is a scope change: it adds a predecessor array and a tie-break contract. Decision point at H0; if it is required, L3/L4 are simplified (drop ALT, keep bidirectional) to buy back the time.

---

## 8. Risk Register & Stop Rule

| ID | Risk | Likelihood | Impact | Detection | Mitigation | Stop rule / trigger |
|---|---|---|---|---|---|---|
| R-D1 | A non-optimal distance from a wrong bidirectional termination rule or a broken radix heap | **High** | **Total** (Gate 1 = 0) | V2 oracle; V3 random-subgraph cross-check; V5 equality on every query | Lazy-deletion heap (stale entries filtered on pop); one explicitly specified termination condition; brute-force cross-checks at every rung | Any distance mismatch → revert the offending rung immediately, never debug in place; if the mismatch persists after two reverts, fall back to plain single-direction Dijkstra (slower but trivially correct) |
| R-D2 | The output contract is unknown (distance-only vs full path; unreachable spelling; format) — DEFERRED to Oct 28 | Medium | **Total** | V1 sample hash | One output module with selectable policies; prefer distance-only; the sample hash selects the spelling | If unresolved at H41, emit candidate renderings and keep the hash-matching one; if full paths are required, treat it as a scope change and simplify the algorithm set to buy time |
| R-D3 | Full-path output required (tie-breaks, predecessor arrays, path reconstruction) | Low–Medium | High (schedule) | The Oct 28 spec | Distance-only preferred; if paths are required, drop ALT and simplify the search to a correct bidirectional Dijkstra, and add the predecessor array (n × u32 ≈ 7MB) | If paths are required at H0, re-plan the ladder at H0 and cut L3/L4 to fit |
| R-D4 | The ALT potential is not admissible (arithmetic, saturation, landmark storage) | Medium | **Total** (wrong distances) | V4 (per-edge admissibility assertions) | Potentials computed from true landmark distances with per-landmark offsets so the u32 storage is exact; the assertion runs on every edge | Any assertion violation → drop the landmark feature (fall back to plain bidirectional) and re-verify; do not "fix" the constant |
| R-D5 | The CSR build is wrong under parallelism (offset arithmetic) | Medium | **Total** (corrupt graph → wrong distances) | Σ degrees = m self-check; worker-count equality; V2 | Deterministic per-worker offset scheme; no atomics; a self-check that fails loudly | Any mismatch → revert to the single-threaded CSR build (0.25s cost) and re-verify |
| R-D6 | Time budget blown by preprocessing (the CH trap) | Low (we reject it) | High | The amortization table in the spec | CH rejected with the explicit calculation; landmark preprocessing capped at ~0.4s; the build-phase budget is tracked hourly | If preprocessing exceeds 25% of the budget, cut the landmark count before anything else |
| R-D7 | Memory blowup: the landmark arrays dominate and M collapses to ~1× | Medium | Medium (−2 to −8 points) | memory.peak at H1, H36, H39 | u32 with per-landmark offsets; k = 4–8; drop the reverse CSR if unjustified; no disk staging | If memory.peak exceeds 512MB at 1×, cut k first, then the reverse CSR, then the heap arena size |
| R-D8 | Query set cardinality is different from our assumption (many distinct sources) so L4 does not pay | Medium | Low (L4 is adaptive, so it just does not trigger) | The distinct-source/target counts logged at H2 | L4 is chosen at run time from the measured cardinality, not assumed | If distinct sources are many, L4 is skipped with no loss; do not spend further hours on it |
| R-D9 | The 10× probe is not a faithful graph (node id collisions when concatenating) | Medium | Low | Probe construction is documented and validated (the concatenated graph is re-indexed and its edge count is checked) | Re-index the concatenated file to keep node ids unique; assert the edge count and the node count | If a faithful probe cannot be built, report the 1×/2×/4× trend with the linear argument, in PERF.md |
| R-D10 | The graph is much denser than the fragment suggests (n small relative to m) | Medium | Medium (search cost per query rises) | n is read from the header; the average degree is logged | The algorithm is degree-agnostic (CSR + A*); the landmark preprocessing cost scales with m, which is capped by the budget | If the average degree exceeds ~10, re-anchor the T anchor down and re-plan the ladder in the first optimization hour |
| R-D11 | Integer overflow in distances or in the potential arithmetic | Low | **Total** | V7 adversarial graphs (u32::MAX weights) | u64 distances; the potential computed in u64 then stored with an offset; assertions in debug builds | Any overflow → disable the corresponding optimization and re-verify |
| R-D12 | Source-size overrun (S ratio below 0.55) | Medium | Low (−2 points) | Normalized byte count per rung | Budget ≤1.85× the reference; cut L11 and L8 first | Over budget → cut features until within 1.85× |
| R-D13 | Reference drift >2% voids a batch | Medium | Medium | Batch-local reference timings | Re-derive ratios from the batch-local reference | Discard and re-measure; do not re-tune to a drifted number |
| R-D14 | Verification overrun eats the stretch row | Medium | Medium | Hour tracking | H41 is fixed; the stretch attempts (L11) are the first thing cut | If behind at H41, ship the Commit row; the track total still includes the 35% judges' share, which verification and PERF.md protect |

**Global stop rule for Track D (in priority order).**
1. **Correctness stop:** if V2/V3/V5 show any distance mismatch at H41, the optimization ends and the last green state ships. The fallback is the simplest provably-correct search (single-direction Dijkstra with a lazy-deletion heap), which is still fast enough for a respectable score.
2. **Heuristic stop:** if the ALT potential cannot be proven admissible by the V4 assertion, the landmark feature is removed entirely — not patched.
3. **Preprocessing stop:** if preprocessing exceeds 25% of the T budget, the landmark count is cut before any query-phase work continues.
4. **Parallelism stop:** if the parallel CSR build ever disagrees with the single-threaded build, the single-threaded build is used permanently.
5. **Diminishing-returns stop:** two consecutive rungs below 3% ⇒ stop the ladder and spend the hours on verification and PERF.md.
6. **Freeze:** at H43 the last green, verified, deterministic build ships with the re-anchored axis table and a PERF.md whose centerpiece is the build-vs-query amortization analysis (including the CH rejection).

**Definition of done for Track D.** The published 10% sample hash is reproduced; the full-dataset output hash matches on 5 of 5 runs; every query's distance equals the brute-force oracle's; the ALT admissibility assertion holds on every edge; the CSR is identical across worker counts; memory.peak is recorded and ≤512MB at 1×; the 10× probe reports X_local ≤ 1.15; PERF.md contains the amortization table with the CH rejection, the budget breakdown, the per-rung measurements, and the failed experiments.
