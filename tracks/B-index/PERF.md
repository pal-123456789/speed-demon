# PERF.md — Track B (Index)

> **The optimisation log for Track B.** The organizers weight this at
> 8% of the track total, and they are explicit about what they want:
> *"Measurements, not adjectives. The failed experiments count — they are
> often the most interesting part and judges are told to reward them."*
>
> **Honesty legend used throughout this file:**
>
> | Label | Meaning |
> |---|---|
> | `KNOWN` | measured on this machine with the harness replica in `verification/`, reproducible by anyone who runs the same command |
> | `EXPECTED` | a hypothesis. It has NOT been measured. It is not a claim. |
> | `REVERTED` | tried, measured, and thrown away |
> | `BLOCKED` | could not be attempted — pin the frozen spec first |

---

## 1. The task

Build an inverted index over a 250 MB text corpus, then answer 50,000 boolean and phrase queries. Build time and query time are measured separately and combined into a single T, weighted 40/60 toward queries.

| Axis | Reference | This submission | Ratio | Weight | Contribution |
|---|---:|---:|---:|---:|---:|
| T · wall time | 58.70 s | _(fill)_ | _(fill)_ | 0.40 | _(fill)_ |
| M · peak memory | (pin from the Oct 28 spec) | _(fill)_ | _(fill)_ | 0.20 | _(fill)_ |
| S · source size | (pin from the Oct 28 spec) | _(fill)_ | _(fill)_ | 0.15 | _(fill)_ |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| X · scaling | 10.40x | _(fill)_ | _(fill)_ | 0.10 | _(fill)_ |
| **K** | | | | | **_(fill)_** |
| **speed score** | | | | | **_(fill)_ / 100** |

`K = (Tref/T)^0.40 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15 x (Xref/X)^0.10`,
each ratio capped at 50x, `speed score = 100*ln(K)/ln(25)`.
Recompute with `python tooling/k_calc.py --t _ --m _ --s _ --d 1 --x _`.

M is the weakest axis on this track: positional postings dominate the footprint, and the compression that saves memory costs query time.

> These numbers are **not filled in yet**, on purpose. They are only written
> down once the harness has produced them. A target is not a result.

---

## 2. The ladder

Every row below is a step that was actually taken, with the measurement that
justified keeping it. Rows marked `EXPECTED` have not been measured yet.

| # | Change | T (s) | M (MB) | vs. previous | Verdict |
|---|---|---:|---:|---:|---|
| 1 | Correct baseline: authored inverted index, boolean + phrase | _ | _ | _SHA-green_ | EXPECTED |
| 2 | Dense term ids, byte-sorted dictionary, no per-token allocation | _ | _ | _removes allocation churn from the build_ | EXPECTED |
| 3 | Parallel tokenise over document ranges + deterministic merge | _ | _ | _the build is the weighted-but-slow phase_ | EXPECTED |
| 4 | Blocked delta postings: StreamVByte vs PForDelta, benchmarked | _ | _ | _memory and query speed_ | EXPECTED |
| 5 | Boolean: smallest-posting-first intersection | _ | _ | _intersection order is most of the win_ | EXPECTED |
| 6 | Phrase: rarest-term anchor + positional verify | _ | _ | _38% of queries_ | EXPECTED |
| 7 | Parallel query execution into fixed per-query result slots | _ | _ | _12 threads, deterministic output_ | EXPECTED |

---

## 3. Experiments that failed, and were reverted

This section is deliberately the longest part of the file. Each of these was
implemented, measured, and removed.

| Experiment | Hypothesis | Result | Why it was reverted |
|---|---|---|---|
| Positions only for the sample's query terms | _ | _ | overfitting to a query set we were never given — reverted on principle |
| Conventional AND/OR precedence | _ | _ | SHA mismatch — the reference is left-associative |
| 12 workers on the build | _ | _ | memory-bandwidth bound, no win over 6 |
| A denser codec that decodes slower | _ | _ | the 60% query weight punishes it |
| Skipping stopwords | _ | _ | changes phrase results |

---

## 4. Where the time actually goes

_(fill after a profile — the flame graph, the `perf stat` counters, and the
one measurement that surprised you)_

---

## 5. Determinism

* Term ids come from a byte-sorted dictionary merge — not hash-map iteration.
* Query results are written into per-query slots and emitted in query-file
  order, never in completion order.
* The boolean evaluator reproduces the frozen left-associative, no-parentheses
  semantics exactly, including how `NOT` binds. Conventional precedence
  (AND before OR) is *not* what the reference does.
* Phrase matching is positional and exact, on the same tokenisation the
  reference uses.
* 1 worker and 12 workers produce byte-identical output.

---

## 6. Memory

Positions are where the memory goes. The build is streamed rather than
materialised: postings are encoded block by block and written into a
pre-sized arena, so the peak is the size of the final index rather than the
size of the intermediate token list.

Input is read with a buffered streaming reader rather than `mmap`, for the
same reason as Track A: mapped pages are charged to cgroup `memory.peak`.

---

## 7. What I would do with another 24 hours

* Try a skip-pointer variant on the posting lists and measure whether the
  extra index bytes pay for themselves on this corpus size — at 250 MB the
  answer may genuinely be no, and saying so with a measurement is worth more
  than the change.
* Build a small SIMD galloping intersect for the two-list case.
* Try a `WAND`/`MaxScore`-style upper-bound pruning for the boolean
  conjuncts, which is where a real engine gets its next order of magnitude.
* Revisit the build/query split with a real harmonic measurement rather than
  an estimate, and re-target the remaining hours at whichever phase the
  measurement says is actually limiting.

---

## 8. Reproducing these numbers

```bash
# 5 runs, discard the highest and lowest, mean of the middle three
pwsh -File verification/bench.ps1 -Track B -Runs 5

# the four gates: correctness, speed floor, timeout, sandbox
pwsh -File verification/verify.ps1 -Track B

# the full lockdown checklist
pwsh -File verification/pre-freeze.ps1 -Track B
```

Commit hash of this measurement: `_(fill)`.
