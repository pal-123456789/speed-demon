# The plan: from 55.997 to the field's frontier

Written while four workstreams run. Every number here is checkable against
`verification/` and the live board; nothing is a target dressed up as a result.

## 1. Where we stand, against the two teams above us

```
us  55.997   rank 41 of 76
#1  79.389   Cause Of Death     A 85.689  B 74.635  C 78.237  D 72.994 (dropped)
#2  77.833   NickTrapp          A 85.793  B 68.990  C 79.664  D 66.800 (dropped)
```

The product must rise **2.85x** for #1 and **2.69x** for #2.

### The gap, per track, split into the three axes that produce it

| Track | our K | #1 K | ratio | from T | from M | from S |
|---|---|---|---|---|---|---|
| A | 16.422 | 51.733 | **3.15x** | 2.614 (**82%**) | 1.211 (18%) | 1.010 (1%) |
| B | 11.190 | 31.095 | **2.78x** | 1.930 (**63%**) | 1.398 (**33%**) | 1.040 (4%) |
| C | 12.646 | 36.707 | **2.90x** | 2.410 (**84%**) | 1.072 (7%) | 1.118 (**11%**) |
| D | 9.378 | 28.833 | **3.08x** | 2.619 (**85%**) | 1.212 (21%) | 0.968 (**-3%**) |

**Time is 63-85% of every gap.** That single fact decides the shape of the plan: A, C
and D cannot be reached by memory and source-size work, and B only partly.

## 2. THE GOVERNING CONSTRAINT, stated once

The overall is the **geometric mean of the best three**. Ours are A 60.771, C 55.097,
B 52.442; **D is dropped at 48.604**. Therefore:

- Improving any counting track by X% raises the overall by **X^(1/3)**.
- **D contributes nothing at all until it EXCEEDS B.** Measured against the board's
  own numbers, taking D's memory all the way from 178 MB to the leader's 68 MB:

```
D  K 9.378 -> 11.37    points 48.60 -> 52.80
best three become A 60.771, C 55.097, D 52.80   ->   56.15   (+0.15)
```

**Nine points of work on D is worth 0.15 on the overall**, because it swaps D for B at
almost the same value. Any hour spent on D before it clears B by a real margin is an
hour that does not count.

## 3. PHASE 1 — bank what is in flight (0-2 hours)

Four workstreams are running. Their value, in the order they should be taken:

| Workstream | Expected | Overall |
|---|---|---|
| **B memory** (224 -> 42 MB, chunked build) | +40% on B | **+2.4** |
| **A time** (hot loop, 5-6 touches -> ~2) | +30-90% on A | +1.5 to +4.4 |
| **A memory** (merge peak, arena) | +8% on A | +1.5 |
| **D memory** (compact label store) | +21% on D | **+0.15** |

**Every one of these is verified on three corpora and an interleaved A/B before it is
pushed, and rejected if it is correct but slower** — that rule has already discarded
six changes today, including one that was hash-correct and 1.5x slower, and one that
was silently wrong on all five full-corpus runs.

## 4. PHASE 2 — the changes no workstream holds (2-6 hours)

| Change | Mechanism | Value |
|---|---|---|
| **A memory 47 -> 30 MB** | The peak is the **tree merge**, not the scan: two levels of `Group` coexist (~14 MB) and a split group's `Vec` is *extended* with another worker's, so both copies live (~25 MB). A single-level k-way merge — already written once and rejected on **time** (flat, 0.4955 vs 0.4975 s) — removes the intermediate levels, and a preallocated arena of exactly `sum(latencies)` = 12.4 MB removes the duplication. | **+1.5** |
| **C lines 648 -> 400** | 2.11x the leader's 307 lines is **11.8%** of C's gap, the largest source-size term anywhere in the submission. `(648/400)^0.15 = 1.062`. Pure refactoring with the hash as the safety net. | **+1.1** |
| **A lines 412 -> 385** | Small, but free at the margin during any other A edit. | +0.2 |

**Phase 2 total: about +2.5. Running total ~61.**

## 5. PHASE 3 — the time problem, which is the only thing that reaches #1

To match #1 we need, on the three counting tracks:

```
A  0.123 -> 0.018 s   6.8x
C  0.180 -> 0.031 s   5.8x
B  0.551 -> 0.148 s   3.7x
```

**What those numbers mean physically.** The leader's A does 238 MB in 18 ms —
**13 GB/s, about a third of the machine's memory bandwidth**. Across twelve threads
and 3.1 M lines that is roughly **20 cycles per line per thread**. That is a programme
doing almost nothing per byte.

**Our per-line cost is now measured rather than guessed:**

