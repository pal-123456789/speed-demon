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

> **Read 2f first if you want the current state of this track.** The table in
> section 1 and the stage narrative in 2c/2e/3/4 record the shape this track was
> frozen at (stage 2, 55.63 points) and the stage that was reverted on the board
> (stage 3). The board has since measured the same track family at **58.290
> points**, and stage 4 -- the component audit, the CPU-second phase split and the
> one change it justified -- is in **2f**. Section 1's table is kept because the
> board-run ledger in 1b is cumulative and each row cites the run that measured it.

## 1. The task

Build an inverted index over a 250 MB text corpus, then answer 50,000 boolean and phrase queries. Build time and query time are measured together, as one total wall time. There is no build/query split any more.

| Axis | Reference | This submission | Ratio | Weight | Contribution |
|---|---:|---:|---:|---:|---:|
| T · wall time | 56.35 s | 0.4776 s | 118.0x | 0.50 | 10.862 (derived) |
| M · peak memory | 2200 MB | 129.6 MB | 16.98x | 0.20 | 1.765 (derived) |
| S · source size | 91 lines | 1150 lines | 0.0791x | 0.15 | 0.684 (derived) |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| **K** | | | | | **12.957** |
| **speed score** | | | | | **55.63 / 100** |

`K = (Tref/T)^0.50 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15`.
Memory is capped at 50x; wall time is not; S and D are capped at 1.0, so they
can cost you and never pay. `speed score = 50*log10(K)`, clamped to 0..100, so
K = 10 is half marks and K = 100 is full marks.

M is the weakest axis on this track: positional postings dominate the footprint, and the compression that saves memory costs query time.

> The row above is the board's best for this track, at commit `a1e256e` (step 2 in
> 1b): **T 0.4776 s, M 129.6 MB, S 1150 lines, K 12.957, 55.63 points.** The step
> after it was measured on the board and reverted, so the frozen shape is this one.
> T is the P-cores timed run and M is cgroup v2 `memory.peak`, both read from the
> board rather than measured locally. A target is not a result, so the local
> harness numbers below stay separate from these.
>
> For the record, the previous best, measured on the live board at `run-d520073f2341`
> and commit `900691d`: T 0.5468 s, M 209.7 MB, S 973 lines, K 11.278, 52.61 points.

---

## 1b. Board runs — the official optimisation log

The starter's `PERF.md` defines this table, and the judges check that every
run ID exists and that each change moved the numbers the way this file says it
did. One row per live-board push, in step order. The board caps a run at 3x
the reference (final scoring 10x). Every number below is a published board
read, and the S and K columns are inside the change cell because the starter's
table does not carry them.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | — | 56.35 s | (pin from the Oct 28 spec) | 0 |
| 1 | Real Index: authored inverted index with posting lists, boolean AND/OR/NOT evaluated left to right, and phrases resolved by a second sequential pass over the candidate document ids | `run-545c440faae8` | 3.9907 s | 126.3 MB | 34.20 |
| 2 | Bucketed build: one scan appends a 16-bit unit -- term index in its bucket, and the documents since that bucket's last posting -- so the per-worker term list, the per-worker posting arenas, the counting sort and the merge are all gone. **The board measured S 1150 and K 12.957 at commit `a1e256e`; this is the best-known shape for this track and the one it is frozen at.** | `run-cb095521ec8c` | 0.4776 s | 129.6 MB | **55.63** |
| 3 | Phrases answered inside the build scan. **REVERTED: the board measured this 0.46 points worse than step 2, with time 4.6% higher, at commit `577a4de`; 2e is the full record of the disagreement between this box and the board.** | `run-cd1f53ba0ff9` | 0.4996 s | 127.9 MB | 55.17 |
| 4 | Per-document resolve-then-handle split of the build scan, with each term's per-worker state prefetched as its id is resolved: a labelled scratch A/B of four interleaved sittings measured the build's user CPU down between 11% and 20%, and the full-corpus pin reproduced 5/5. **Not yet on the board — the Lead pushes and the run ID is transcribed from the board UI, so this cell stays pending until then; 2f is the measurement record.** | _pending the Lead's push_ | _pending_ | _pending_ | _pending_ |

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

## 2e. Stage 3: measured on the board, and reverted

**`KNOWN` on the board, and this is the change that did not survive it.** Stage 3
moved every phrase's verification into the build scan. Locally it measured as a
clear win; on the board it was a loss, so it was reverted and this track is frozen
at stage 2.

