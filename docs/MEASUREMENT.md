# Measurement — how every number in this repository was obtained

> A performance claim is only as good as the method behind it. This document states the method, so
> that any number here can be judged on how it was produced rather than on how it is presented.

---

## The scoring model

```
K = (T_ref / T)^0.50 · (M_ref / M)^0.20 · (S_ref / S)^0.15 · (D_ref / D)^0.15
points = 50 · log10(K),  clamped to 0…100
overall = geometric mean of the best three tracks
```

**What one point costs on any track:**

| Axis | Cost per point |
|---|---|
| **Time `T`** | 8.8% faster |
| **Memory `M`** | 21% less |
| **Source `S`** | 26% fewer lines |

The axes are independent, so a change is only worth making if it improves the product. **That has a
direct practical consequence, and it is the single most important thing in this document: a change
that improves one axis while worsening another by more must be rejected, however good the first axis
looks.**

That is exactly what happened to the narrow distance labels on Track D — exact, faster on time, and
rejected because the source cost was larger. See [JOURNEY.md](JOURNEY.md).

**Source `S` is measured over the whole track folder** after the harness's canonical formatter, which
means build scripts and container definitions count as code. **Deletions are therefore scored**, and
this is why several of the largest wins in this project were removals of code that had been added
earlier in the same project.

**Memory `M` is a cgroup high-water mark**, and — an important detail — **file-backed mapped pages
are not charged, only anonymous ones.** A program that memory-maps a 239 MB input does not pay for it
in memory; a program that reads it into a heap buffer does. Every track here maps rather than reads.

---

## The verification harness

No measurement is taken without first proving the output is still correct. The gate is the same for
every track:

```
1. FULL CORPUS          5 runs, each output hashed with sha256
                        → every run must equal the pinned expected hash
2. TINY HOSTILE ORACLE  a hand-built corpus of boundary cases
                        → must equal its pinned hash
3. OFFICIAL SAMPLE      the organizers' published sample
                        → must equal its pinned hash
```

A change that passes one and fails another is a change with a bug, and the corpus that failed tells
you which. **A change that fails any of the three is reverted, not debugged forward.**

**The full-corpus gate is five runs, not one.** The corpora are large enough that a
schedule-dependent defect can pass a single run; five runs of a deterministic program that hash
identically is meaningful evidence, one run is not.

### Boundary cases the tiny oracle covers

zero weights · parallel and duplicate edges · self-loops · isolated vertices · unreachable targets ·
64-bit distance sums · `""` escapes · bare newlines inside quoted notes · mixed CRLF and LF line
endings · a byte order mark before any record · empty inputs · a target with no in-edge · `s == t`

---

## Taking a timing measurement

**A measurement that is not repeatable is not a measurement.** On the shared machines used here:

**1 — Interleave the variants.** Run-to-run spread on time reaches **16%** on identical code. Every
A/B comparison therefore alternates between the two binaries rather than running one block and then
the other, so a slow period on the machine hits both variants equally.

**2 — Trust the smallest, not the mean.** A loaded machine can only ever make a run *slower*. The
minimum over several interleaved runs is therefore the closest estimate of the true cost.

**3 — Measure on the right filesystem.** The corpus is staged on a native Linux filesystem for local
timing; a Windows-mounted path costs roughly eight times more in I/O and would dominate everything.

**4 — Measure the phase, not just the total.** A total tells you a change helped; it never tells you
why. Every track that needed optimisation carries an `SD_TIME` phase-lap facility, and using it is
what produced the single most valuable measurement in this project:

```
Track A:  parse 50.8% · hours 24.4% · text 22.0% · rank 2.8% · size+map 0.6%
```

Before that number existed, every idea for Track A had been about memory. The time was in the parser.

**5 — Delete the build product before rebuilding.** A stale binary silently reports the previous
version's result. This is a standing rule, not a precaution.

**6 — Price only the change you actually made.** A scratch variant built to isolate one optimisation
must differ *only* in that optimisation. An early experiment violated this — the variant that forced
one path also removed the surrounding scan, the target-feature attribute and the inlining — and its
result was meaningless.

---

## The board as an instrument

The live board builds and runs on the scoring hardware, which makes it the only timing rig that
measures the real thing. Two properties make it usable as an experimental instrument:

- **It keeps the best result per track.** A run that is worse than the banked best cannot remove the
  banked points, so an experiment can be pushed and measured at no risk to the score.
- **Its time column is measured on the scoring CPU**, by an external timer, on the real dataset.

**One caveat governs the interpretation of any single board run:** the board's own run-to-run noise
is roughly **±1.5 points**, and time moves by up to 16% between runs of identical code. **Where a
gain fell inside that spread, this repository says so rather than claiming it.** Several experiments
were recorded as "inconclusive" for exactly this reason.

**And one distinction that matters for a final evaluation.** The board protects a banked score with
its best-per-track rule — but **final scoring measures the frozen commit**. Banked points do not
transfer to it. **Every change that lost was therefore reverted from the source, not merely superseded
on the board.**

---

## What was deliberately *not* claimed

Honesty is part of the method, so these are stated plainly:

| Claim | Why it is not made |
|---|---|
| narrow distance labels on D are a win | measured exact, measured faster on time, **and rejected** because the source cost exceeded the gain |
| the page-drop on B is a win | the measurement sat **inside the run-to-run spread**, and it cost 16 certain source lines — so it was reverted |
| `MADV_SEQUENTIAL` / `MADV_POPULATE_WRITE` help Track A | both were measured and **both were slower** |
| LTO helps | `lto = fat` measured **+12.5% slower** |
| `native` helps | measured **0.6%–6.7% slower on all four tracks** |
| anything about memory from the local machine | local RSS is not the board's measurement; only anonymous pages are charged, and a small-input proxy is not a substitute |

---

## Reproducing the numbers

```sh
# build a track exactly as the scored path does
sh track-a/build.sh

# correctness on the tiny hostile oracle and the official sample
tooling/check-corpora.sh a summary.txt "$(cat track-a/verification/expected/summary.txt.sha256)"

# correctness on the full corpus, five hash-checked runs
tooling/measure-full.sh a /path/to/track-a <expected-hash> summary.txt
```

Each track's own `PERF.md`, beside its source, holds that track's measurement ledger — what was
tried, what it measured, and what stayed.

---

<p align="center">
  <sub>
  <b>Speed Demon 2026</b> · four Rust programs · no dependencies · every answer exact<br/>
  <a href="../README.md">README</a> ·
  <a href="JOURNEY.md">Journey</a> ·
  <a href="TECHNIQUES.md">Techniques</a> ·
  <a href="MEASUREMENT.md">Measurement</a> ·
  <a href="../plan/README.md">Plan</a>
  </sub>
</p>
