# Track B — Index (SOLID, target K 6)

**Task:** build an inverted index over the corpus, then answer **50k queries (62% boolean / 38%
phrase)**. Scored time blends **40% build + 60% query**. Reference ≈ 58.7 s. Timeout 587 s.
*(Figures verbatim from the frozen context block.)*

> All K below are **TARGET/EXPECTED** — replace T/M/S/Bref/Qref/Mref with the Oct 28 frozen values
> and remeasure. **M is Track B's weakest axis** (positions dominate memory), so don't bank on it.

## Corrected K budget (recompute `∏(ref/ours)^w` on the box)

| Row | T | M | S | D | X | K | Speed |
|---|---:|---:|---:|---:|---:|---:|---:|
| Solid (target) | 30× | 12× | 0.62× | 1 | 1.12× | **6.03** | 55.8 |
| Commit (additive-blend) | 35× | 12× | 0.65× | 1 | 1.15× | **6.48** | 58.1 |
| Stretch (cap-maxed) | 50× | 20× | 0.60× | 1 | 1.20× | **8.21** | 65.4 |

**Headline caveat — the blend model.** Astra's §2 K's assume an **additive-ratio** time blend
`R_T = 0.40·rB + 0.60·rQ`. The more literal "40% build / 60% query" reading is a **weighted-time
(harmonic) blend** `R_T = 1 / (0.4/rB + 0.6/rQ)`, which is **always ≤ additive** and dominated by
the slower phase. Example: rB 20× / rQ 60× gives additive 44× but harmonic only **33.3×** (−10.5% K
on T). Build (tokenize + sort + encode, memory-bound) is plausibly the harder phase to accelerate,
so additive overstates K exactly where it hurts. **Treat the table as optimistic-case; prioritize
BUILD speedups more than a query-lean plan implies.** Pin the exact blend formula + Bref/Qref/Mref
from the frozen harness before trusting any K. **§11 "K 8–12" → cap is ~10; strike 12.**

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
- **Postings compression:** StreamVByte vs PForDelta head-to-head (delta-gap + block codec).
- Parallel: 6 P-cores default, sweep to 12; **deterministic dictionary merge + per-query result
  slots** (workers never write results directly in completion order).

## Optimization ladder (PERF.md spine; keep ≥3 failed experiments)
1. Correct baseline: authored inverted index + boolean/phrase → SHA-green.
2. Dense term IDs + sorted dictionary; drop per-token allocation.
3. Compressed postings (StreamVByte / PForDelta).
4. Smallest-first AND; rarest-anchor phrase.
5. **Build-side speedups** (parallel tokenize + merge) — the harmonic-blend priority.
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
- Risk: the time-blend formula (additive vs harmonic) materially changes the achievable T ratio —
  **this is the #1 B uncertainty.** Hours: Astra's §11 blocks sum ~27 h; the master plan buys only
  ~7 h here (solid, not deep) — bank K≈6 and move on.