```
stage                 local CPU s   local wall s   local peak MB   board T s   board M MB   board S   board points
2, kept                   6.781         1.034           92.5          0.4776       129.6        1150       55.63
3, reverted               6.422         0.914           91.8          0.4996       127.9        1159       55.17
```

**The local reading was -5.3% CPU and -11.6% wall; the board read +4.6% time.** Both
are honest measurements and they are not reconcilable by rounding: the seven-round
interleaved local A/B that produced the -11.6% had the output hash checked on every
run. This is the second time in this campaign that a large local wall-time win on
this track did not survive the board, which is why every number in 2c and 2d is
labelled as local and the board rows above are the ones that count.

**Why that can happen, and what it says about this track.** The two phases stage 3
deleted were real: measured locally at 0.171 s of a 1.027 s total, with the sample
pass and the in-scan anchor checks costing 0.097 s. On the board the same exchange
is a net loss, which is consistent with the work moving out of compute-bound phases
into the scan, where the scoring box is memory-bound. A phase ledger from a
development box ranks the phases; it does not rank the machines.

**What it leaves behind, because the negative result is the useful part.** Stage 3
also produced the measurement that prices the remaining memory: the postings total
21,086,219 across 53,444 of 75,747 terms, and 370 terms over 7,800 documents hold
47.9% of them while 181 terms over 15,250 hold 38.2%. Above roughly 15,000 documents
a 31 KB bitmap is both smaller than a u16 list and keeps those postings out of the
unit stream, which is the remaining duplication. It is not attempted in the time
left: it needs a document count up front, a term renumbering so the bitmap terms are
contiguous, and a two-representation query evaluator, and a half-landed query
evaluator scores zero on a holdout rather than scoring low.

---

## 2f. Stage 4: the component audit, the measured split, and the one change it justified

**Where this track actually stands.** The board measured `6ba0849` at **58.290 points**: T
`0.4028 s`, M `90.84 MB`, S `1531` code lines, D `1.00`. Memory had already come down from the
`219 MB` the same track read at the start of the session, which is why M is no longer the axis to
spend on. Everything in this section is a **local** measurement on the WSL2 dev box with the corpus
staged on ext4, and every one of them is labelled local; the board rows in 1b are the ones that
count.

### 2f.1 The phase split, in CPU seconds, not wall seconds

Wall time on this host cannot rank two variants: the same binary measured between `0.47 s` and
`5.46 s` in one sitting, because WSL2 charges about ten microseconds per first-touch page fault and
the box takes a variable load. So the phases are measured in **user CPU seconds summed over all
twelve workers**, which excludes fault handling and is the quantity that carries to the bare-metal
board. The binary's own laps were extended for this measurement to report wall, user CPU, system
CPU and `smaps_rollup` `Anonymous` at every boundary; that extension is a scratch variant and is not
shipped.

```
before stage 4, best of five runs, full corpus, 12 workers

stage                    wall     user     sys    share of wall
parse queries           0.028     0.02    0.01       4.6%
sample                  0.025     0.09    0.06       4.1%
build index             0.418     2.84    0.49      69.2%
phrase candidates       0.018     0.06    0.02       3.0%
phrase verify           0.080     0.58    0.06      13.2%
evaluate+format+write   0.035     0.25    0.04       5.8%
total                   0.604                           
```

**The build is 74% of the CPU, not the half the stage-2 profile in section 4 claimed**, and it is
not instruction-bound. Counting the scan directly (a scratch build that increments relaxed
counters) gives:

```
tokens scanned                    37,411,528
of which a query term             28,716,697   (76.8%)
of which a bitmap term            15,968,525
of which a new posting-list term  12,648,435   == the posting count
query terms interned                  75,747
terms held as bitmaps                    311   holding 9,592,095 postings
```

So 76.8% of every token in the corpus is looked up, and each hit does two *dependent* random reads:
the term-table slot and then the key it names, then the term's per-worker state. At about 3.3
CPU-seconds for 37.4 M tokens the scan spends on the order of 89 ns of CPU per token, which no
instruction-count model explains. It is memory latency, and that single fact predicted every
result below.

### 2f.2 Pricing each component by deleting it

Each row is a **labelled scratch build that answers WRONG on purpose**, timed with the SD_TIME laps
and discarded. User CPU is summed over threads and `base` is the committed shape. The three rows
marked † were priced in a second sitting against a base measured in the first, so their deltas carry
about ±10% on top of the run-to-run spread; `F_split`, `L16` and `L32` were interleaved with `base`
round by round and do not have that extra error.

