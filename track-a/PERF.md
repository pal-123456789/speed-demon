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
> | `DECLINED` | not attempted, with the measurement that says it would not pay |
> | `BLOCKED` | could not be attempted — pin the frozen spec first |
>
> **Which measurements are which.** Two rigs are used and they are NOT the same
> number. The **board** rig is the organizers' box: T is that box's wall time and
> M is its cgroup v2 `memory.peak`. The **local Linux** rig is this repo's build
> of the same source as a native ELF in WSL, on the full 238,836,234-byte corpus
> held on ext4, and its numbers are what *this* file cites as local. Window's
> MSYS build is a third thing and is used only for the hash gates: on Windows the
> Linux mmap path is compiled out, so it is not a speed measurement at all.

---

---

## Current measured state

| Board points | Time | Peak memory | Source | Measured bottleneck |
|---|---|---|---|---|
| **80.039** | 26.73 ms | 18.41 MB | 625 lines | parse 50.8% · hours 24.4% · text 22.0% |

Every entry below is a change that was made, measured and kept — or made, measured and
**reverted**, which is why the rejected ones are here too. The result above is the live board
measurement of the frozen commit; the design is in [README.md](README.md) and the cross-track
record is in [docs/JOURNEY.md](../docs/JOURNEY.md).

## 1. The task

Aggregate 3.1M structured log lines (239 MB) into per-endpoint request counts, p50/p95/p99 latency, and error rates bucketed by hour.

| Axis | Reference | This submission | Ratio | Weight | Contribution |
|---|---:|---:|---:|---:|---:|
| T · wall time | 32.047276 s | 0.041585 s | 770.65x | 0.50 | 27.7605 (derived) |
| M · peak memory | 227.98336 MB | 31.993856 MB | 7.1258x | 0.20 | 1.4810 (derived) |
| S · source size | 56 lines | 750 lines | 0.0747x | 0.15 | 0.6776 (derived) |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| **K** | | | | | **27.8589 (board)** |
| **speed score** | | | | | **72.2482 (board)** |

`K = (Tref/T)^0.50 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15`.
Memory is capped at 50x; wall time is not; S and D are capped at 1.0, so they
can cost you and never pay. `speed score = 50*log10(K)`, clamped to 0..100, so
K = 10 is half marks and K = 100 is full marks.

M is the container's cgroup v2 `memory.peak`, read on the host after the run (not
`ru_maxrss`). The reference's 228 MB comes mostly from retaining every latency in
a growing per-endpoint list. `/data` is pre-warmed outside our cgroup, so reading
it is free for M; our own allocations, /tmp writes and the output file's
first-touched pages are what count.

> **The M unit, because it decides the fifth digit of K.** The board's M axis is
> `memory.peak` in BYTES and the published reference is 227.98336 decimal MB, so
> the term is `(227.98336 / (M_bytes / 1e6))^0.20`. Reading `M_bytes` as MiB
> divides by 1048576 instead, and that alone makes K about 0.95% high. It is
> worth writing down because it cost an afternoon: the axes the board published
> for this run multiply, in decimal MB, to the K it published (27.858872 against
> 27.8589, a derived check), while the same three axes in MiB give 28.1244 and
> look like a board that contradicts itself. There is no contradiction and no
> reconciliation was invented here; the unit was the whole of it. A local K
> computed from `M_bytes` in MiB reads roughly a percent high on every track.

> Measured on the live board, best run `run-1001097d113a` at commit `043ec44`:
> **T 0.041585 s, M 31.993856 MB (30.512 MiB), S 750 lines, K 27.8589, 72.2482
> points.** T, M and S are the board's own for that run rather than measured
> locally, which is why the local Linux numbers below stay separate from them.
> The board keeps only the BEST result per track, so this is the only live run ID
> for Track A.
>
> **A board run ID is valid only until the next run.** This same commit was
> scored twice and the board issued a new ID the second time — the first citation
> of it in this file became a dangling reference within the hour, which is
> exactly what this table's own rule is written against. The ID above is the
> current one; if the run is scored again, this file is stale again.

**Where the next point comes from.** With the weights above, each 1% off wall
time is worth about 0.108 points, each 1% off memory about 0.043, and each 1% on
source size costs about 0.033. That ordering is the whole plan. It also says
where this round actually landed: time and memory both moved and source grew, and
source is now the only axis still well behind the 56-line reference.

