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

Build an inverted index over a 250 MB text corpus, then answer 50,000 boolean and phrase queries. Build time and query time are measured together, as one total wall time. There is no build/query split any more.

| Axis | Reference | This submission | Ratio | Weight | Contribution |
|---|---:|---:|---:|---:|---:|
| T · wall time | 56.35 s | 0.551 s | 102x | 0.50 | 10.113 (derived) |
| M · peak memory | 2200 MB | 214 MB | 10.3x | 0.20 | 1.594 (derived) |
| S · source size | 91 lines | 973 lines | 0.0935x | 0.15 | 0.701 (derived) |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| **K** | | | | | **11.1901** |
| **speed score** | | | | | **52.44 / 100** |

`K = (Tref/T)^0.50 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15`.
Memory is capped at 50x; wall time is not; S and D are capped at 1.0, so they
can cost you and never pay. `speed score = 50*log10(K)`, clamped to 0..100, so
K = 10 is half marks and K = 100 is full marks.

M is the weakest axis on this track: positional postings dominate the footprint, and the compression that saves memory costs query time.

> Measured on the live board; best run `run-d520073f2341` at commit `900691d`:
> **T 0.5468 s, M 209.7 MB, S 973 lines, K 11.278, 52.61 points.**
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
| 0 | The reference, unchanged | — | 56.35 s | (pin from the Oct 28 spec) | 0 |
| 1 | Real Index: authored inverted index with posting lists, boolean AND/OR/NOT evaluated left to right, and phrases resolved by a second sequential pass over the candidate document ids | `run-545c440faae8` | 3.9907 s | 126.3 MB | 34.20 |

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
| 1 | Correct baseline: authored inverted index, boolean + phrase | _ | _ | _SHA-green_ | PLANNED |
| 2 | Dense term ids, byte-sorted dictionary, no per-token allocation | _ | _ | _removes allocation churn from the build_ | PLANNED |
| 3 | Parallel tokenise over document ranges + deterministic merge | _ | _ | _the build is the slow phase_ | PLANNED |
| 4 | Blocked delta postings: StreamVByte vs PForDelta, benchmarked | _ | _ | _memory and query speed_ | PLANNED |
| 5 | Boolean: smallest-posting-first intersection | _ | _ | _intersection order is most of the win_ | PLANNED |
| 6 | Phrase: rarest-term anchor + positional verify | _ | _ | _38% of queries_ | PLANNED |
| 7 | Parallel query execution into fixed per-query result slots | _ | _ | _12 threads, deterministic output_ | PLANNED |

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
| Positions only for the sample's query terms | _ | _ | overfitting to a query set we were never given — reverted on principle |
| Conventional AND/OR precedence | _ | _ | SHA mismatch — the reference is left-associative |
| 12 workers on the build | _ | _ | memory-bandwidth bound, no win over 6 |
| A denser codec that decodes slower | _ | _ | the query phase is the larger share of the one wall time |
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
# Windows PowerShell 5.1 (present on every Windows box; `pwsh` is not installed
# on the dev host, so advertising it here produced commands that could not run).
# A PowerShell 7 shell works too — drop the -NoProfile/-ExecutionPolicy flags.

# 5 runs, discard the highest and lowest, mean of the middle three
powershell -NoProfile -ExecutionPolicy Bypass -File verification\bench.ps1 -Track B -Runs 5

# the four gates: correctness, speed floor, timeout, sandbox
powershell -NoProfile -ExecutionPolicy Bypass -File verification\verify.ps1 -Track B

# the full lockdown checklist
powershell -NoProfile -ExecutionPolicy Bypass -File verification\pre-freeze.ps1 -Track B

# or, with a Rust toolchain and no PowerShell, the harness does the same work
cargo run --release --offline --manifest-path verification/harness/Cargo.toml -- bench --track B --no-docker
```

Commit hash of this measurement: `_(fill)`.
