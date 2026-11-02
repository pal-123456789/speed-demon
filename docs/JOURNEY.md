# The Journey — how this submission was built, and what it cost

> **28.468 → 78.121 overall.** Every number here is a live board measurement.
>
> This document exists because the interesting part of a performance project is not the final
> code — it is the reasoning that produced it, and especially **the ideas that were measured and
> thrown away.** A project that only lists its wins is telling you half the story, and the less
> useful half.

---

## The shape of the work

The score is a geometric mean over tracks, and each track's points come from four independent axes:

```
K = (T_ref / T)^0.50 · (M_ref / M)^0.20 · (S_ref / S)^0.15 · (D_ref / D)^0.15
points = 50 · log10(K)        clamped to 0…100
overall = geometric mean of the best three tracks
```

Two consequences shaped everything that follows.

**First, the axes are not equally expensive.** To gain one point on any track you need:

| Axis | What one point costs | Which means |
|---|---|---|
| **Time** | **8.8% faster** | the ordinary target |
| **Memory** | **21% less** | expensive |
| **Source** | **26% fewer lines** | the most expensive, and the most reliable |

**Source is the most expensive axis and the easiest to improve**, because it rewards *deletion*.
That single fact explains most of what happened in this project.

**Second, only three of four tracks count.** The best three are taken, so a fourth weak track costs
nothing — but a *regression* on one of the three costs a third of the overall. That asymmetry is why
every risky change here was pushed behind the board's "keep the best" behaviour and reverted the
moment it lost.

---

## Where it started

The four tracks began as correctness-first implementations. They were **exact** — the output hash
matched on every corpus — and they were **slow**:

```
overall 28.468
```

Exactness came first and never left. Every measurement in this document was taken with a harness that
compares the **sha256 of the output** against a pinned expected value, so no change was ever accepted
on a feeling.

---

## The wins

### The pattern, stated once

**Nearly every large gain was a deletion.** That is the single most useful thing this project
learned, and it is why the source axis is discussed first in every ledger here.

| Track | Change | Result |
|---|---|---|
| **A** | cut the parser and formatter from 704 to 440 lines | the first big jump |
| **A** | `drop(parts)` *before* the output map rather than after | **+1.20 points** — memory 31.4 → 23.94 MB |
| **A** | rewrote `decode`/`select` | 74.28 → **78.16** |
| **B** | bucketed `u16` build, then the class-renumber rewrite | 61.65 → **72.62**, memory 219 → 49.7 MB |
| **C** | two full rewrites | 61.44 → 66.62 → **66.81** |
| **C** | the `Phase` claim primitive, a 1 MB name table, deletion of the pre-pass | 66.81 → **74.02**, lines 1105 → 652 |
| **C** | the phase laps that located the real cost | 74.02 → **75.18** |
| **D** | restoring `prctl(PR_SET_THP_DISABLE)` | **+25 MB** recovered |

**Every one of those is a removal or a simplification.** Not one is an added mechanism.

### The two rewrites that moved the board furthest

Late in the project two of the four tracks were rewritten end to end. **Both got faster and
smaller at the same time**, which is the ideal outcome on a two-axis score:

```
B   72.829 → 75.124   (+2.295)   time −17%    683 → 644 lines
C   75.345 → 79.292   (+3.947)   time −32%    610 → 582 lines
```

**Track C's rewrite is the clearest example of the pattern.** Instead of walking each record field by
field, the input is now *classified*: each 64-byte block becomes quote, comma and LF masks at once,
and the "inside quotes" state falls out of a **single carry-less multiply (`PCLMULQDQ`) computing the
prefix XOR of the quote mask**, carried block to block. That one idea replaces the entire quoted-field
scanner — and it *deletes* the code that used to do it.

**Track B's rewrite is the same story on a different axis.** The term table was keyed by a 32-byte
lowercased vector with a byte comparison per probe. It is now keyed by a **64-bit hash built with
`bzhi`-masked 8-byte loads**, the `keys` array is gone, and the byte compare is gone. **Smaller and
faster, simultaneously.**

---

## The reversals — and why they are in this document

**These are experiments that were built, verified exact, measured, and then deleted.** They are
recorded because a submission that claims a win rate of 100% is not reporting honestly, and because
each one closed a hypothesis permanently.