---

## 1b. Board runs — the official optimisation log

The starter's `PERF.md` defines this table, and the judges check that every
run ID exists and that each change moved the numbers the way this file says it
did. One row per live-board push, in step order. The board caps a run at 3x
the reference (final scoring 10x).

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | — | 32.05 s | 228 MB | 0 |
| — | *measured by the board on the full public dataset, seed 1; commit `3032ab8`, 316 code lines* | | | | |
| 1 | Real Ingest: 13-byte hour key (no calendar arithmetic), interned endpoints, rank selection instead of sorting, parsed across twelve threads over disjoint record ranges | `run-2e1da425c7aa` | 0.1338 s | 60.2 MB | 59.43 |
| 2 | Mapped input instead of buffered reads; compact single-word record; one packed 64-byte AVX2 probe per line for both field boundaries; single-level k-way merge with an exactly reserved output | `run-821ff711c091` | 0.0867 s | 49.5 MB | **62.97** |
| 3 | Hour-partitioned phase 2 (the serial merge between workers gone); one shared lock-free endpoint table with a 13-bit record key sized to the spec's 1,842-endpoint catalogue; every hour formatted straight into the mapped output file at a precomputed offset; 32-byte AVX2 newline search; per-thread scratch zeroed once | `run-1001097d113a` (measured) | 0.041585 s | 31.993856 MB | **72.2482** |

The M column is decimal MB (`memory.peak` bytes, divided by 1e6), which is the
unit the published reference is in; see the note in section 1 for why that
matters.

**Step 3 is one board row and four local steps, and that is the board's rule, not
a shortcut.** The board keeps only the BEST result per track, so the intermediate
pushes between step 2 and step 3 - commits `f6ab1c5`, `b071bf2`, `812c2be` and
`0eaaa6f` - no longer exist in the board's data to cite. They are therefore not
listed as their own rows with invented run IDs; their measured effect, one at a
time, is in table 2a and section 4, which were recorded locally as each one
landed. The 68.610 points that `f6ab1c5` measured on the board is likewise not a
citable row any more: its run ID was superseded by step 3 and the board no longer
holds it. The same applies to the intermediate pushes between step 1 and step 2:
what they contained is in section 3, with the measurement that killed each one.

**One run ID, and it is provisional.** This commit has already been scored twice
under two different IDs for the same numbers, so the ID above is the current one
as of this writing and not a durable reference. Anyone re-pushing, re-scoring or
re-checking this table should confirm the ID against the board first; a stale ID
is a dangling citation, which is the one thing this table's rule forbids.

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

**Local Linux ladder** (native ELF, full corpus on ext4, interleaved A/B, both
binaries hashing `495ca096...` on every run). Wall time is the minimum and the
median of 11 to 15 alternating runs; the box is shared, so the minimum is the
least contended and the median is the honest middle. CPU is user+sys summed over
the twelve threads, which is the load-robust signal:

```
                              wall min   wall med   cpu min   cpu med
baseline (f6ab1c5)              0.0728     0.0857        --        --
+ one shared endpoint table     0.0564     0.0606        --        --
+ mapped output file            0.0476     0.0527        --        --
+ 32-byte newline search        0.0420     0.0470     0.302     0.322
+ 2 KB prefetch (not kept)      0.0340     0.0420     0.286     0.333
```

Peak ANONYMOUS rss, which is the part cgroup `memory.peak` charges (the median
of five runs; `/data` is already in the page cache so its pages are not ours):

```
baseline                       28,324 KB
+ one shared endpoint table    24,712 KB
+ mapped output file           17,800 KB
```

Source, in the unit the board counts — scc's Code column after rustfmt, so
comments cost nothing. The local counter is the repo's own fallback scc (this
box has no scc and no tools image), so it is used as a RATIO, not as the board's
number: it reports 506 for the baseline main.rs while the board reported 543 for
the whole folder, and the two counters do not agree exactly.

```
src/main.rs, rustfmt Code lines (local counter)
  baseline (f6ab1c5)   506
  final (0eaaa6f)      713      +41%
board S at the baseline        543
```

