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
| T · wall time | 14.44 s | 0.182 s | 79.5x | 0.50 | 8.918 (derived) |
| M · peak memory | 367 MB | 16 MB | 22.4x | 0.20 | 1.862 (derived) |
| S · source size | 108 lines | 648 lines | 0.167x | 0.15 | 0.764 (derived) |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| **K** | | | | | **12.5759** |
| **speed score** | | | | | **54.98 / 100** |

`K = (Tref/T)^0.50 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15`.
Memory is capped at 50x; wall time is not; S and D are capped at 1.0, so they
can cost you and never pay. `speed score = 50*log10(K)`, clamped to 0..100, so
K = 10 is half marks and K = 100 is full marks.

M is easy to win here by streaming: the reference materialises rows the aggregate does not need.

> Measured on the live board; best run `run-166bebffcc78` at commit `900691d`:
> **T 0.1595 s, M 14.0 MB, S 707 lines, K 13.665, 56.78 points.**
> T is the P-cores timed run and M is cgroup v2 `memory.peak`, both read from the
> board rather than measured locally. A target is not a result, so the local
> harness numbers below stay separate from these.

---

## 1b. Board runs — the official optimisation log

The starter's `PERF.md` defines this table, and the judges check that every
run ID exists and that each change moved the numbers the way this file says it
did. One row per live-board push, in step order. The board caps a run at 3x
the reference (final scoring 10x).

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | — | 14.44 s | (pin from the Oct 28 spec) | 0 |
| 1 | Real Join: spec 2.1 parser taken literally, twelve threads split at proven quote-parity record boundaries, exact integer cents | `run-e2f7ca94f947` | 0.9079 s | 367.6 MB | 25.02 |

---

## 2. The ladder

### 2a. What was measured

<!-- BEGIN GENERATED measured (tooling/gen_docs.py)
     Everything between this marker and the matching END marker is REPLACED on
     every run of `python tooling/gen_docs.py`. Change the ledger, or the
     TRACKS config at the top of that file -- never this text, because your edit
     will be overwritten. Everything OUTSIDE the two markers, including the
     board-run table above, is hand-written and is preserved. -->
> **No measurements yet.** This table is filled automatically from
> `verification/ledger.jsonl` the moment the harness records a run.
> A hand-written number in this section would be a guess.

_(no measurements recorded yet)_
<!-- END GENERATED measured -->

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
| 1 | Correct scalar 4-state CSV machine, single threaded | _ | _ | _SHA-green — this is the oracle and is protected_ | PLANNED |
| 2 | Fixed-point cents instead of float; dense region ids | _ | _ | _bit-exact sums_ | PLANNED |
| 3 | Serial structural pre-scan for safe row boundaries | _ | _ | _makes parallel parsing legal_ | PLANNED |
| 4 | SIMD candidate-byte locator feeding the scalar machine | _ | _ | _finds candidates, never decides meaning_ | PLANNED |
| 5 | Thread-local aggregates + deterministic merge | _ | _ | _12 threads on 6 P-cores_ | PLANNED |
| 6 | Hash join vs sort-merge, measured | _ | _ | _the join strategy is a decision, not an assumption_ | PLANNED |

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
# Windows PowerShell 5.1 (present on every Windows box; `pwsh` is not installed
# on the dev host, so advertising it here produced commands that could not run).
# A PowerShell 7 shell works too — drop the -NoProfile/-ExecutionPolicy flags.

# 5 runs, discard the highest and lowest, mean of the middle three
powershell -NoProfile -ExecutionPolicy Bypass -File verification\bench.ps1 -Track C -Runs 5

# the four gates: correctness, speed floor, timeout, sandbox
powershell -NoProfile -ExecutionPolicy Bypass -File verification\verify.ps1 -Track C

# the full lockdown checklist
powershell -NoProfile -ExecutionPolicy Bypass -File verification\pre-freeze.ps1 -Track C

# or, with a Rust toolchain and no PowerShell, the harness does the same work
cargo run --release --offline --manifest-path verification/harness/Cargo.toml -- bench --track C --no-docker
```

Commit hash of this measurement: `_(fill)`.
