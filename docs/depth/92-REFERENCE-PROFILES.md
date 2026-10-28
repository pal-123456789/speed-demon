# REFERENCE PROFILES — measured, not hypothesised

> `tooling/profile-reference.py` runs the organizers' reference for each track
> under cProfile and attributes self-time to pipeline phases. The numbers below
> are from the committed fixtures (`verification/data/track-*`, default sizes:
> A 20k lines, B 3k docs/500 queries, C 20k rows, D 20k edges/2k queries) on
> the dev host — **shares, not absolutes**. Scale shifts them (the board's
> dataset is 100× larger on A/B/C and 250× on D), so re-run this on the real
> sample at Oct 30 and again when a rung lands.

## How to run it

```sh
python tooling/profile-reference.py --track a --top 12
python tooling/profile-reference.py --track all
```

## The measured tables

### Track A — reference wall 0.97 s

| Phase | self s | share | the rung it justifies |
|---|---:|---:|---|
| **timestamp** | 0.467 | **48.7%** | A03/A13 — the hour-key insight (no calendar math) |
| unattributed (main loop, builtins) | 0.362 | 37.7% | — |
| aggregate | 0.101 | 10.5% | A11/A14 — bump chunks + parallel shards |
| **line parse** | 0.023 | **2.4%** | A01/A17 — SWAR/AVX2 split (small!) |
| percentile/sort | 0.006 | 0.6% | A07/A13 |
| output | 0.001 | 0.1% | A09/A20 |

**Corrections to our docs:** the hypothesis "date-library timestamp parse is
35–45% of T" is **confirmed** (measured 48.7%). The hypothesis "record split +
field extraction is 15–25%" is **refuted** — it is 2.4%. **Build the hour-key
fast path and the timestamp arithmetic first; do not spend early hours on the
line splitter.** Percentiles are 0.6% of the *reference* (because it sorts per
group at the end after paying the parse); that is not where the ratio is won,
but selection still protects your own M and tail.

### Track B — reference wall 0.73 s

| Phase | self s | share | the rung it justifies |
|---|---:|---:|---|
| unattributed | 0.201 | 28.0% | — |
| **tokenize** | 0.188 | **26.2%** | B01/B21 — class table, then SIMD |
| **index build** | 0.164 | **22.7%** | B11/B13 — single pass, deterministic ids |
| **phrase eval** | 0.120 | **16.6%** | B05/B15 — positions + k-cursor (it re-tokenizes candidates!) |
| boolean eval | 0.043 | 6.0% | B14 — shortest-lead intersection |
| output | 0.004 | 0.6% | B09 |

**Corrections:** the build side (tokenize + index) is **49%** — half the track
is building, not querying. Phrase queries are **16.6%** because the reference
re-tokenizes every candidate document; keeping positions removes almost all of
it. Boolean evaluation is only 6% of the reference — "intersection order" is
still the right micro-optimisation, but it is **not** where the first hours go.

### Track C — reference wall 0.35 s

| Phase | self s | share | the rung it justifies |
|---|---:|---:|---|
| unattributed | 0.135 | 38.2% | — |
| **amount** | 0.085 | **24.1%** | C04/C22 — i64 cents, SWAR digits |
| **aggregate** | 0.070 | **19.7%** | C14/C24 — i128 sums, distinct-name dedupe |
| csv parse | 0.036 | 10.1% | C01/C12 — oracle + parity split |
| output | 0.015 | 4.3% | C23 — table-driven formatting |
| **join** | 0.013 | **3.7%** | C06/C25 — hash join, prefetch |

**Corrections:** the biggest single cost is the **money parse (24%)**, then the
**aggregation (20%)** — not the quoting machinery (10%). And the join is
**3.7%**: "hash join versus sort-merge" is a minor lever on the reference's
time, so do not architect around it. The quoting rules remain the correctness
spine (a wrong parse is a zero), but the *performance* plan should lead with
the amount parser and the aggregation layout.

### Track D — reference wall 16.80 s

| Phase | self s | share | the rung it justifies |
|---|---:|---:|---|
| **dijkstra** | 16.309 | **97.1%** | D11/D12 — radix heap, early exit, warm scratch |
| unattributed | 0.414 | 2.5% | — |
| **edge parse** | 0.063 | **0.4%** | D02/D21 — SWAR u32, chunked pread |
| query i/o | 0.006 | 0.0% | D13 |

**The biggest correction in the pack.** The organizer hint ("a fast submission
spends much of its time parsing the edge list") describes *a fast submission*
— once the search is fast, parsing is what remains. In the **reference**,
97.1% is the per-query Dijkstra: pure-Python `heapq` + dict labels on 2,000
queries. So:

- The **T ratio against the reference is won in the search loop** (heap,
  label storage, early exit, and parallelism across queries). That is also
  where the reference is weakest per operation.
- Parsing is 0.4% of the reference but still matters for your **absolute**
  time once the search is microseconds — keep the SWAR + CSB plan as the
  second rung, not the first.
- This independently supports the model's P5 answer (radix heap + the proven
  meet condition) and downgrades "parser first" as a D strategy.

## What this changes in the plan

| Track | Old first rung | **New first rung (measured)** |
|---|---|---|
| A | parallel chunked parse | **hour-key + timestamp fast path** (48.7%) |
| B | posting compression | **tokenizer + single-pass build** (49%) |
| C | quoting oracle + SIMD locator | **amount parser + aggregation layout** (44%) |
| D | SWAR edge parse | **search loop: heap + labels + early exit** (97%) |

Re-run `profile-reference.py` on the Oct 30 sample and after each big rung:
the table is the empirical answer to "where are the next 10 points?"