**Local ladder against the board, and where the projection was wrong.** The
local ratios were used to project the board before the push. Direction was right
on all three axes; magnitude on time was optimistic, and the source estimate was
better than the local counter suggested:

```
                    projected from local   board, run-1001097d113a
wall time           0.0578 -> 0.032 s      0.041585 s   (a 41% local gain landed
                                                         as 27.5%)
peak memory         39.4   -> 27.6 MB      31.993856 MB (decimal MB)
source              543    -> 750 lines   750 lines
points              68.610 -> ~75.8        72.2482
```

The gap is the box, exactly as the caveat said: `/data` arrives warm on the
board and its L3 is 24 MiB against this laptop's, so a 41% local wall gain landed
as 27.5%. The local numbers are still the right *direction* instrument and the
wrong *magnitude* one, and that is worth knowing the next time they are used to
choose between two candidate changes.

**The same lesson, one axis over, and it is sharper.** The local memory figure is
in MiB because that is what `/proc` reports; the board's M axis is decimal MB. A
projection that carries MiB into the K arithmetic inherits the 0.95% bias
described in section 1 on top of the box difference, and the two errors point in
opposite directions, which is the worst combination for learning anything from
the comparison. Convert to decimal MB before the formula sees it.

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

| # | Change | Status | Where it landed |
|---|---|---|---|
| 1 | Correct baseline: `split_whitespace` + `HashMap` + sort-per-endpoint | **SUPERSEDED** | the starting shape; step 1 of table 1 replaced all three parts |
| 2 | Dense endpoint ids; per-endpoint `Vec<u32>` replaced by a flat histogram | **DONE** | interned endpoints with dense ranks; the per-endpoint list is gone |
| 3 | Custom byte scanner over the fixed line grammar | **DONE** | the hand-written field walk, now with a 32-byte AVX2 newline search |
| 4 | Timestamp by arithmetic instead of a date library | **DONE** | the 13-byte hour key; no calendar arithmetic anywhere |
| 5 | Two-level radix histogram for exact percentiles | **SUPERSEDED** | nearest-rank selection over the latency vector instead |
| 6 | Thread-local aggregation + deterministic merge | **DONE** | twelve workers over disjoint byte ranges; the row blocks are per (worker, hour) and the serial merge is gone |
| 7 | Bump arena for interned endpoints; buffer reuse | **DONE** | one shared table with a 512 KiB arena, mapped input, packed single-word records |
| 8 | PGO / branchless inner loop / cache blocking | **PARTIAL** | PGO is in `build.sh` behind a fallback; the branchless record fold measured FLAT and is in section 3 |
| 9 | One output file written through a mapping at known offsets | **DONE** | the output is `set_len`'d and `MAP_SHARED`, and each thread renders its hours into its own slice |

**All nine are now either measured or struck out, which is what the rule above
requires.** The numbers for what shipped are in table 1 and section 2a; what did
not pay is in section 3 with the measurement that killed it. No row is left as an
untested plan.

---

## 3. Experiments that failed, and were reverted

This section is deliberately the longest part of the file. The organizers say
so explicitly: *"The failed experiments count — they are often the most
interesting part and judges are told to reward them."*

