# PERF.md — Track C (Join)

> **The optimisation log for Track C.** The organizers weight this at
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

Parse a hostile multi-megabyte CSV — quoted commas, escaped quotes, embedded newlines, mixed line endings, a UTF-8 BOM on some chunks and not others — join it against a lookup table, and emit sorted aggregates.

| Axis | Reference | This submission | Ratio | Weight | Contribution |
|---|---:|---:|---:|---:|---:|
| T · wall time | 36.90 s | _(fill)_ | _(fill)_ | 0.40 | _(fill)_ |
| M · peak memory | (pin from the Oct 28 spec) | _(fill)_ | _(fill)_ | 0.20 | _(fill)_ |
| S · source size | (pin from the Oct 28 spec) | _(fill)_ | _(fill)_ | 0.15 | _(fill)_ |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| X · scaling | 10.40x | _(fill)_ | _(fill)_ | 0.10 | _(fill)_ |
| **K** | | | | | **_(fill)_** |
| **speed score** | | | | | **_(fill)_ / 100** |

`K = (Tref/T)^0.40 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15 x (Xref/X)^0.10`,
each ratio capped at 50x, `speed score = 100*ln(K)/ln(25)`.
Recompute with `python tooling/k_calc.py --t _ --m _ --s _ --d 1 --x _`.

M is easy to win here by streaming: the reference materialises rows the aggregate does not need.

> These numbers are **not filled in yet**, on purpose. They are only written
> down once the harness has produced them. A target is not a result.

---

## 2. The ladder

Every row below is a step that was actually taken, with the measurement that
justified keeping it. Rows marked `EXPECTED` have not been measured yet.

| # | Change | T (s) | M (MB) | vs. previous | Verdict |
|---|---|---:|---:|---:|---|
| 1 | Correct scalar 4-state CSV machine, single threaded | _ | _ | _SHA-green — this is the oracle and is protected_ | EXPECTED |
| 2 | Fixed-point cents instead of float; dense region ids | _ | _ | _bit-exact sums_ | EXPECTED |
| 3 | Serial structural pre-scan for safe row boundaries | _ | _ | _makes parallel parsing legal_ | EXPECTED |
| 4 | SIMD candidate-byte locator feeding the scalar machine | _ | _ | _finds candidates, never decides meaning_ | EXPECTED |
| 5 | Thread-local aggregates + deterministic merge | _ | _ | _12 threads on 6 P-cores_ | EXPECTED |
| 6 | Hash join vs sort-merge, measured | _ | _ | _the join strategy is a decision, not an assumption_ | EXPECTED |

---

## 3. Experiments that failed, and were reverted

This section is deliberately the longest part of the file. Each of these was
implemented, measured, and removed.

| Experiment | Hypothesis | Result | Why it was reverted |
|---|---|---|---|
| Nearest-newline chunking for parallel parsing | _ | _ | splits quoted fields — wrong answers, and the kind of wrong that passes a casual test |
| Global BOM stripping | _ | _ | corrupts a BOM inside a quoted field |
| A fully SIMD CSV parser | _ | _ | could not get the embedded-newline cases right; reverted to the scalar oracle |
| `f32`/`f64` amount accumulation | _ | _ | non-associative under a parallel reduction — nondeterministic sums |
| A perfect hash built from the sample's region ids | _ | _ | overfitting to a holdout we never see |

---

## 4. Where the time actually goes

_(fill after a profile — the flame graph, the `perf stat` counters, and the
one measurement that surprised you)_

---

## 5. Determinism

* Row boundaries come from the structural pre-scan, never from a naive newline
  search.
* Sums are integer cents, aggregated in a fixed order — no float reduction,
  so no reassociation is possible.
* Output rows are emitted in the frozen sort order with the frozen
  comparator, and never in hash-map iteration order.
* The differential tester compares this implementation against the published
  reference byte for byte, and reports the first differing byte with its
  offset and context.

---

## 6. Memory

The parser is a streaming state machine over a buffered reader, so peak
memory is the size of the aggregate plus the lookup table — not the size of
the file. The one exception is the structural pre-scan, which needs to know
where the row boundaries are; that is stored as a compact array of offsets,
not as a copy of the data.

`mmap` is avoided for the same reason as the other tracks: mapped pages are
charged to cgroup `memory.peak`.

---

## 7. What I would do with another 24 hours

* A second, independent parser — a deliberately naive one — kept as a
  cross-check that runs on a small sample of every batch. Two independent
  implementations agreeing is much stronger evidence than one implementation
  being careful.
* SIMD the quote-run rule itself (count consecutive quotes) rather than just
  locating the candidates.
* Replace the structural pre-scan with a single-pass resumable state machine
  that can be checkpointed at a chunk boundary, removing the need for a
  separate pass.
* Measure hash join against sort-merge on the real lookup-table cardinality
  rather than assuming, and document the result either way.

---

## 8. Reproducing these numbers

```bash
# 5 runs, discard the highest and lowest, mean of the middle three
pwsh -File verification/bench.ps1 -Track C -Runs 5

# the four gates: correctness, speed floor, timeout, sandbox
pwsh -File verification/verify.ps1 -Track C

# the full lockdown checklist
pwsh -File verification/pre-freeze.ps1 -Track C
```

Commit hash of this measurement: `_(fill)`.
