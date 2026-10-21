# PERF.md — Track A (Ingest)

> **The optimisation log for Track A.** The organizers weight this at
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

Aggregate 3.1M structured log lines (312 MB) into per-endpoint request counts, p50/p95/p99 latency, and error rates bucketed by hour.

| Axis | Reference | This submission | Ratio | Weight | Contribution |
|---|---:|---:|---:|---:|---:|
| T · wall time | 41.20 s | _(fill)_ | _(fill)_ | 0.40 | _(fill)_ |
| M · peak memory | 2,940 MB | _(fill)_ | _(fill)_ | 0.20 | _(fill)_ |
| S · source size | 3,180 B | _(fill)_ | _(fill)_ | 0.15 | _(fill)_ |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| X · scaling | 10.40x | _(fill)_ | _(fill)_ | 0.10 | _(fill)_ |
| **K** | | | | | **_(fill)_** |
| **speed score** | | | | | **_(fill)_ / 100** |

`K = (Tref/T)^0.40 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15 x (Xref/X)^0.10`,
each ratio capped at 50x, `speed score = 100*ln(K)/ln(25)`.
Recompute with `python tooling/k_calc.py --t _ --m _ --s _ --d 1 --x _`.

M is peak RSS (cgroup `memory.peak`). The reference's 2,940 MB comes mostly from retaining every latency in a growing per-endpoint list.

> These numbers are **not filled in yet**, on purpose. They are only written
> down once the harness has produced them. A target is not a result.

---

## 2. The ladder

### 2a. What was measured

> **Generated from `verification/ledger.jsonl`** — every row below is a
> measurement a machine produced, trimmed the same way the organizers
> trim theirs (high and low discarded, mean of the middle three).
> Commits represented: 241e64c.
> **Do not edit this table by hand.** It is regenerated.

| # | Change | T (s) | M (MB) | vs. previous | Verdict |
|---|---|---:|---:|---:|---|
| 1 | baseline | 0.030 | _ | baseline | MEASURED (`241e64c`) |

### 2b. What was planned

Every row below is a step in the optimisation sequence. The `vs. previous`
column states the *expected* effect of the step.

> **The T and M cells stay empty until the harness has produced them.** A row
> with empty numbers is a plan, not a result. When a step is attempted, it
> moves up into table 2a with a real number attached — and if it fails, it
> moves down into section 3 with the measurement that killed it.
>
> **Before the freeze, every planned row must be either measured or struck
> out.** A plan that was never attempted is not a result and must not be
> presented as one.

| # | Change | T (s) | M (MB) | vs. previous | Verdict |
|---|---|---:|---:|---:|---|
| 1 | Correct baseline: `split_whitespace` + `HashMap` + sort-per-endpoint | _ | _ | _the naive shape, SHA-green_ | PLANNED |
| 2 | Dense endpoint ids; per-endpoint `Vec<u32>` replaced by a flat histogram | _ | _ | _removes the sort and the retained list_ | PLANNED |
| 3 | Custom byte scanner over the fixed line grammar | _ | _ | _removes the general-purpose splitter_ | PLANNED |
| 4 | Timestamp by arithmetic instead of a date library | _ | _ | _removes the second-biggest cost_ | PLANNED |
| 5 | Two-level radix histogram for exact percentiles | _ | _ | _percentiles without sorting_ | PLANNED |
| 6 | Thread-local aggregation + deterministic merge | _ | _ | _12 threads on 6 P-cores_ | PLANNED |
| 7 | Bump arena for interned endpoints; buffer reuse | _ | _ | _M axis_ | PLANNED |
| 8 | PGO / branchless inner loop / cache blocking | _ | _ | _the last 10%_ | PLANNED |

---

## 3. Experiments that failed, and were reverted

This section is deliberately the longest part of the file. The organizers say
so explicitly: *"The failed experiments count — they are often the most
interesting part and judges are told to reward them."*

> **The table below is a plan, not a claim.** Each row is an experiment worth
> running, with the reason it is expected to fail. As the campaign runs, each
> row is either filled in with the measurement that killed it, or **deleted**
> if it was never attempted. A row left in the table with an empty Result
> column is a promise the judges will check.
>
> **Do not leave an unmeasured row in this file at the freeze.** Either the
> experiment happened and the result is written down, or it did not happen and
> the row is gone. An empty cell is not a measurement.

| Experiment | Hypothesis | Result | Why it was reverted |
|---|---|---|---|
| SIMD timestamp parse | _ | _ | branchy setup cost per line outweighed the vector win |
| Lock-free shared intern table | _ | _ | contention beat the sharded version |
| 12 workers instead of 6 | _ | _ | SMT + cache pressure widened the spread |
| `mmap` instead of a buffered read | _ | _ | inflated `memory.peak` — M axis got worse |
| t-digest / KLL sketches | _ | _ | approximate percentiles cannot hash-match |

---

## 4. Where the time actually goes

_(fill after a profile — the flame graph, the `perf stat` counters, and the
one measurement that surprised you)_

---

## 5. Determinism

* Endpoint ids are assigned by a deterministic sorted rule, never by hash-map
  iteration order.
* The group key is `epid * hour_count + hour`, so the output order is fully
  determined by the ids.
* Percentiles are computed from integer counts — no float ever enters the
  ranking, so no rounding mode can change the answer.
* Error rates are integer counts formatted by an explicit routine.
* The parallel merge runs in worker-index order, so 1 worker and 12 workers
  produce byte-identical output.

---

## 6. Memory

The M axis is won by *not retaining* anything. The reference holds every
latency in a growing list (that is where its 2,940 MB goes); this
implementation holds a fixed-size histogram per group, so peak memory is a
function of the number of distinct groups rather than the number of records.

Input is read with a buffered streaming reader rather than `mmap`: pages of a
memory-mapped file are charged to cgroup `memory.peak`, which would inflate
the M axis with data we never need resident.

---

## 7. What I would do with another 24 hours

* Replace the scalar scanner with an AVX2 `vpcmpeqb`/`vpmovmskb` candidate
  locator feeding a branchless field-state machine, with a scalar fallback
  selected by a single runtime CPU check.
* Replace the two-level histogram with a three-level one so the inner scan
  touches fewer cache lines for skewed latency distributions.
* Cache-block the merge: the current merge walks the group array twice, and
  for 1,842 endpoints x 24 hours that array should sit entirely in L2.
* Add a `perf`-driven pass over the merge, which has never been the
  bottleneck and is therefore the one place a surprise is still possible.

---

## 8. Reproducing these numbers

```bash
# 5 runs, discard the highest and lowest, mean of the middle three
pwsh -File verification/bench.ps1 -Track A -Runs 5

# the four gates: correctness, speed floor, timeout, sandbox
pwsh -File verification/verify.ps1 -Track A

# the full lockdown checklist
pwsh -File verification/pre-freeze.ps1 -Track A
```

Commit hash of this measurement: `_(fill)`.
