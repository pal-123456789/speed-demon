# Track D — Traverse (DEEP, highest ceiling, target K 7)

**Task:** a **5M-edge graph (~180 MB)**, **2000 shortest-path queries**; **preprocessing is counted
inside the timed run.** Reference ≈ 72.4 s (the highest reference → the highest cap, ~9.7). Timeout
724 s. *(Figures verbatim from the frozen context block.)*

> Output contract (distance-only vs full path) and the unreachable spelling are **deferred to the
> frozen spec** — distance-only makes tie-breaks moot and lowers CH risk. All K are **TARGET/EXPECTED**.

## Corrected K budget (recompute `∏(ref/ours)^w` on the box)

| Row | T | M | S | D | X | K | Speed |
|---|---:|---:|---:|---:|---:|---:|---:|
| Commit | 25× | 8× | 0.55× | 1 | 1.05× | **5.04** | 50.3 |
| Stretch | 40× | 12× | 0.58× | 1 | 1.10× | **6.69** | 59.0 |
| Exceptional | 50× | 15× | 0.50× | 1 | 1.15× | **7.51** | 62.6 |

Astra **understated** the stretch/exceptional rows by ~8% (stated 6.14 / 6.9 → actual **6.69 /
7.51**); commit 5.04 was correct. **The understatement does NOT make D easier** — the *ratios*
(T 40–50×, M 12–15× with preprocessing inside the timed run) are aggressive. Bank the ratio
difficulty, not the higher K. **§11 "6–9" is within D's ~9.7 cap; realistic band K≈5–7.**

## Algorithm & data layout — a portfolio with hard rollback
- **Bidirectional Dijkstra on forward + reverse CSR** = the permanent, always-correct rollback. The
  reverse CSR is a *traversal structure*, not a claimed `v→u` edge; test on a directed `0→1` case.
- **ALT (A\* + landmarks)** = a **bounded experiment** (landmark count sweep).
- **Contraction Hierarchies** = a **time-boxed stretch** behind hard abort gates: shortcut cap ~2 m,
  memory cap ~4 GB, a preprocessing break-even budget, and **1 mismatch ⇒ CH removed.**
- **§2.5 preprocessing break-even (the standout):** the 2000 queries run **in parallel**, so a
  per-query saving must be divided by `W_eff (~12)` before comparing to preprocessing cost. The
  "sequential-query illusion" (2000 × 18 ms = 36 s *looks* like a huge CH budget, but ÷12 ≈ 3 s)
  kills naive CH enthusiasm for only 2000 queries. **Preprocessing must pay for itself inside the
  timed run.**
- **Generation stamps** instead of O(Q·n) visited-array clears. `u64` checked/saturating add; **`u32`
  distances prohibited** without a proven frozen weight bound.

## Optimization ladder (PERF.md spine; keep ≥3 failed experiments)
1. Correct bidirectional Dijkstra on fwd+rev CSR → SHA-green.
2. Generation-stamp scratch (no per-query clears).
3. Tuned priority queue / early termination on the meeting condition.
4. ALT landmarks (bounded) — keep only if it beats plain bidirectional within the time budget.
5. CH (time-boxed) — keep only if it wins **both** the 1× total and the 10× (X-axis) ratio.
Likely failed/reverted: CH shortcut explosion (fails X-axis at 10× scale), too many landmarks
(memory blows the M axis), 12 workers on query (contention) — document the break-even math.

## Determinism checklist
- [ ] Fixed tie-break (if path output is required): diamond / input-order / heap-tie suite exercised.
- [ ] Distances are integer; no float accumulation.
- [ ] Query outputs in frozen order; unreachable spelled per the ref.
- [ ] 5-run (10 for the final candidate) byte-identical; CH removed on any mismatch.

## Memory & risk
- CSR fwd+rev ≈ **93.9 MB** (u32 offsets); landmarks `2·k·n·8B` = **111 MB @ k=4 … 891 MB @ k=32**;
  per-worker scratch `2·u64·n·12` ≈ **334 MB**. Keep M well under 8 GB.
- **X-axis is the CH killer:** at 10× scale, preprocessing-in-timed-run means CH must still win the
  ratio; shortcut explosion fails it. Defer the 10× construction test to the harness.
- D and A hold the two real ceilings — this is a deep track; spend the full ~12 h block.
