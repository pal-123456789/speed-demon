# TRACK D · TRAVERSE — INSANE-LEVEL DEEP DIVE

> **MODEL-DERIVED UPGRADES (external model, see `97-MODEL-OUTPUT-DIGEST.md` §6):**
> (1) **Two-pass CSR build, no edge buffering** — pass 1 parses u only +
> `deg.fetch_add`, scan → cursor = row ends, pass 2 `fetch_sub` places
> `(w<<32)|v` packed u64 edges (40 MB) with `off: [u32; n+1]`; ~30–40 ms total.
> (2) **Radix heap** (33 buckets, u64 `key<<32|node` items, eager normalization)
> + the **meet condition with proof**: exit when `tf + tb >= mu`, μ updated
> UNCONDITIONALLY per relaxed edge (`if O.dist[v] finite: mu = min(mu, nd +
> O.dist[v])` even when nd didn't improve); "settled by both sides" is NOT a
> valid exit. (3) **Overflow:** u64 labels unconditionally safe; `d+w` and
> `tf+tb` overflow u32 even for small D → u64 registers; prune at
> Rcap=min(R,2^32−1) with a rerun flag. (4) **Symmetric-reverse shortcut:** if
> Σ mix(u,v,w) == Σ mix(v,u,w) (wrapping u64), the forward CSR serves as the
> backward view (save 44 MB). (5) Per-thread open-addressing scratch (2^16
> slots/direction, 12 MB total vs 96 MB dense), 12 threads (SMT pays on
> latency-bound loops), prefetch `edge[off[v]]` on label improvement.
> (6) SIMD on the search loop: skip (degree ≈ 5; no scatter for labels).

> Pre-kickoff design. No solution code. Companion to `docs/specs/track-d-traverse.md`.
> **Engine:** weighted shortest-path over millions of edges → road-network routing.
> **Ladder goal:** L3. **Contract anchors:** ref **T 72.69 s** (highest ref in the
> event → most absolute headroom), timeout 727 s; `graph.txt` = header `n m`, then
> `m` lines `u v w` (**u32 × 3**: tail, head, weight), **not sorted by u**, ids
> dense 0-indexed, **directed**, reverse edges listed separately (the reverse
> graph must be built); graph is **92 MB / 5 M edges**; queries ≈ 2 k, **LOCAL
> and drawn at a BOUNDED search distance** (events page); distance-only or full
> path (DEFERRED); unreachable spelling DEFERRED. **Absolutes:** Commit
> T ≈ **2.42 s** (30×), Stretch ≈ 1.62 s (45×).
> **Organizer hint (events page):** "fast parsing, a compact layout and a lean
> search loop decide this track. ... Contraction hierarchies are legal, but this
> query mix rarely repays them" → **the L3 plan is parsing + CSR layout + lean
> bidirectional search; heavy preprocessing (CH/ALT) is explicitly
> DEPRIORITIZED** — spend those hours on the parser and the layout instead.

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
| D21 | **Fast edge-list parsing** (SIMD digit scanning, no per-line allocation, bulk u32 decode) | The organizer hint is explicit: "A fast submission spends much of its time parsing the edge list" and "fast parsing ... decide[s] this track" — the parser is the L3 room | keep the scalar parse for the first SHA |
| D22 | **Compact CSR variant**: heads as u32, offset array, weights u32 in a parallel slice; prefer array-of-structs vs struct-of-arrays decided by the bench | "Memory layout beats asymptotics at this size, consistently" (events page) | measure, don't guess |
| D23 | **Lean bidirectional search loop** with an efficient meet condition and early exit | "queries are local ... bounded search distance ... a lean search loop decide[s] this track" | the front-loaded distance arrays |
| D24 | **SIMD/sorted-neighbor relax** — when the frontier is dense, process the outgoing slice with wide loads | micro | depends on layout |
| D25 | **Full-path recovery via parent arrays only when D03 = full path** | parent[u] u32 per node per query thread (bounded queue) | memory: 2 × n × u32 transient per thread |

> **CH/ALT — deprioritized.** The events page says the bounded-distance query
> mix "rarely repays" contraction hierarchies; leave them as a measured H24+
> experiment ONLY if L2 + parsing work is done and the bench says the query
> loop still dominates. Do not pre-build preprocessing before the parser is fast.

## 2. The ladder

```
L0  reference-equivalent (python, ~72.7 s)      H0–H1   — the SHA oracle
L1  correct single-thread Dijkstra on CSR       H1–H4   — target ~8–10 s, SHA-green
L2  bidirectional + radix heap + parallel       H4–H12  — target ~2.4 s
L3  insane (D21–D23: fast edge parse, compact   H12+    — target ~0.8–1.2 s; stop when <0.3 pts/hour
    layout, lean search; CH/ALT only as a
    measured H24+ experiment if query time still dominates)
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
- If L3 experiments with CH/ALT: preprocessing would be single-threaded (or
  2-phase parallel contraction); queries stay embarrassingly parallel on the
  CH graph. Per the events page hint, treat this as a measured experiment, not
  a default.

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
4. **H12+:** L3 = fast edge-list parsing + compact layout + lean bidirectional
   search (D21–D23); CH/ALT only if the L2 bench still shows query time
   dominating AND the parser is already fast. Stop when the hour buys < 0.3 points.
5. **Freeze-day:** determinism ×3, final SHA, PERF.md ladder with run IDs.

## 9. Open questions → Oct 28

- Distance-only vs full path (D03) — the biggest structural fork.
- Unreachable spelling (D04), u32-distance bound (D05), tie-break rule (D07),
  parallel-edge/self-loop policy (D06 edge matrix).
- CH/ALT: only as a measured H24+ experiment if the L2 query loop still
  dominates — the events page says this query mix "rarely repays" them.