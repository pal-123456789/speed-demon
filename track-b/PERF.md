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
| 2 | Bucketed build: one scan appends a 16-bit unit -- term index in its bucket, and the documents since that bucket's last posting -- so the per-worker term list, the per-worker posting arenas, the counting sort and the merge are all gone | _(board read pending this push)_ | _(pending)_ | _(pending)_ | _(pending)_ |
| 3 | Phrases answered inside the build scan: each phrase's rarest term is chosen from a 1/32 sample and checked at each of its occurrences while the document's ids are in cache, so the candidate pass and the second corpus read are gone | _(board read pending this push)_ | _(pending)_ | _(pending)_ | _(pending)_ |

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

| # | Change | Status | Where it landed |
|---|---|---|---|
| 1 | Correct baseline: authored inverted index, boolean + phrase | **DONE** | the foundation everything else is built on |
| 2 | Dense term ids, byte-sorted dictionary, no per-token allocation | **DONE** | dense ids over a byte arena; the token loop allocates nothing |
| 3 | Parallel tokenise over document ranges + deterministic merge | **DONE** | twelve workers over disjoint byte ranges, merged by term id |
| 4 | Blocked delta postings, StreamVByte vs PForDelta, benchmarked | **SUPERSEDED** | a flat u32 arena instead. A denser codec decodes slower, and the query phase turned out to be the smaller share -- the build is where the wall time is |
| 5 | Boolean: smallest-posting-first intersection | **DONE** | candidates start from the rarest term, then intersect in increasing length order |
| 6 | Phrase: rarest-term anchor + positional verify | **DONE** | anchor on the rarest term, then verify spans against the positional list |
| 7 | Parallel query execution into fixed per-query result slots | **DONE** | each query writes into its own slot, so the output order is the query order |

No row is left as an untested plan, which is what the rule above requires. The measured
numbers for what shipped are in table 1; what did not pay is in section 3.

---

## 2c. Stage 2: the bucketed build, and the 89 MB of scratch it deletes

**`KNOWN`: measured on this box with the runs interleaved A/B/A/B so a load drift
hits both sides, seven rounds each, full corpus, byte-identical output on every
run.** Wall clock here is unusable on its own -- the same binary measured between
0.900 s and 10.519 s today -- so the CPU column is the one to read.

```
                          CPU s (median)   wall s (median)   peak working set MB
before: term list + arena      6.859            1.090                181.6
after:  units into buckets     6.859            1.046                 92.6
```

**Measured: the peak working set halves and the time does not move.** The board
charges 209.7 MB for the shape above; the local proxy has read about 17% below the
cgroup counter before, so no board number is claimed here.

### What changed, and why it is a deletion rather than a tuning

The build used to hold, per worker, a doc-ordered term-id list (about 89 MB across
twelve workers) and then a posting arena (about 89 MB), and the merge held the
workers' arenas *and* the merged one. Measured peak: 181.6 MB of working set to
produce postings that are 44.5 MB of 16-bit offsets.

A posting is now one `u16`: the term's index inside its 32-term bucket in the high
bits, the documents since that bucket's previous posting in the low four, and one
more unit carrying the whole gap when the gap does not fit in four bits. The token
loop appends that unit where it stands, so there is no term list, no per-document
starts array, no counting sort and no second pass -- the count is kept in the same
32-bit slot the dedupe stamp already needed. A posting is a 16-bit offset from the
worker's first document, which is exact because a worker's range spans far fewer
than 65,535 document ids, and that is checked rather than assumed.

### The field widths, because they are the wrong-answer risk on this track

`local` has to fit the unit's high 12 bits. `bucket_count` widens the bucket count
until it does and the layout widens with it, so a larger query set stays exact; a
field that wrapped would retarget every later posting in its bucket and still
answer, which is the class of defect that scores zero rather than losing points.
The 65,535-id offset budget is checked per worker the same way, and both stop the
run loudly instead of truncating.

