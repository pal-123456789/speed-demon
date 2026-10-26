# TRACK D · TRAVERSE — INSANE-LEVEL DEEP DIVE

> Pre-kickoff design. No solution code. Companion to `docs/specs/track-d-traverse.md`.
> **Engine:** weighted shortest-path over millions of edges → road-network routing.
> **Ladder goal:** L3. **Contract anchors:** ref **T 72.69 s** (highest ref in the
> event → most absolute headroom), timeout 727 s; `graph.txt` = header `n m`, then
> `m` lines `u v w` (**u32 × 3**: tail, head, weight), **not sorted by u**, ids
> dense 0-indexed, **directed**, reverse edges listed separately (the reverse
> graph must be built); queries ≈ 2 k DISTANCE-only or full path (DEFERRED);
> unreachable spelling DEFERRED. **Absolutes:** Commit T ≈ **2.42 s** (30×),
> Stretch ≈ 1.62 s (45×).

---

## 1. The complete feature inventory

### P0 — correctness-critical

| # | Feature | Why it exists / the trap | Oct-28 pin needed |
|---|---|---|---|
| D01 | **CSR construction from the edge list** (u,v,w u32; ids dense; not sorted → sort by u) | The adjacency representation everything else sits on; CSR is what makes both forward and reverse traversals cache-clean | — |
| D02 | **Reverse graph built from the separated reverse edges** | The page lists reverse edges SEPARATELY — you cannot assume symmetry; bidirectional algorithms need the true reverse adjacency | — |
| D03 | **Distance-only vs full path output contract** | Full path needs parent pointers + recovery; distance-only needs none — a 2× memory/pointer difference | output contract |
| D04 | **Unreachable spelling** (what exactly prints when no path exists) | The SHA sees it | unreachable spelling |
| D05 | **Distance arithmetic overflow safety** | u32 weights on a 5 M-edge graph can exceed u32 if distances are long — prove the max bound, or carry u64 internally | max distance bound |
| D06 | **Deterministic query order in the output** | 2 k queries → 2 k lines, in query-file order | — |
| D07 | **Deterministic tie-breaking at equal distances** | If two paths tie, the CHOSEN one affects parents (full-path mode) and any printed path node; tie-break must be a fixed rule (by node id), never heap-order | tie-break rule (matters only if paths print) |
| D08 | **Zero/negative weights policy** | zero-weight edges allowed? negative impossible in shortest-path — reject or ignore | weights are u32 ≥ 0 |

### P1 — the Commit row

| # | Feature | Why | Notes |
|---|---|---|---|
| D11 | **Bidirectional Dijkstra with a binary heap + an efficient stop condition** | The textbook win: two frontiers meeting. With reverse CSR (D02) this is the L2 workhorse | λ (meet) tracking |
| D12 | **Cache-friendly CSR walks** (neighbor iteration over contiguous u32 slices; weights in a parallel array) | The inner loop is neighbor scans — make it a linear stride | — |
| D13 | **Per-query parallel evaluation over 12 threads** (2 k queries are independent) | T is 0.50 weight; 2 k × bidirectional Dijkstra is the whole time budget | immutable graph → lock-free |
| D14 | **Bucket/heap tuning**: radix heap on u32-weights (integer buckets, O(1) amortized per relax) | ~2–3× faster than a binary heap for integer weights, standard literature result | — |
| D15 | **Early-exit queries** — stop the search the moment the meet condition is proven minimal | Avoids exploring the full graph for every query | depends on D03 (distance-only helps) |
| D16 | **Node-id → dense u32 remap confirmed** (ids dense 0-indexed per the spec) | Enables plain arrays, no hashing | — |

### P2 — the insane toppings

| # | Feature | Why | Risk |
|---|---|---|---|
| D21 | **Contraction Hierarchies (CH)** — preprocessing the graph once; queries become bidirectional Dijkstra on 2 small upward/downward DAGs | The event's heavyweight: preprocessing can take 30–120 s on 5 M edges, but the 2 k queries then run in ~10–50 µs each. Since T is ONE total wall time, CH pays if preprocess + queries < plain-2k-queries | preprocessing cost counts against T; only worth it if L2 query time dominates; CH legal (and "rarely necessary" per our spec notes — verify against the frozen spec) |
| D22 | **ALT (A\* + landmarks)**: a handful of landmarks precomputed once; A\* on the original graph with the triangle-inequality bound | Cheaper to implement than CH, still reads 2 frontiers | landmark choice affects quality |
| D23 | **SIMD/sorted-neighbor relax** — when the frontier is dense, process the outgoing slice with wide loads where the CPU allows | micro | depends on layout |
| D24 | **Weighted radix + bucketed dial if weights have a small max** | Dial's algorithm is O(E + C·V) — ideal for small C | only if weight range is small |
| D25 | **Full-path recovery via parent arrays only when D03 = full path** | parent[u] u32 per node per query thread (bounded queue) | memory: 2 × n × u32 transient per thread |