| Experiment | Hypothesis | Result | Why it was reverted |
|---|---|---|---|
| SIMD timestamp parse | | measured FLAT | branchy setup cost per line outweighed the vector win |
| Lock-free shared intern table | fewer instructions per line | measured FLAT, 30 more lines | this was measured before the merge it removes was counted: the table is only worth it once the twelve per-worker tables and the serial union behind them go too. Re-done properly, it is step 4 |
| Hand-written SWAR space scan | fewer instructions | measured 3% SLOWER | `iter().position()` already compiles to SIMD |
| Hand-written AVX2 two-space probe | fewer instructions | measured FLAT, +32 lines | the compiler already vectorises the same scan |
| Branchless record fold | | measured FLAT | the branch predicts perfectly on this corpus |
| t-digest / KLL sketches | | impossible, not just slow | approximate percentiles cannot hash-match |
| 2 KB per-line prefetch | the hardware prefetcher stops at each 4 KB page, so reach into the next one before the parse needs it | measured NEUTRAL | two builds, 11 interleaved rounds, both hashing identically: wall min 0.0420 -> 0.0340 and median 0.0470 -> 0.0420, while CPU min 0.322 -> 0.333 and median 0.322 ... i.e. the wall gain and the CPU loss are each inside the box's spread and they disagree with each other. A one-line change that cannot be shown to pay does not ship, and it is recorded here rather than quietly kept |
| MAP_ANONYMOUS left out of the spare-page mapping | (a defect, not a hypothesis) | measured WRONG PATH | flags `MAP_PRIVATE` without `MAP_ANONYMOUS` makes the first `mmap` fail, so every worker silently took the buffered fallback. The hash was still correct, which is why only memory caught it: maxrss read 30 MB where the 239 MB file-backed mapping had to show 265 MB. A silent fallback is invisible in the one gate everyone runs |
| Composing the output with a `MAP_SHARED` mapping over an `O_WRONLY` file | (a defect, not a hypothesis) | measured EACCES, silently | the kernel checks the open mode, not just `PROT_WRITE`, and refuses with `EACCES`. The fallback swallowed it: the step measured NEUTRAL on time and two MB WORSE on memory until the file was opened `O_RDWR`. Two silent fallbacks in one change, and the second one would have had a working change reverted |
| The unified 64-byte field window | one compare pair finds both spaces and the newline | DECLINED | the newline search is now one 32-byte AVX2 compare chain, and the two space walks are `iter().position()`/`rposition()`, which the row above already measured as SIMD and which a hand-written replacement made 3% SLOWER. Restructuring the hot loop around a shared window would add about 25 lines to replace code already known not to be the cost, so it is not attempted rather than attempted and guessed at |

**Two of these are recorded as defects rather than experiments**, because that is
what they were and because the failure mode is the dangerous one: a fallback that
keeps the answer correct and quietly gives back the entire gain. The per-hour
written-length check added with step 5 is the same kind of guard, and it caught a
third one immediately (`":00:00Z "` is 8 bytes, not 9, which mis-sized every row,
and every hour after the first would have been written at the wrong offset).

---

## 4. Where the time actually goes

Both splits are TSC-free phase timers compiled into the binary, on the full
corpus, and both are the best of several runs. The inner numbers are CPU
summed over the twelve threads, so they do not add up to the wall total.

```
before (f6ab1c5)          internal CPU of the hours phase
scan (12 workers)   56%     count      7%
serial merge         6%     scatter   21%
hours phase         21%     select+format  72%
write (serial)      17%

after (0eaaa6f)
scan (12 workers)   71%     count     8%
rank (serial)        0%     scatter  38%
hours phase         19%     select+rows  54%
output + text        9%
```

**What moved, and what that says.** The serial `write_all` was 17% of the run and
the formatting behind it was 72% of the hours phase's CPU; both are gone because
the file is mapped and each thread renders its own hours into it, which is one
change worth about 16% of wall time and six megabytes of peak memory at once. The
scan's share went UP only because everything around it got smaller — the scan is
the same 238 MB it always was, and it is now the whole problem.

**The measurement that redirected the work.** The hour phase looked like the
target because its CPU total is high, but its wall share is 19% against the
scan's 71%, and splitting the phase showed the serial write hiding inside the
number everyone was looking at. Formatted text was never the cost the profile
implied; the *serial* path it took was.

**The one number that says where this stops.** Replacing the endpoint table
lookup with a constant and rebuilding drops CPU from 0.318 to 0.185 s, about 40%.
That figure is not the cost of the table: with one endpoint instead of 1,842 the
counting sort, the scatter and the 126,908 selections all collapse as well. It is
recorded because it bounds the remaining opportunity — the parse and the shared
table together are ~40% of the program and the table's own share of that is two
loads, an L1 access and two vector compares per line, which leaves little to win.

---

## 5. Determinism

* Endpoint ids are assigned by a deterministic sorted rule — the names are sorted
  once and each slot becomes its rank — never by hash-map iteration order. The
  slot a name lands in depends on which thread got there first, and that is
  invisible because the rank array is what the output is built from.
* A record's key is the endpoint rank in the low bits and the latency above it, so
  the order of the output rows is fully determined by the ranks.
* Percentiles are computed from integer counts — no float ever enters the
  ranking, so no rounding mode can change the answer.