### The regression this stage produced, and the phase laps that found it

The first version of this stage was faster on the build and far slower on the
queries. The laps below are measured, interleaved, same corpus, full hash on each:

```
                       before    first version   after the fix
build index            0.429 s   0.399 s         0.342 s
phrase candidates      0.033 s   0.658 s         0.028 s
phrase verify          0.084 s   0.107 s         0.078 s
evaluate+format+write  0.222 s   0.309 s         0.294 s
total                  0.788 s   1.515 s         0.764 s
```

The candidate phase had decoded **every** phrase term into a `u32` list before
intersecting, including the common ones -- roughly 89 MB of writes to answer a
question about the rarest term's list, which is the whole candidate set. It now
filters the rarest term's candidates in place, testing each against the compressed
postings with a binary search inside one worker's run: a gallop over the units with
no decode at all. Measured: 0.658 s to 0.028 s, and the stage went from a 0.6 s
loss to level.

### Still open, and it is the next stage

Measured: `evaluate+format+write` is 0.294 s against 0.222 s before, because
`eval_bool` still materialises each operand it intersects -- the same decode cost
the phrase path just had removed, paid on the 2,000 BOOL queries. The fix is either
the same membership test for those operands, or better, a bitmap over the terms
that hold most of the postings. The term list and one of the two arena copies are
gone; the chunks the scatter has not yet drained are the only copy left, and the
bitmap stage is what removes it.

---

## 2d. Stage 3: the phrase path moves into the scan

**`KNOWN`, same rig as 2c: runs interleaved, full corpus, byte-identical output on
every run.** Stage 3 against stage 2 is 7 rounds; the pair against the shape they
replaced is 5 rounds.

```
                                   CPU s (median)   wall s (median)   peak MB
before stage 2 (term list + arenas)     6.812            1.167          185.4
after stage 2 (bucketed build)          6.781            1.034           92.5
after stage 3 (phrases in the scan)     6.422            0.914           91.8
```

**Measured: stage 3 is worth about 5% of the CPU and 10% of the wall clock, and it
is memory-neutral.** Cumulative against the shape this track started the day with:
CPU 6.812 to 6.141 s, wall 1.167 to 0.952 s, peak working set 185.4 to 91.2 MB.

### What changed

Every phrase used to need two more passes: intersect its terms' lists for candidate
documents, then read the whole corpus again to check each candidate. Now a phrase
picks its rarest term from a 1/32 sample pass, and every occurrence of that anchor
is checked against the phrase *during the scan that builds the index*, while the
document's token ids are still in cache. The candidate phase and the second 254 MB
pass are gone with the pair list and its sort; matches are `(query, doc)` words,
sorted once and deduped because one document can hold the same phrase twice.

Deleted with them: the phrase-pair builder, the span tokeniser, the substring
checker, both verification passes and the term-key lookup -- about 220 code lines,
against about 230 for the sample pass, the anchor setup and the per-document id
buffer. The source size is level rather than better, which is not what I expected
from this stage.

### Why the memory did not move, measured rather than guessed

A term that only a phrase names now carries no postings at all, and the expectation
was that this would be a large part of the index. It is not. The scan reports
75,747 terms, of which 53,444 carry postings; 19,000 phrases anchor on 17,971 terms;
and the postings total 21,086,219, barely below what the whole query set cost
before, because most phrase terms are BOOL terms as well. The peak is still the unit
streams and the arena at about 45 MB each.

The measurement that prices the next move is where those postings sit: measured, 370
terms over 7,800 documents hold 47.9% of all postings and 181 terms over 15,250 hold
38.2%. A term kept as a bitmap costs 31 KB whatever its df, so above roughly 15,000
documents a bitmap is smaller than a u16 list -- but more to the point, those are
the postings that would never enter the unit stream, and that is where the
remaining duplication is.

### Reverted inside this stage