## 2. The ladder

```
L0  reference-equivalent (python, ~72.7 s)      H0–H1   — the SHA oracle
L1  correct single-thread Dijkstra on CSR       H1–H4   — target ~8–10 s, SHA-green
L2  bidirectional + radix heap + parallel       H4–H12  — target ~2.4 s
L3  insane (CH or ALT, D21/D22)                 H12+    — target ~0.8–1.2 s; stop when <0.3 pts/hour
```

## 3. Determinism traps (specific to D)

- **Heap iteration order must never decide the answer** — for DISTANCE-only it
  cannot (distances are exact minima), but for FULL-PATH the parent/recovery
  choice at a tie must be a fixed rule (e.g., smallest node id), applied
  identically across thread counts.
- **Parallel query scheduling** — each query writes to its own output slot;
  the 2 k-line file is emitted in query-file order.
- **Integer overflow** — relaxing a u32 accumulator is a silent wrong answer;
  carry u64 for distances AND for the "max distance" bound check (D05).
- **Graph load order** — the CSR build sorts by u; the sort must be stable
  w.r.t. nothing (it's an adjacency, not an output), but the adjacency ORDER
  inside a list should be deterministic (input order after sort) so full-path
  parents are reproducible across runs.

## 4. Memory budget (M target « 1 GB; the graph is ~60–80 MB CSR)

| Structure | Size (5 M edges, ~n nodes) |
|---|---|
| Forward CSR (head[] u32 + offset[] u32 + weight[] u32) | ~3 × (m) × 4 B ≈ 60 MB + n×4 |
| Reverse CSR (same again) | ~60 MB |
| Per-query frontier + dist arrays (per thread, transient) | 2 × n × 8 B × threads — **reuse** between queries; don't allocate per query |
| CH preprocessing (if L3) | +1–2× adjacency briefly, or disk-free incremental | keep under cap |

The 8 GB cap is far away; the discipline is reusing per-thread scratch arrays so
`memory.peak` doesn't spiky.

## 5. Parallelism plan

- **Load:** one thread builds CSR from the edge sort; the sort itself can be a
  parallel (stable) sort over m edges — cheap and deterministic.
- **Queries:** static partition of the 2 k queries across 12 workers; each
  worker owns its scratch (dist[], heap, parents) — allocated ONCE at start,
  reused per query, freed at the merge.
- If L3 = CH: preprocessing is single-threaded (or 2-phase parallel
  contraction); queries stay embarrassingly parallel on the CH graph.

## 6. Edge-case matrix

| Input | Decision | Fallback |
|---|---|---|
| Query asks for the same node (u == v) | distance 0 — never run a search | constant-time |
| u == v with a positive self-loop | ignore the loop, answer 0 | — |
| Unreachable target | the pinned spelling (D04) | spec |
| Parallel edges u→v with different weights | keep the minimum? first? | spec (min is natural) |
| Multiple reverse edges | same rule | — |
| Node id out of range in a query | skip/error policy | spec |
| Empty graph (n=0) or m=0 | — | all queries unreachable |
| Very heavy first line / short file | — | header parse exact: `n m` |

## 7. Validation plan

- `difftest` vs L0 on the sample graph + adversarial fixtures (dense small
  graphs with many equal-distance ties for the full-path rule; star graphs;
  a single long chain to stress overflow).
- **Overflow proof:** compute the true max finite distance in the sample;
  assert it fits u32 if the spec allows u32 answers — else switch internals to u64.
- Determinism ×3 (1/6/12 threads): byte-identical output.
- The SHA gate pins `traverse.txt`; re-pin per rung.

## 8. H0 window runbook (first 6 hours)

1. **H0–H1:** freeze D03 (distance-only vs full path), D04 (unreachable
   spelling), D05 (max bound). If distance-only and bound is small: u32
   internals and radix heaps are safe. Tag spec notes.
2. **H1–H4:** L1 — CSR + reverse CSR + single-thread Dijkstra, SHA-green.
3. **H4–H12:** L2 — bidirectional + radix + parallel queries; bench per rung.
4. **H12+:** decide CH vs ALT on MEASURED L2 query times; implement the winner;
   stop when the hour buys < 0.3 points.
5. **Freeze-day:** determinism ×3, final SHA, PERF.md ladder with run IDs.

## 9. Open questions → Oct 28

- Distance-only vs full path (D03) — the biggest structural fork.
- Unreachable spelling (D04), u32-distance bound (D05), tie-break rule (D07),
  parallel-edge/self-loop policy (D06 edge matrix).
- Whether CH/ALT preprocessing competes against the single total wall time the
  same way build-time does for B (it does — one clock).