# DEPTH DEPLOYMENT — The Power-House Blueprints (pre-kickoff design arsenal)

> **Status: PRE-KICKOFF DESIGN. No solution code.** Rules 04/05 explicitly permit
> reading, designing, and preparing the environment before Oct 30 18:00 UTC.
> Nothing in this directory is code; everything is decision — the exhaustive
> feature inventory, the optimization ladder, the determinism traps, and the
> hour-by-hour window runbook for each track.
>
> These are the **second-level deep dives**. The first level is `docs/specs/*.md`
> (contract teardown and axis budgets); this directory is the *arsenal*: every
> feature we could implement, ranked and priced, so the 72 hours are spent
> choosing, not discovering.

---

## What this project is, in the real world

This is a four-engine data-processing power plant. Each track is a production
system you would pay actual money for, reduced to a 72-hour build:

| Track | The power-house engine | Real-world sibling |
|---|---|---|
| **A · Ingest** | Latency/epoch aggregator with exact percentiles and error rates | Prometheus-grade SLO monitoring, API analytics |
| **B · Index** | Inverted-index full-text search with boolean + phrase queries | Elasticsearch-lite, ripgrep-grade tokenizer, document retrieval |
| **C · Join** | Hostile-CSV ETL join + group aggregation with money-safe arithmetic | Billing pipelines, dataframe-style ETL over untrusted input |
| **D · Traverse** | Weighted shortest-path engine over millions of edges | Road-network routing, game-pathfinding, network telemetry graphs |

The four share one doctrine: **deterministic output, bounded memory, zero
non-stdlib dependencies, and code terse enough that the S axis doesn't eat the
score.** That doctrine is what makes a "comfortable" build a *power house*.

---

## The scoring physics (recite before every hour of the window)

```
K  = (T_ref/T)^0.50 × (M_ref/M)^0.20 × (S_ref/S)^0.15 × (D_ref/D)^0.15
speed = 50 × log10(K), clamped [0,100];  track total = 0.65×speed + 0.35×judge
overall = geometric mean of the BEST THREE track totals (≥3 tracks entered)
```

- **T has half the weight — half your hours go to time.** M caps at 50×. S and D
  can only cost. Three gates: correctness (SHA), timeout (10× ref), sandbox.
- **The S axis is real.** S = code lines by `scc` **per track folder**. The
  reference impls are 56 lines (A). A clean 150-line implementation gives an S
  factor of (56/150)^0.15 ≈ **0.86** — that's ~1.4 speed points lost for the
  luxury of 150 lines. 300 lines costs ≈ 0.78. **Terse, documented, tight.**
- **Tie-breaks:** raw K, then wall time, then earliest commit — every
  optimization also buys tie-break safety.

## Win-condition table (the committed K budgets, from `docs/specs/*`)

| Track | ref T | Safe K / speed | Commit K / speed | Stretch K / speed |
|---|---|---|---|---|
| A | 32.05 s | 6.57 / 40.9 | 8.76 / 47.1 | 11.67 / 53.4 |
| B | 56.35 s | (re-derive at Oct 28 — S_ref DEFERRED) | | |
| C | 14.44 s | (re-derive at Oct 28) | | |
| D | 72.69 s | (highest ref → most T headroom) | | |

Absolutes to hit the **Commit** row (T axis): A ≈ 1.07 s, B ≈ 1.88 s,
C ≈ 0.48 s, D ≈ 2.42 s — on a pre-warmed page cache, 12 CPUs, 8 GB cap,
no_turbo ≈ 2.5 GHz. These numbers are why single-threaded "reasonable Rust"
is not enough: they demand the ladder tops.

## Best-3-of-4 strategy

The overall is the geometric mean of your best three. Concretely:

- **All four must be SHA-green** (a zero track ruins the mean).
- The **drop candidate** is the track whose marginal hour buys the least K.
  With equal effort, keep the three with the highest K — but a track at speed
  ~25 (K≈3) still contributes; don't drop a track that is merely "second best".
- D has the largest reference time (72.69 s) → the most absolute headroom; A
  has the strictest parse correctness. The typical power-house allocation:
  **A and C get the parser rigor hours; B and D get the algorithm hours.**

## Cross-track doctrine (the immutable seven)

1. **Determinism is correctness.** No `HashMap` iteration order anywhere; fixed
   merge orders; integer money math; byte-identical output across runs on any
   thread count. The SHA gate is binary — nondeterminism is a zero, not a warning.
2. **D = 1 forever.** Std-only. One crate costs K × 0.90.
3. **S discipline per track folder.** Target ≤ 150 code lines per track at H0+17,
   ≤ 260 total (impl + io). The io.rs scaffold (56 lines) is already there — reuse.
4. **Scratchless.** No temp files, no /tmp staging (inflates `memory.peak`),
   no reads of /data except the input; the image ships no cached outputs.
5. **Buffered reads, no mmap** by default; page cache is pre-warmed by the
   organizers (`vmtouch`), so sequential buffered reads hit hot RAM.
6. **12 threads max** (`--cpuset-cpus=0-11`); oversubscription hurts at ~2.5 GHz.
7. **SHA-green before speed.** Every ladder rung re-runs the SHA gate, then the
   5-run trimmed-mean bench, against the SAME session's reference. A >2% drift
   in the reference voids a batch, and batches are the evidence judges read.

## The ladder shape (every track)

```
L0  reference-equivalent baseline     (H0–H3)  — correct, slow, deliberate
L1  correct single-thread Rust        (H3–H6)  — SHA-green, first real numbers
L2  parallel + allocation hygiene     (H6–H24) — the big T jump
L3  insane: SIMD/encode/precompute    (H24+)   — only after L2 is SHA-green
```

Each rung has a measurement gate (below). You do not climb with a red SHA.

## Files

- `track-A-INGEST-DEEP.md` — the latency/percentile engine, every feature.
- `track-B-INDEX-DEEP.md` — the inverted-index search engine, every feature.
- `track-C-JOIN-DEEP.md` — the hostile-CSV join, the quoting oracle, every parse detail.
- `track-D-TRAVERSE-DEEP.md` — the shortest-path engine, every algorithm.