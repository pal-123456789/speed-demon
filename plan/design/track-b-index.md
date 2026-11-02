# Track B — Index (SOLID, target K 8)

**Task:** build an inverted index over the corpus, then answer **50k queries (62% boolean / 38%
phrase)**. Scored as **one total wall time** — build and query are no longer measured separately.
Reference ≈ 56.35 s. Timeout 564 s. *(These figures are the published reference numbers.)*

> All K below are **TARGET/EXPECTED** — replace T/M/S refs with the batch-local values and
> remeasure. **M is Track B's weakest axis** (positions dominate memory), so don't bank on it.

## Corrected K budget (recompute `∏(ref/ours)^w` on the box)

`K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15`; speed score = `50·log10(K)`,
clamped to 0–100. Wall time is uncapped; memory is capped at 50×; S and D can cost but never pay.

| Row | T | M | S | D | K | Speed |
|---|---:|---:|---:|---:|---:|---:|
| Solid (target) | 30× | 12× | 0.62× | 1 | **8.38** | 46.2 |
| Commit | 35× | 12× | 0.65× | 1 | **9.12** | 48.0 |
| Stretch (cap-maxed) | 50× | 20× | 0.60× | 1 | **11.92** | 53.8 |

**Headline caveat — there is no build/query split any more.** The 40/60 weighting is gone: Track B
is timed as **one total wall time**, and the queries are readable from the start. That removes the
blend-robustness problem entirely — a build speedup and a query speedup are now worth exactly the
same, in the same T. It also means the build can no longer be traded against the query phase, so
**spend the hours wherever the measurement says the time actually is**, and measure build and query
separately *for our own diagnosis only* — the score does not see them.

## Algorithm & data layout
- **Authored codec, D=1.** No ready-made search engine / off-the-shelf index library.
- **Stopwords RETAINED** (dropping them breaks phrase correctness). **Positional postings for ALL
  indexed terms** — never "positions only for the sample's query terms" (that's holdout overfit → DQ
  risk).
- **Boolean semantics:** preserve the frozen **left-associative, no-parenthesis** evaluation; do not
  impose conventional AND/OR precedence. Pin semantics to the reference.
- **AND:** smallest-postings-first intersection, but only across subruns proven
  associative/commutative under the frozen left-assoc rule.
- **Phrase:** rarest-term anchor, then positional verify.
- **Postings compression:** StreamVByte vs PForDelta compared side by side (delta-gap + block codec).
- Parallel: 6 P-cores default, sweep to 12; **deterministic dictionary merge + per-query result
  slots** (workers never write results directly in completion order).

## Optimization ladder (PERF.md spine; keep ≥3 failed experiments)
1. Correct baseline: authored inverted index + boolean/phrase → SHA-green.
2. Dense term IDs + sorted dictionary; drop per-token allocation.
3. Compressed postings (StreamVByte / PForDelta).
4. Smallest-first AND; rarest-anchor phrase.
5. **Build-side speedups** (parallel tokenize + merge) — under one total wall time a build win is
   worth exactly as much as a query win.
6. Parallel query execution into fixed result slots.
Likely failed/reverted: positions-only-for-sample-terms (overfit, reverted on principle),
conventional Boolean precedence (SHA mismatch), 12 workers on build (memory-bandwidth bound → no win).

## Determinism checklist
- [ ] Term IDs assigned by a deterministic sorted/stable rule; dictionary merged in byte order.
- [ ] Query results emitted in the frozen result order (no hash-map iteration).
- [ ] Boolean evaluation matches the frozen left-assoc semantics exactly.
- [ ] 1-worker vs 12-worker output byte-identical across ≥10 runs.

## Memory & risk
- **M is the weakest axis** — positional postings dominate the footprint; keep postings compressed
  and stream the build. Prefer a **buffered streaming read over mmap** (mmap may inflate
  `memory.peak`); measure both.
- Risk: the time-budget split between build and query is unknown, so the achievable T ratio
  depends on where our own profile says the time is. **This is the #1 B uncertainty.** Hours:
  Astra's §11 blocks sum ~27 h; the master plan buys only ~7 h here (solid, not deep) — bank
  K≈8 and move on.
