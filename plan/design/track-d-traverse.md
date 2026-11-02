# Track D — Traverse (DEEP, target K 9–11)

**Task:** a **5M-edge graph (92 MB)**, **2000 shortest-path queries**; **preprocessing is counted
inside the timed run.** Reference ≈ 72.69 s. Timeout 727 s. *(These figures are the published
reference numbers.)*

> The organizers' hints now match the data: **queries are local, so parsing and memory layout decide
> most of the time.** Output contract (distance-only vs full path) and the unreachable spelling are
> **deferred to the frozen spec** — distance-only makes tie-breaks moot. All K are **TARGET/EXPECTED**.

## Corrected K budget (recompute `∏(ref/ours)^w` on the box)

`K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15`; speed score = `50·log10(K)`,
clamped to 0–100. Wall time is uncapped; memory is capped at 50×; S and D can cost but never pay.

| Row | T | M | S | D | K | Speed |
|---|---:|---:|---:|---:|---:|---:|
| Commit | 25× | 8× | 0.55× | 1 | **6.93** | 42.0 |
| Stretch | 40× | 12× | 0.58× | 1 | **9.58** | 49.1 |
| Exceptional | 50× | 15× | 0.50× | 1 | **10.95** | 52.0 |

Astra **understated** the stretch/exceptional rows by ~8% (stated 6.14 / 6.9 → actual **6.93 /
9.58**). The understatement does NOT make D easier — the *ratios* (T 40–50×, M 12–15× with
preprocessing inside the timed run) are aggressive. Bank the ratio difficulty, not the higher K. The
old "6–9 is within D's cap" reading is void: **T is uncapped**, so the ceiling is set by effort.

## Algorithm & data layout — a portfolio with hard rollback
- **Bidirectional Dijkstra on forward + reverse CSR** = the permanent, always-correct rollback. The
  reverse CSR is a *traversal structure*, not a claimed `v→u` edge; test on a directed `0→1` case.
- **ALT (A\* + landmarks)** = a **bounded experiment** (landmark count sweep).
- **Contraction Hierarchies** = **legal, but this query mix rarely repays them.** They stay off the
  ladder: shortcut cap ~2 m, memory cap ~4 GB, a preprocessing break-even budget, and **1 mismatch ⇒
  CH removed.**
- **§2.5 preprocessing break-even (the standout):** the 2000 queries run **in parallel**, so a
  per-query saving must be divided by `W_eff (~12)` before comparing to preprocessing cost. The
  "sequential-query illusion" (2000 × 18 ms = 36 s *looks* like a huge CH budget, but ÷12 ≈ 3 s)
  kills naive CH enthusiasm for only 2000 queries. **Preprocessing must pay for itself inside the
  timed run.**
- **The hints now match the data: queries are local, so parsing and memory layout decide most of
  the time.** A cheap custom numeric parse of the edge text and a cache-friendly CSR build are
  therefore higher-yield here than a fancier graph index.
- **Generation stamps** instead of O(Q·n) visited-array clears. `u64` checked/saturating add; **`u32`
  distances prohibited** without a proven frozen weight bound.

## Optimization ladder (PERF.md spine; keep ≥3 failed experiments)
1. Correct bidirectional Dijkstra on fwd+rev CSR → SHA-green.
2. Generation-stamp scratch (no per-query clears).
3. Tuned priority queue / early termination on the meeting condition.
4. ALT landmarks (bounded) — keep only if it beats plain bidirectional within the time budget.
5. Custom edge-text parse + cache-friendly CSR layout — the local-query mix means parsing and memory
   layout decide most of the time.
Likely failed/reverted: CH shortcut explosion (preprocessing never repays itself at 2,000 queries),
too many landmarks (memory blows the M axis), 12 workers on query (contention) — document the
break-even math.

## Determinism checklist
- [ ] Fixed tie-break (if path output is required): diamond / input-order / heap-tie suite exercised.
- [ ] Distances are integer; no float accumulation.
- [ ] Query outputs in frozen order; unreachable spelled per the ref.
- [ ] 5-run (10 for the final candidate) byte-identical; CH removed on any mismatch.

## Memory & risk
- CSR fwd+rev ≈ **93.9 MB** (u32 offsets); landmarks `2·k·n·8B` = **111 MB @ k=4 … 891 MB @ k=32**;
  per-worker scratch `2·u64·n·12` ≈ **334 MB**. Keep M well under 8 GB. The M axis is capped at 50×;
  T is not capped.
- **CH is a poor trade here, not a forbidden one:** preprocessing sits inside the timed run, and at
  2,000 local queries the shortcut build never repays itself. Document the break-even math rather
  than deleting the reasoning.
- D and A hold the two real ceilings — this is a deep track; spend the full ~12 h block.