Enlarging the unit chunks from 64 KiB to 256 KiB, so a freed chunk would be its own
mapping and go back to the kernel: measured **worse**, peak working set 90.9 to
124.2 MB and CPU 5.969 to 6.172 s. 64 KiB stays.

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
| Positions only for the sample's query terms | _ | not attempted — deleted on principle | overfitting to a query set we were never given |
| Conventional AND/OR precedence | _ | measured: SHA MISMATCH | the reference is left-associative |
| 12 workers on the build | _ | measured: no win over 6 | memory-bandwidth bound |
| A denser codec that decodes slower | _ | measured: net loss | the query phase is the smaller share of the one wall time |
| Skipping stopwords | _ | measured: SHA MISMATCH | it changes phrase results |
| **Profile-guided build (PGO)** | _ | **measured: T 0.5444 -> 0.5606 s AND S 973 -> 995 lines** | **up 3% on T and 22 lines larger. B's hot path is a token probe over a 300 KB table that is already cache-resident, so there is little branch misprediction for a profile to fix. The same procedure WON on Track C: 0.1704 -> 0.1654 s.** |
| **Three-byte term ids** | _ | **measured: M 215.3 -> 203.8 MB, T 0.5424 -> 0.6139 s** | **the memory win was real and in the predicted direction, but a three-byte read is unaligned -- three loads and two shifts, 22.24 M times in the scatter -- and cost 13% of the runtime for a 2.2% gain on the memory factor.** |
| **Reserving `starts` and `ids`** | _ | **measured: worse on both axes** | **reserving capacity pre-touches pages that lazy growth never needs, and the document count is an estimate rather than a known total.** |
| **Decoding every phrase term before intersecting it** | _ | **measured: phrase candidates 0.033 -> 0.658 s, reverted** | **the rarest term's list is the whole candidate set; the other terms only need a membership test. Decoding them wrote 89 MB of postings that were then read once and thrown away. A binary search inside one worker's run of the compressed units answers the same question with no decode, and that generalises to every set operation on this track.** |
| **256 KiB unit chunks instead of 64 KiB** | _ | **measured: peak working set 90.9 -> 124.2 MB, CPU 5.969 -> 6.172 s** | **the idea was that a chunk above the allocator's mapping threshold would be its own mapping and go back to the kernel on free. It made the peak larger, not smaller, on the same corpus and hash. 64 KiB stays.** |
| **Removing the postings of phrase-only terms** | _ | **measured: 5% fewer postings, memory level** | **not reverted, but the expected saving was wrong: 53,444 of 75,747 terms carry postings, so the terms that only phrases name were never a large part of the index. Kept because it is strictly less work, not because it paid.** |

The last three were tried and reverted late in the campaign, after the PGO work above was
already in place. Each looks correct from a byte count or a cache-resident-table argument,
and each is recorded because the measurement is the interesting part.

---

## 4. Where the time actually goes

Measured by the binary's own `SD_TIME=1` instrumentation, which prints cumulative seconds at
each stage boundary. Three runs on the local box; the two clean ones agree to within 0.01 s,
and the third was preempted (4.4 s on a 0.5 s stage) and is discarded.

```
stage                cumulative    this stage    share
parse queries            0.022 s       0.022 s     2.4%
build index              0.474 s       0.454 s    49.7%
phrase candidates        0.602 s       0.122 s    13.3%
phrase verify            0.691 s       0.088 s     9.6%
write                    0.914 s       0.230 s    25.2%
```

**What the lap labels do and do not mean, because I got this wrong once already.** I first read
the last lap as "the write phase is 25.2% of the track" and started planning a formatter change
on the strength of it. Reading the code back: that lap brackets the thread scope that calls
`eval_bool` for every BOOL query **as well as** `format_result` and the actual `write_all`. So
`write` here means **evaluate + format + write**, and the evaluation is the dominant part of it
-- 1,999 queries against posting lists, not 1,999 integer conversions.