```
next_space x2              SIMD
status + latency parse     arithmetic
13-byte hour compare       guarded to run only when the hour changes
interner.get(ep)           hash + probe        touches 1-2
index.get(&key)            hash + probe        touches 3-4
groups[gi].latencies.push  Vec header + push   touches 5-6
```

**Five to six memory touches against their one.** The append-and-sort restructure
that a workstream is running now removes touches 3 through 6 by keeping the interner
and appending fixed-width records sequentially, sorting per worker at the end. **That
is worth an estimated 1.5-2x on A. It is not worth 6.8x, and nothing on this list
is.** The remaining 3-4x is a different algorithm, and I do not have one.

**This is the honest boundary of the plan.** Phase 1 and Phase 2 together are worth
about **+5**, to roughly **61**. Phase 3 is what stands between 61 and 79, and it is
four to seven times on three independent tracks.

## 6. What the ceiling actually is

The best score **any team** holds per track:

```
A 85.793 (NickTrapp)   B 74.635 (Cause Of Death)   C 79.664 (NickTrapp)
(85.793 x 79.664 x 74.635)^(1/3) = 79.9
```

**#1 is at 79.389 — within 0.6% of that.** The top three teams are inside 1.6 points
of each other. Beating #1 does not mean outworking one person; it means exceeding the
best result **anyone in a field of 76 has produced** on three tracks simultaneously,
while those teams are still pushing.

## 7. THE ASYMMETRIC PLAY — the 35% that is not on the board

**Track total = 0.65 x speed + 0.35 x judge**, and the practice board shows only the
machine axis. Every team is optimising the 65%. Almost none is investing in the 35%,
and it is the one axis where this campaign is genuinely ahead of the field:

- **Code quality (15).** std-only, no dependencies, every track documented with *why*
  rather than *what*, and **zero wrong answers across every push ever made** — with a
  hidden holdout seed deciding the real score, that record is worth more than any
  single track's points.
- **Technique (12).** `docs/VERIFY.md` records **three gates that were green because
  they could not fail**, a bug ledger ordered by what each cost, and a Track D build
  that produced a *deterministic wrong answer* which a timer would never have caught
  and the hash did. `docs/GAP-ANALYSIS.md` carries the full gap arithmetic, including
  the changes rejected and the number that rejected each.
- **PERF.md (8).** Four files, one per track, now carrying the **real board-issued run
  IDs** with the T/M/points each changed. Judges are told to verify that those runs
  exist and moved the numbers the way the file says — ours do, because they were read
  from the board's own JSON rather than transcribed.

**And the strongest technique story this campaign has is a refusal.** Bidirectional
Dijkstra was implemented in full over a reverse CSR, hash-verified on all 2,000
queries, and **rejected at 1.52x slower** — because `ball(2r)/ball(r) ~ 3.2` on this
graph gives only 1.6x fewer relaxations against a `mu` probe and a doubled footprint.
I had costed that branch twice from arithmetic on an unmeasured constant and had it
wrong in both directions. The measurement closed it. **A judge reading that learns
something about how the work was done; a judge reading "we made it fast" learns
nothing.**

## 8. The schedule

| Window | Action | Value |
|---|---|---|
| now + 2h | Bank **B's memory**. Verify and ship Phase 1. | +2.4 |
| +2 to +5h | **A's merge peak**, then **C's line count**. | +2.6 |
| +5 to +12h | One serious attempt at A's hot loop end-to-end, with the hash as the gate. | +1.5 to +4.4 |
| **last 8h before freeze** | **Stop optimising. Fix, document, record the video, verify every citation.** | protects 35% |
| final | Full verification: `check.py` on all four, three corpora, both gates green | — |

**The last row is not a formality.** With 35% of the score in judging and a hidden
holdout seed deciding the machine axis, the difference between a well-documented 61
and a reckless 66 is decided by whether every number in the repository can be traced
to a run that exists.

## 9. The honest answer to "can we beat them"

**On the machine axis, no — not to 79.** That needs 2.85x the product, which is
4-to-7x on three tracks whose leaders are already at the field's ceiling and still
moving. What is reachable is **61**, which is roughly top-25.

**On the combined score, it is not impossible and it is not something I can promise.**
A 35% judge term compresses speed differences: teams that neglected code quality and
PERF.md will lose more there than they gained on the board. We are unusually strong on
exactly that axis, by accident of having spent the session trying not to break things.

**What I will do either way:** take the verified multipliers, refuse the unmeasured
ones, and never push a number I cannot trace to a run. That discipline is the only
reason the correctness record is intact, and with a hidden holdout seed it is worth
more than any single track.