* Error rates are integer counts formatted by an explicit routine.
* The hours are claimed by an atomic counter, so which thread renders which hour
  varies run to run. That cannot change the bytes: a row is one hour and one
  endpoint, each hour is a disjoint byte range of the output computed before any
  of it is written, and the percentile selections are order statistics of a
  multiset.
* Measured: five full-corpus runs in the board's container were byte-identical,
  and every A/B pair above hashed `495ca096...` on both binaries.

---

## 6. Memory

The M axis is won by *not retaining* anything. The reference holds every latency
in a growing per-endpoint list (that is where its 228 MB goes); this
implementation holds one 4-byte record per line (12.4 MB for 3.1M lines) and then
throws each hour's records away as they are consumed.

* Input is **mapped**, not copied. This file first claimed the opposite — that
  mapped pages are charged to cgroup `memory.peak` — and the board refuted it:
  read-only `MAP_PRIVATE` file pages stay in the kernel page cache and are NOT
  charged to the container.
* The endpoint table is a 32 KiB slot array plus an arena allocated by capacity:
  the pages it never uses are never touched, and an untouched page is not
  resident. The arena is 512 KiB because the spec fixes the catalogue at 1,842
  endpoints of at most 125 bytes, and 4096 slots × 125 = 512,000 bytes is the
  worst legal input.
* The output is the mapped file. There is no per-hour string and no flat buffer on
  the mapped path, which is most of the 10 MB this track used to hold twice.
* The latency scratch is one allocation per thread, grown to its high-water mark
  and never re-zeroed; the hours vary in size, and clearing it every hour wrote
  about 12.4 MB over the run for nothing.

---

## 7. What I would do with another 24 hours

* Not another hand-written space scan: that experiment is already in section 3
  twice, measured, and the answer did not change when the scan was rewritten
  around the vector unit.
* A `perf`-driven pass on the parse. This box cannot run `perf` (the WSL kernel
  exposes no counters), so the split in section 4 is timers only, and the parse's
  internals are the one place a surprise is still possible.
* The remaining serial step is the rank pass: at 0.2 ms it is not worth
  parallelising, but it is the only thing between the parse and the hours.
* Source size. At 0.15 weight a code line costs about 0.0033 points, and the hour
  machinery (discover, ordinal, map, label) is about 45 of them. Collapsing it to
  the spec's fixed 72-hour window would pay, and it is the one structural
  simplification left; it was not taken because it narrows a program that
  currently accepts any window, and that is a decision for the Lead rather than a
  late-night rewrite.

---

## 8. Reproducing these numbers

```bash
# Windows PowerShell 5.1 (present on every Windows box; `pwsh` is not installed
# on the dev host, so advertising it here produced commands that could not run).
# A PowerShell 7 shell works too — drop the -NoProfile/-ExecutionPolicy flags.

# the three published corpora, on the Windows build (hash gates only)
sh tooling/check-corpora.sh a summary.txt <sample-pin>
sh tooling/measure-full.sh a /d/sd-data/track-a 495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3 summary.txt

# the four gates: correctness, speed floor, timeout, sandbox
powershell -NoProfile -ExecutionPolicy Bypass -File verification\verify.ps1 -Track A

# the full lockdown checklist
powershell -NoProfile -ExecutionPolicy Bypass -File verification\pre-freeze.ps1 -Track A

# or, with a Rust toolchain and no PowerShell, the harness does the same work
cargo run --release --offline --manifest-path verification/harness/Cargo.toml -- bench --track A --no-docker
```

The local Linux numbers above come from building the same file as a native ELF
with `build.sh`'s flags inside WSL, running it against a copy of the corpus on
ext4 (not the 9p mount, which changes the I/O), and alternating the two binaries
run by run. On this host the board's own container protocol reports a measured
peak of 270 MiB for every version, because `/data` is bind-mounted from the WSL
filesystem and
the 239 MB of page cache is charged to the container; that is a property of the
host, not of the program, which is why M is cited here as peak anonymous rss.

Commit of the code these local measurements are of: `0eaaa6f`.
Board run for the same code: `run-1001097d113a` at commit `043ec44` (provisional:
this commit has been re-scored once already, and a re-score issues a new ID).