```
variant       what it removes from the build scan              user CPU s   vs base
base          nothing (the shape that shipped)                     4.64        --
E_tokenize    everything after the token bounds                     0.58     -87.5%
A_probe       the whole per-token effect: hash + probe only         1.83     -60.6%
B_bmi         A_probe plus the bmi[t] load, nothing else            1.87     -59.7%
H_bits        the posting-list path, keeps the bitmap bit (†)       2.46     -47.0%
G_nslot       the unit writes, keeps the dedupe (†)                 3.06     -34.1%
D_noscatter   the scatter into the posting arena                    3.96     -14.7%
F_push        the Vec-of-Vec chunk cursor, replaced by a raw one (†) 4.88     +5.2%
L16 / L32     the prefetch ring 16 / 32 tokens deep            3.90 / 3.92  -2.5 / -2.0%
```

`F_split` -- the change that actually shipped -- is measured properly interleaved in 2f.4 and is
deliberately **not** given a row here: comparing it against this table's `base` would compare two
different sittings, which is the mistake this file keeps warning about.

Reading the deltas as shares of the one wall time, and letting the scratch noise average out:

```
tokenize (separator and token bounds)              0.58    12.5%
hash (2x CRC32 + multiply) + table probe           1.25    26.9%
bitmap path (15.97 M bit sets)                     0.59    12.7%
posting-list dedupe + count (12.75 M)              0.60    12.9%
posting-list unit writes + gap bookkeeping         0.94    20.3%
scatter into the posting arena (12.65 M)           0.68    14.7%
```

**Three of those results are worth more than the change they led to, because each one closes a door
that looked open:**

* **The `bmi[t]` read is free.** `B_bmi` minus `A_probe` is `0.04 s`, inside the spread. Renumbering
  terms by class so a token's role is read off its id range — the idea with the most obvious
  theoretical appeal, since it deletes one random read per token — would therefore buy about 1%,
  which is not measurable here. **Not attempted**, and that is the measured reason.
* **The chunk cursor is not the cost.** `F_push` replaced `Vec<Vec<u16>>::last_mut().unwrap()[len]`
  with a raw pointer, deleting roughly ten instructions per unit write on 12.65 M writes, and
  measured *five percent worse* (i.e. nothing). Consistent with 2f.1: the scan is waiting on memory,
  so deleting instructions is free of benefit. **Reverted, never committed.**
* **A deeper prefetch ring is not the cost either.** `L16` and `L32` measured inside the noise, so
  the eight-token lag is already deep enough to cover the slot load. **Not taken.**

### 2f.3 The change, and why it is the one the measurements point at

The two dependent random reads are: the term-table probe, and the term's per-worker state. The
existing eight-token ring covers **only the first** — it prefetches a token's slot and resolves the
token eight tokens later. But the per-term state was read *at the moment the token was resolved*, so
it had no lead time at all: it is one unhidden cache miss per hit, and there are 28.7 M hits.

So the per-document work is split into two passes:

1. resolve every token of the document into a small `ids` buffer, which is where the ring already
   drains, and **prefetch that term's `nslot` and `bmi` slots as each id is appended**;
2. run the per-term work over the buffer.

Nothing about *what* is computed changes, and the order does not change either: pass 1 emits ids in
token order and pass 2 walks them in that order, and all of a document's tokens are still processed
before the next document. That is the whole correctness argument for the gap chain (`prev[b]`), the
per-document dedupe stamp and the bitmap bits, all of which depend on document order and on nothing
else. The bound is `max line 7,609 bytes` in this corpus, so the buffer is at most ~3,800 `u32` per
worker.

The second pass now reads a working set of about 115 cache lines per document -- the terms that
document actually holds -- and pass 1 has already asked for all of them.

### 2f.4 Before and after, and what is not in the noise

`F_split` was measured **interleaved** against the committed binary, round by round, in four
separate sittings, because wall time drifts by more than the effect inside one sitting. `min` and
`median` are over the rounds of that sitting.

```
sitting   rounds   base user CPU (min / median)   F_split (min / median)   min delta   median delta
C            6         4.00 / 4.20                   3.31 / 3.68            -17.2%        -12.4%
D            8         5.68 / 6.12                   4.54 / 4.92            -20.1%        -19.6%
E           12         4.10 / 4.83                   3.62 / 4.31            -11.7%        -10.8%
F           10         4.62 / 4.84                   3.99 / 4.10            -13.6%        -15.3%
```