| Experiment | Expected | Measured | Verdict |
|---|---|---|---|
| **`-C target-cpu=native`** | a systemic time win | **+2.1% / +2.8% / +0.6% / +6.7% slower** on A/B/C/D | reverted |
| **`lto = fat` + `embed-bitcode`** | cross-unit inlining | **+12.5% slower** | rejected |
| **Grouped four-slot term probe** | a large scan win | **scan +20%, total +14%** | reverted |
| **16-bit saturating distant labels** | a much smaller label store | exact, but **time +13.9%, memory +3.9%, +122 lines** | reverted |
| **`MADV_POPULATE_WRITE` on A's output** | fewer faults while formatting | **0.039 → 0.069 s** | reverted |
| **`MADV_SEQUENTIAL` on A's log** | better readahead | **parse 0.0289 vs 0.0233 s** | reverted |
| **Dropping `docs.txt`'s pages after the scan** | a time win | inside the noise floor; **16 certain lines** | reverted |
| **Degree-2 contraction on D** | remove chains | **the corpus has no chains of length ≥ 2** | impossible, not slow |
| **Atomic-free transpose on D** | remove atomics | **+47%** | reverted |
| **A paged flat distance array on D** | less memory | **memory ×2.7** | reverted |
| **Structural fast-path deletion in C** | fewer branches | **time +49%** | reverted |
| **A one-word heap entry on D** | smaller heap | 67.880 vs 68.590 — **the +99 source lines cost more than the memory saved** | reverted |

### The most instructive one: `native`

`-C target-cpu=native` was tried, measured **worse on all four tracks**, and reverted — but the
*reason* was not understood at the time. It was only explained later, from first principles about how
the board builds: **the machine that compiles is not the machine that runs.** A `native` build
resolves against the build host, which has AVX-512; the scoring CPU is a Core i5-13500 whose AVX-512
is fused off. The build was emitting instructions the scoring machine did not want.

The fix is not a flag to be found by trial — it is to **name the target explicitly**. The build now
carries `-C target-cpu=alderlake`, which is correct by construction rather than by luck.

**That is the lesson: a measurement tells you *that* something is slower, never *why*. An unexplained
measurement is a hypothesis, not a conclusion.**

### The one that was measured wrong

One experiment was rejected on **invalid evidence** and later had to be revisited. A scratch variant
of Track C was built to price a structural fast path by *forcing the fast path to decline* — but the
build that did so also removed the surrounding scan, the target-feature attribute and the inline
annotations. **The measurement priced a difference that was not the difference being tested.**

The rule that came out of it is now applied everywhere: **a scratch variant only prices the change it
actually makes, and nothing else.**

---

## The measurement discipline

Everything in this repository is downstream of one workflow.

**1 — Locate the cost before touching it.** Track A, B and D each carry an `SD_TIME` phase-lap
facility. For B and D it immediately named the bottleneck (**B: the scan is 70% of the run; D:
construction is 66%, and `renumber` alone is 24%**). For A it produced the first attribution that
track ever had:

```
parse      50.8%     ← the parser is over half the track
hours      24.4%
text        22.0%
rank         2.8%
size+map     0.6%
```

**That measurement changed what "optimising Track A" meant.** Every prior idea for A had been about
memory. The time was in the parser.

**2 — Verify exactness on more than one corpus.** Full corpus, a tiny hostile oracle, and the
official sample. A change that passes one and fails another is a change with a bug.

**3 — Measure interleaved.** On a shared machine, run-to-run spread reaches 16% on time. Every A/B
comparison is therefore **alternated** between variants rather than run in blocks, and the
**smallest** of several runs is trusted rather than the mean.

**4 — Delete the build products before rebuilding.** A stale binary silently reports the previous
version's result. This bit the project once and is now a standing rule.

**5 — Never chain a write, a commit and a push.** A gate that fails must not be followed by a push.
The publish step is always one whose success *depends* on the gate passing.

**6 — Read the result back.** After editing documentation or a generated file, the file is read again
to confirm what was actually written.

**7 — A check with an invented expected value is not a check.** One early verification compared
against a hash that had been written by hand rather than produced by the reference. It was reported
as wrong and removed; every expected hash here now comes from a reference run.

---

## Where it finished

```
                    time         memory      source        points
A  Ingest         26.73 ms      18.41 MB     625 lines      80.039
B  Index         140.30 ms      46.93 MB     708 lines      75.124
C  Join           23.64 ms       9.61 MB     721 lines      79.292
D  Traverse      169.86 ms      70.57 MB     536 lines      68.589

overall  78.121        session start 28.468
```

**Zero wrong answers were published at any point**, including for every experiment listed as a
reversal above — because a losing experiment was always **reverted**, never shipped. The board's
"best result per track" rule made that safe to do: a losing run cannot take a banked score away.

**The remaining known inefficiencies are documented rather than hidden** — see the "Honest limits"
section of the [README](../README.md) and the per-track `PERF.md` ledgers beside each source file.