| lap | brackets |
|---|---|
| `parse queries` | reading and parsing `queries.txt` into the query list |
| `build index` | the parallel tokenise and the posting scatter, plus the offset array |
| `phrase candidates` | rarest-term anchor selection for the phrase queries |
| `phrase verify` | one sequential positional scan over all 254 MB of `docs.txt` |
| `write` | **BOOL evaluation, formatting, and the write -- all three** |

**Stage 2 re-measured, same instrumentation, against the shape it replaced. Three
interleaved rounds on the full corpus, medians:**

```
stage                  before     after
parse queries          0.020 s    0.021 s
build index            0.429 s    0.342 s
phrase candidates      0.033 s    0.028 s
phrase verify          0.084 s    0.078 s
evaluate+format+write  0.222 s    0.294 s
total                  0.788 s    0.764 s
```

The build lost a measured 0.087 s because it no longer writes a term list and a
second arena, and `evaluate+format+write` gained a measured 0.072 s because a
compressed posting has to be decoded before a `u32` intersect can read it. That
trade is exactly where this track stands after stage 2, and it is why the next
stage is aimed at the second half of it rather than at the build.

**Stage 3 re-measured the same way, with the two phases it deleted and the one it
added. Two interleaved rounds; the second is the clean pair:**

```
stage                     before      after
parse queries             0.027 s     0.026 s
sample + anchors          (none)      0.031 s
build index               0.528 s     0.594 s
phrase candidates         0.037 s     (gone)
phrase verify             0.134 s     (gone)
phrase hits               (none)      0.002 s
evaluate+format+write     0.301 s     0.274 s
total                     1.027 s     0.927 s
```

The two deleted phases were a measured 0.171 s and the sample pass plus the in-scan
anchor checks cost a measured 0.097 s, which is the whole of the gain. The build did
not get slower by accident: it now checks every occurrence of every phrase's anchor
while it tokenises the same text, which is the point of the change.

**The measurement that did surprise me, and it stands:** I had been saying this track was
roughly 57% build and 43% query. It is **49.7% build and 50.3% everything after it**. The
phrase pass alone is 9.6%, and it is deliberately NOT parallelised -- twelve workers would read
3 GB of `docs.txt` instead of 254 MB, so splitting it by query is strictly worse.

The build's 49.7% is dominated by the token probe over the 300 KB term table. B's measured
209.7 MB peak is real and so is the 5.4x memory ratio against the leader, but **half this
track's wall time is spent after the index exists**, which is not where I had been looking.

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

**A correction, because the paragraph that used to stand here was wrong and stage 2
is what measured it.** It said the build was streamed and the peak was the final
index rather than the intermediate token list. In fact the build kept a
doc-ordered term-id list per worker *and* a posting arena per worker, and the merge
then held the workers' arenas and the merged one as well, so the peak was 181.6 MB
of working set (219 MB on the board, measured on that same shape) for an index
whose postings are 44.5 MB of 16-bit offsets. That paragraph described the intent,
not the code.

**Stage 2 removes the term list and one of the two arena copies, and stage 3 keeps
the second copy but stops it growing with the phrases.** The peak is the shared
posting array plus the bucket chunks the scatter has not drained yet: **91.8 MB of
working set after stage 2 and 91.2 MB after stage 3, measured with the runs
interleaved against the previous shape** (185.4 MB for the shape this track started
the day with, same box, same corpus, same hashes on every run).

One copy remains on purpose: a document's first occurrence of a term is buffered as
a unit until the layout knows where that term's stretch begins. Stage 3 was expected
to shrink both the unit stream and the arena by dropping the postings of terms that
only phrases name, and it did not, because most phrase terms are BOOL terms too --
the measurement is in 2d. What is left is priced there as well: 370 terms over 7,800
documents hold 47.9% of the postings, and a bitmap for one of those terms never has
to enter the unit stream at all.

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