**Direction: a win in all four sittings. Magnitude: 11% to 20% of user CPU, and the spread between
sittings is the host's load, not the code.** Attributing the gain inside the change: the split on
its own measures between 5.6% and 7.0% better than base at the minimum, and the state prefetch
is the other half.

**What is NOT claimed. Wall time on this host moved less and inconsistently** -- one twelve-round
sitting read a `9.6%` wall improvement at the minimum and only `3.2%` at the median, because a third
of the local wall time is page faults that the scoring box does not pay the same way. And a point
estimate: this change is worth roughly `+0.9` to `+1.2` board points, which is **at or inside the
board's own ±1.5 point noise**. It is the right change on the measurement, but one board run of it
will not prove it, and nobody should treat a single unchanged-looking run as a refutation.

Memory did not move: `smaps_rollup` `Anonymous` peaks at `56.2 MB` after the change against `55.9
MB` before, both during `build index`, which is the phase that sets the peak.

### 2f.5 Exactness

Every gate, run against the committed binary:

```
full corpus, 5 runs, pin f86973c210228141bcb6f1f1b10188ad2a9b65f71e7cd7cf4590b38de4e80927
                                                   5/5 MATCH
tiny hostile oracle (verification/cases/track-b)    MATCH
official sample (verification/data/track-b)         MATCH  eeeec1e9319da98a
check-b-variants.sh, three data dirs               ALL MATCH (as-written, all-bitmaps, no-bitmaps)
check-consistency.py                               229 assertions, all agree
check_claims.py --strict                           0 unsourced claims
```

`check-b-variants.sh` was given the tiny oracle, the official sample and a fresh adversarial query
set from `tooling/fuzz-b-queries.py` (7,136 queries over sampled documents, deliberately
over-sampling the bitmap/list transitions). The change does not touch the classification, so this
test is not expected to move -- it is run because it is the test that would catch it if a
restructuring of the scan had quietly changed which documents get a bit.

