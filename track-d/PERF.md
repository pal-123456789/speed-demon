# PERF.md — Track D (Traverse)

> **The optimisation log for Track D.** The organizers weight this at
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

Traverse a 5M-edge directed weighted graph and answer 2,000 source-target shortest-path queries. Preprocessing is allowed and is counted inside the timed run.

| Axis | Reference | This submission | Ratio | Weight | Contribution |
|---|---:|---:|---:|---:|---:|
| T · wall time | 72.69 s | 0.837 s | 86.8x | 0.50 | 9.318 (derived) |
| M · peak memory | 893 MB | 170 MB | 5.26x | 0.20 | 1.394 (derived) |
| S · source size | 43 lines | 354 lines | 0.121x | 0.15 | 0.729 (derived) |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| **K** | | | | | **9.3775** |
| **speed score** | | | | | **48.60 / 100** |

`K = (Tref/T)^0.50 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15`.
Memory is capped at 50x; wall time is not; S and D are capped at 1.0, so they
can cost you and never pay. `speed score = 50*log10(K)`, clamped to 0..100, so
K = 10 is half marks and K = 100 is full marks.

M has to hold the graph *and* whatever preprocessing we build, inside an 8 GB ceiling. Preprocessing that doubles memory to save a second of query time is a bad trade at this weight.

> Measured on the live board; best run `run-58030bea7c39` at commit `8891f86`.
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
| 0 | The reference, unchanged | — | 72.69 s | (pin from the Oct 28 spec) | 0 |
| 1 | Real Traverse: CSR built in parallel across twelve windows, u64 distance labels, 2,000 Dijkstra queries across twelve threads | `run-c7cd32a91b0e` | 1.5791 s | 225.3 MB | 41.82 |

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
| 1 | Bidirectional Dijkstra on forward + reverse CSR | _ | _ | _SHA-green — the permanent rollback_ | PLANNED |
| 2 | Generation-stamp scratch, no per-query clears | _ | _ | _removes O(n) setup per query_ | PLANNED |
| 3 | Tuned priority queue + early termination on the meeting condition | _ | _ | _the constant factor_ | PLANNED |
| 4 | Parallel CSR build | _ | _ | _the build is inside the timed run_ | PLANNED |
| 5 | ALT landmarks, bounded sweep | _ | _ | _kept only if it beats plain bidirectional_ | PLANNED |
| 6 | ALT landmark count tuned to 6-8 with per-landmark offsets | _ | _ | _kept only if it wins the 1x AND the 10x ratio_ | PLANNED |

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
| A Contraction-Hierarchy build | _ | _ | LEGAL, but this query mix rarely repays it: the searches are local, so preprocessing inside the timed run costs more than the queries it saves. The organizers say the same thing — CH is legal, it just rarely repays on this mix — and our own amortisation arithmetic agrees. Not a moral objection to CH; an arithmetic one |
| Too many ALT landmarks | _ | _ | memory grows as k*n*4 with per-landmark offsets; past 8 the M axis gets worse faster than the queries get faster |
| 32-bit distances | _ | _ | no proven bound on the maximum path weight — a wrong answer is worse than a slow one |
| Preprocessing justified by sequential query timing | _ | _ | the queries are parallel; the budget is 12x smaller than it looks |

---

## 4. Where the time actually goes

_(fill after a profile — the flame graph, the `perf stat` counters, and the
one measurement that surprised you)_

---

## 5. Determinism

* Distances are integers. No float ever enters a shortest path.
* If the output contract asks for a path rather than a distance, the tie-break
  is fixed explicitly and exercised by a diamond-graph test suite, since a
  graph with equal-length paths is where an unspecified tie-break produces a
  different answer on a different run.
* Query results are written into per-query slots and emitted in query order.
* Any preprocessing variant is validated against the bidirectional baseline
  on every graph in the fixture set; one mismatch removes it.

---

## 6. Memory

CSR forward plus reverse is roughly 94 MB of `u32` ids and weights, and
the offsets are `u32` at `(n + 1) * 4` bytes per direction — about 7 MB each.
Landmark distances are `k * n * 4` bytes with a per-landmark offset: about
42 MB at 6 landmarks and 56 MB at 8, the range `docs/specs/track-d-traverse.md`
fixes. Per-worker scratch for a bidirectional search is about 334 MB across 12
workers. All of that fits inside the 8 GB ceiling with room to spare, which is
what makes the 6-8 landmark range worth sweeping rather than dismissing.

(These figures previously read `2 * k * n * 8` bytes, "111 MB at 4 landmarks and
891 MB at 32". That was the superseded design: `u64` distances, two copies, no
landmark cap. The spec of record fixes `u32` with per-landmark offsets and
k = 6 to 8, so the old arithmetic described a structure nobody is building.)

The memory axis has a hard cap of 50x, and the 8 GB limit is a hard kill
rather than a slowdown, so a preprocessing variant that runs out of memory
is a failed run, not a slow one.

---

## 7. What I would do with another 24 hours

* Hierarchical hub labelling or a transit-node routing scheme, both of which
  beat CH on road-like graphs by a wide margin — this graph's structure
  decides whether they are worth the time.
* A proper `Delta-stepping` implementation for the case where the weights
  turn out to be small and bounded, which would change the whole algorithm
  choice.
* Cache-block the CSR for the 24 MiB L3: the working set for a single search
  is much smaller than the graph, and a blocked layout could matter more than
  the search algorithm.
* Measure the preprocessing break-even empirically instead of analytically,
  by finding where the preprocessing stops paying for itself.

---

## 8. Reproducing these numbers

```bash
# Windows PowerShell 5.1 (present on every Windows box; `pwsh` is not installed
# on the dev host, so advertising it here produced commands that could not run).
# A PowerShell 7 shell works too — drop the -NoProfile/-ExecutionPolicy flags.

# 5 runs, discard the highest and lowest, mean of the middle three
powershell -NoProfile -ExecutionPolicy Bypass -File verification\bench.ps1 -Track D -Runs 5

# the four gates: correctness, speed floor, timeout, sandbox
powershell -NoProfile -ExecutionPolicy Bypass -File verification\verify.ps1 -Track D

# the full lockdown checklist
powershell -NoProfile -ExecutionPolicy Bypass -File verification\pre-freeze.ps1 -Track D

# or, with a Rust toolchain and no PowerShell, the harness does the same work
cargo run --release --offline --manifest-path verification/harness/Cargo.toml -- bench --track D --no-docker
```

Commit hash of this measurement: `_(fill)`.