Code lines: `1491` before, `1510` after, so the board's S moves from `1531` to about `1550`
(estimate; `scc` is not installed on this host and the repo's own fallback is what produced these).
That costs about `0.004` points on the S axis -- 25 lines for a double-digit CPU win is the right
trade on this track, where S is the smallest weight and capped at 1.0 anyway.

### 2f.6 The measurement that matters most: the step to track D is not reachable here

This track is the **dropped** one in the overall: it contributes exactly zero until it passes track
C, and the real prize is passing track D. So it is worth writing down what passing would actually
cost, from the board's own scoring formula and the board's own readings:

```
K          = (T_ref/T)^0.50 * (M_ref/M)^0.20 * (S_ref/S)^0.15 * (D_ref/D)^0.15
points     = 50 * log10(K)
T_ref 56.352376 s, M_ref 2199.9616 MB, S_ref 91 lines, D_ref 1.00

this track now (board)      T 0.4028   M 90.84   S 1531   ->  K 14.65   ->  58.29 points
after stage 4 (estimate)    T ~0.370   M 90.84   S 1550   ->  K 15.26   ->  ~59.2 points
track C's 66.81 needs                                              K 21.69
track D's 68.59 needs                                              K 23.55

the *best competing entry* on this track, for scale:
                            T 0.1738   M 47.1    S 607    ->  K 29.22   ->  ~73.1 points
```

Read the last two blocks together, because they decide the whole question. Reaching track D needs
`K 23.55`, which is **80% of the best entry's K**. From where this track stands that is a factor
`1.61` on the composite — a **2.6x cut in T alone (derived)**, or a 2.6x cut in T *and* memory down to the
best entry's, together. Nothing in 2f.2 is within an order of magnitude of that: the largest single
lever found on the hottest path was 11-20% of user CPU, and the remaining map (build 74%, phrase
verify 13%, evaluate 6%) offers perhaps another 20-30% across several coordinated changes, not 160%.

**So the honest conclusion is that this step cannot be reached from here in this session, and the
arithmetic above is the reason rather than an opinion.** Closing it would take a rewrite of the
scan rather than a change to it — the three structural differences that the best entry's own log
attributes its speed to are a mapped pointer walk instead of a per-line buffered read, a 64-byte
class-mask tokenizer instead of this track's 8-byte SWAR scan, and phrase verification folded into
the build scan so the second full read of the corpus disappears. Each is a structural change; the
third has already been priced on the board and lost (**2e**), which is itself a warning about
expecting the other two to transfer. What stage 4 does is take the one deletion the local
measurements actually support, prove it cannot change an answer, and leave the tree building.

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
| **Verifying phrases inside the build scan** | _ | **measured on the board: T 0.4776 -> 0.4996 s and 55.63 -> 55.17 points, reverted** | **locally it was -11.6% wall, and the board read +4.6% time. The deleted candidate and verification phases were real (0.171 s locally) but compute-bound, while the build scan they moved into is memory-bound on the scoring box. Full accounting in 2e. This is the reason every time number in this file is labelled local or board.** |
| **Removing the postings of phrase-only terms** | _ | **measured: only 5% fewer postings, memory level** | **kept, but not for the reason it was written: 53,444 of 75,747 terms carry postings, so the terms that only phrases name were never a large part of the index. It is strictly less work, not a win.** |
| **A raw-pointer chunk cursor for the build's unit stream** (stage 4) | the `Vec<Vec<u16>>::last_mut().unwrap()[len]` store costs ten instructions on each of 12.65 M unit writes | **measured: `+5.2%` user CPU, i.e. nothing** | **the scan is memory-latency bound, not instruction bound, so deleting instructions buys nothing — and the raw cursor costs 14 more lines for it. Measurement and the model it falsified are in 2f.2. Not committed.** |
| **A deeper prefetch ring** (24 or 32 tokens instead of 8, stage 4) | 28.7 M hits at 8 tokens of cover are not enough to hide the term-table slot load | **measured: `-2.5%` and `-2.0%`, both inside the run-to-run spread** | **the eight-token lag already covers the slot; the miss that was *not* covered is the per-term state read, which is what stage 4 actually fixed. Not committed.** |
| **Renumbering terms by class so the `bmi` array goes away** (stage 4) | one random read per token deleted on the hottest path should pay, and it is the idea with the most theoretical appeal here | **priced before attempting: the `bmi[t]` load is `0.04 s` of `4.64` user CPU seconds, inside the spread — about 1%** | **not attempted. Deleting a cache-resident read that is already overlapped is not a win, and the renumbering would have to be paid for across the query evaluator. Priced in 2f.2.** |
| **Unchecked indexing in the new per-document handle loop** (stage 4) | with the state now L2-resident, the two bounds checks per hit (28.7 M hits) should become visible | **measured: `-21.6%` at the minimum but `-15.7%` at the median against `-13.6%`/`-15.3%` for the split alone — one lucky round, not an effect** | **not committed. Reported because a minimum-only comparison would have claimed a further 8% that does not reproduce.** |

The last three were tried and reverted late in the campaign, after the PGO work above was
already in place. Each looks correct from a byte count or a cache-resident-table argument,
and each is recorded because the measurement is the interesting part.

---

## 4. Where the time actually goes

> **Historical: this section profiles the stage-2 shape**, when the track still carried per-worker
> term lists and a merged posting arena, and it is kept because it is the measurement that produced
> stage 2's deletion. **For the shape that ships, the CPU-second phase split, the component pricing
> and the reasoning are in 2f.** In particular the "49.7% build and 50.3% everything after it" line
> below is not true of the current code: measured on it, the build is 74% of the CPU.

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

**Stage 2 removes the term list and one of the two arena copies.** The peak is now
the shared posting array plus the bucket chunks the scatter has not drained yet:
**92.6 MB of working set, measured with the runs interleaved against the previous
shape** (181.6 MB before, same box, same corpus, same hashes). The board reads the
same shape at **129.6 MB** (step 2 in 1b) against 219 MB for the shape it replaced,
so the local working-set proxy reads about 29% under the cgroup counter here. One
copy remains on purpose -- a document's first occurrence of a term is buffered as a
unit until the layout knows where that term's stretch begins -- and that is what the
next stage, bitmaps over the terms holding most of the postings, is aimed at; the
measurement that prices it is in 2e.

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

Commit hash of this measurement: `_(fill with the pushed commit)`. Stage 4 was measured on the
local stage-4 commit, which the Lead verifies and then pushes; this cell is filled from that push, as
on the other tracks. Naming a hash here would be self-referential -- writing it into the file changes
the commit it names. The scratch variants those numbers came from were never committed and are
deleted -- every one is described in 2f.2 by what it deleted, so any of them can be rebuilt and
re-priced if a judge wants to.
