# Where the points are: per-track gap analysis

## THE ONE PATTERN THAT WORKED, BOARD-VERIFIED THREE TIMES

Every large gain this campaign has made came from **removing a per-record random
memory access**, not from tuning. All three changes improved time *and* memory
together, because the cost was never arithmetic — it was a cache miss per record
plus millions of allocator calls.

| Track | The structural change | Board result |
|---|---|---|
| **C** | shared 64-shard byte-exact interner + per-country bitmaps, replacing a per-country **per-thread** name set | peak **295.9 → 19.0 MB (15.6×)**, T 0.903 → 0.347 s, points 25.071 → **47.909** |
| **B** | flat postings arena: per worker append the term id to one flat `Vec<u32>`, count df in a 75,748-entry array, prefix-sum in place, scatter doc ids into one flat arena | **T 1.359 → 0.614 s (2.2×)**, points 43.284 → **51.369** |
| **A** | interned `u64` keys + a flat `Vec<Group>` with a compact index, replacing `Box<[u8]>` keys and a fat `HashMap` | M 64 → **47 MB**, points 60.088 → **60.740** |

**The counter-example is the instructive part.** Every *tuning* change I tried — a
hand-written eight-byte space scan, a single-level k-way merge replacing the tree
merge, six versus twelve threads — measured flat or worse and was reverted. The
exponent split (T^0.50 against M^0.20) had me optimising time for a day for +2
points; the data-layout changes took an afternoon and produced +30.

## Two latent defects, and the class they belong to

**1. A run-aborting out-of-bounds read.** Track C read its amount digits with a
guard that proved *one* byte in bounds while reading *two*:

```rust
if pos + 1 < have && recs[pos] == b'.' { pos += 1;
    let d1 = recs[pos];  let d2 = recs[pos + 1]; ... }   // indexes recs[have]
```

With a 4 MB chunk the 202 MB corpus is only about 60 buffer fills, so it passed
**every board run by luck**; at 128 KB it fired on the first attempt. **Final scoring
runs a hidden holdout seed, where a panic is not a slow run — it is zero.** The guard
is now `pos + 2 < have`, and all four tracks were audited for the same shape: only C
had it.

**2. Silent truncation.** `if want == 0 { break; }` dropped the rest of a worker's
range when a record exceeded the buffer, with no error and no visible output change.

**The rule: any fixed-size refill buffer is a place where an off-by-one guard is
invisible on the big corpus and fatal on a different one.**

## Track A at ground level: five to six memory touches per line

Read out of `take()`, which runs once per line for 3.1 M lines:

```
next_space(line, 28)              SIMD, cheap
next_space(line, s1 + 1)          SIMD, cheap
status / latency parse            arithmetic
self.hour[..] != line[..13]       13-byte compare, already guarded by a
                                  run-of-identical-hours check
self.inter.get(ep)                hash + probe the endpoint interner   touch 1-2
self.index.get(&key)              hash + probe the group index         touch 3-4
groups[gi].latencies.push(..)     Vec header, then the push target     touch 5-6
```

**The arithmetic that says this is the whole gap.** The leader runs 238 MB in
**0.019 s**, i.e. 3.1 M lines at about **18 cycles each**. Eighteen cycles is
approximately *one* memory touch, plus the parse. We spend five or six.

And this is the same shape that produced every large gain this campaign: Track B
removed a random write per token and got 2.2×; Track C removed a per-country
per-thread set and got 15.6× on memory; Track A's own interning change removed a
`Box<[u8]>` key allocation per row and took M from 64 to 47 MB.

### The change that would remove touches 3 to 6

**First, a wrong turn worth recording, because I had it half-written.** The key
packs as `(hour_ordinal << 32) | endpoint_id` with endpoint ids under 1842, so I
spent a while designing a direct-indexed table in place of the group hash map — no
hashing, one array access, and it would have fitted in L2. That design is
impossible, and `hour_ord` says why:

```rust
fn hour_ord(h: &[u8]) -> u32 {
    let d = |i: usize| h[i].wrapping_sub(b'0') as u32;
    ((y * 100 + mo) * 100 + dy) * 24 + hh
}
```

The ordinal is the **dense date-plus-hour**, not a 0..71 index. For `2026-10-30T18`
it returns **486,264,738**. Monotone — which is exactly what the sort needs — but so
sparse that indexing by it would want a table of 10^12 entries. **The hash map is
the correct structure here, and no repacking makes it otherwise.**

### So: keep the interner, drop the group index, append instead of looking up

Per line, after the single interner probe (into ~1,842 cache-resident entries),
append a fixed-width record to a flat buffer — a **sequential** write:

```
record:  u64 key (hour_ordinal << 32 | endpoint_id)  +  u32 latency   = 12 bytes
       or  u32 packed (ordinal_rank << 11 | endpoint_id) + u32 latency =  8 bytes
```

12 bytes × 3,100,000 lines = **37 MB total across twelve workers, about 3 MB each**
— L2/L3 resident, and in the same range as the ~20 MB the current group tables and
latency vectors occupy. Each worker then sorts its own ~258 k records by key and
runs the existing order-statistic selection over each run of equal keys. Sorting
258 k `u64`s per worker is a few milliseconds, spread over twelve cores, and an
integer comparison is far cheaper than a hash probe plus a scattered `Vec` access.

If the 8-byte form is used, the ordinal must first be **ranked** (dense 0..k, in
first-seen order) so it fits in 21 bits; the ranking already exists for hours in
`Sink::hours` and is monotone, so the packed order still equals byte order.

**Not implemented.** It is an ~80-line restructuring of `take`, `scan`, `merge` and
the per-worker sort, and it needs a measured before/after rather than a projection.
What is measured is the touch count; what is proposed is a hypothesis.

## Track D: the distance array is sized for the graph, not for the search

Measured, not inferred. A workstream instrumented the solve directly:

```
1 worker   = 7.1 s
8 workers  = 2.05 s        -> only 3.5x scaling
~12,500 nodes settled per query
answers are 5-9 hops
```

**Two conclusions follow, and both invalidate something I had written down.**

**1. The solve is DRAM-latency-bound and does not scale.** 3.5× from eight workers
means the bottleneck is memory latency, not arithmetic — which is the same finding
as every other track, arriving from a different direction.

**2. Each worker allocates a 13.9 MB distance array over all 1,740,512 nodes in order
to touch about 12,500 of them.** Eight workers is **111 MB** — the dominant term in
D's 183 MB peak — for a working set of roughly 200 KB per thread. The array is sized
for the graph rather than for the search, and the search is two orders of magnitude
smaller.

**This also corrects my own arithmetic on the bidirectional search.** I costed it at
+1.4% and dismissed it, on the assumption that it halves the solve. It does not
halve it: going from ~12,500 settled nodes to ~200 is a different order of
magnitude, because unidirectional search settles about `b^r` nodes for branching
factor `b` and radius `r`, while a bidirectional search settles about `2 * b^(r/2)`.
For short paths in a road network that is a multiple, not a factor of two. **My
+1.4% was wrong because the 2x was wrong.**

**And then it was implemented and measured, and it is WORSE.** Forward plus reverse CSR,
the mu/meeting rule, hash-verified against `a23dbdf0…` on the whole corpus — correct,
and rejected on the stopwatch:

```
relaxations        67.5 M  ->  41.6 M       only 1.6x fewer, because on this graph
                                            ball(2r)/ball(r) ~ 3.2, so the square-root
                                            argument buys 1.6x and not an order
solve              2242 ms ->  3406 ms      1.52x SLOWER
label footprint     111 MB ->   222 MB      the second frontier needs a second array
```

The per-relaxation `mu` probe into the second label array costs more than the
relaxations it removes. **So the bidirectional branch is closed by measurement, not
by argument** — and note the direction of my two errors: I first dismissed it at
+1.4% on a guessed 2×, then talked myself into "several times that" on a `b^r`
versus `b^(r/2)` argument that assumed a branching factor this graph does not have.
Both were arithmetic on an unmeasured constant. The measurement is 1.52× slower.

**What the same instrumentation did find, and it is cheap.** Two of the 2,000 queries
(indices 781 and 1914) are `-1` answers with **in-degree(t) = 0**, and between them
they account for essentially the whole chunk imbalance: 5 M relaxations each. A
precomputed in-degree column makes those two an O(1) answer, and they are ~15% of all
relaxations in the run. Separately, a static chunk split gives every worker the same
*number* of queries but not the same *cost*, and these two prove it — dynamic
scheduling, or simply answering the zero-in-degree case before entering the search,
addresses the same imbalance from two sides.

## Memory on B and D, decomposed against #1

The overall is the geometric mean of the best **three** tracks, so the leverage sits
on the **weakest counting track**, not on the best one. As of the current board that
is **B at 51.454**; C is at 54.977 and D at 48.494 is dropped.

**Track B — K 10.693 against #1's 29.934, a gap of 2.80×**

| axis | ours | #1 | ratio | K contribution | share of the gap |
|---|---|---|---|---|---|
| T | 0.612 s | 0.160 s | 3.83× | 1.956 | **64.5%** |
| **M** | **232 MB** | **42 MB** | **5.52×** | **1.408** | **32.9%** |
| S | 889 | 745 | 1.19× | 1.027 | 2.6% |

**Track D — K 9.330 against #1's 28.833, a gap of 3.09×**

| axis | ours | #1 | ratio | K contribution | share of the gap |
|---|---|---|---|---|---|
| T | 0.846 s | 0.122 s | 6.93× | 2.633 | **66.5%** |
| **M** | **178 MB** | **68 MB** | **2.62×** | **1.212** | **21.1%** |
| S | 354 | 441 | **0.80×** | 0.968 | **-3.6%, we win** |

**Three things fall out.**

**1. Track D's line count already beats the leader's** — 354 against 441. It is the
only axis anywhere in the submission where we are ahead of #1, and it is worth
noting because it means D's remaining gap is *purely* time and memory, with nothing
to claw back on S.

**2. Track B's memory is the larger relative defect — 5.52× against D's 2.62× — and
it is worth 33% of B's gap.** Where B's 232 MB goes, from the workstream's own
measurements:

```
build phase:  89 MB  term-id list (22.24 M deduped pairs as u32)
           +  85 MB  raw postings arena            = 174 MB   <- this is the peak
merge phase:  85 MB  worker parts
           +  85 MB  final arena                   = 170 MB
```

**The peak is the build phase, and that is what rules out the obvious fix.** Delta
plus varint compression can only be applied *after* the counting-sort scatter,
because the byte offsets are not known during it — so it cannot touch the 174 MB
build peak at all, while adding a decode cost to every access and breaking the O(1)
indexing `gallop` depends on. That is why it was refused.

**3. The fix that would work on B is chunking the build, not compressing it.** Build
the postings for a slice of the document range at a time — term list and arena both
shrink by the slice fraction — and merge each slice into an accumulating postings
structure. At four slices the build peak becomes `22 + 21 = 43 MB` against an
accumulator that grows to 85 MB, i.e. **128 MB instead of 174 MB, about -26%**, and
the merge is a concatenation that is already cheap. That is worth roughly
`(232/172)^0.20 = 1.06` on B — about +1 point — and it is the only route to B's
memory that does not pay in time.

**And on D, the distance arrays are the whole story and the objection to shrinking
them is now gone.** Eight workers each hold a 13.9 MB dense array over all 1,740,512
nodes in order to touch about **12,500** of them: **111 MB** of D's 178 MB. The
workstream costed a hash-table label store and dropped it *for the right reason* —
13,000 random labels occupy ~13,000 cache lines whether they sit in a dense array or
a hash table, so it buys no time. **But it was being judged as a time change, and on
that axis it is neutral.** On the memory axis, which is 21% of D's gap, it turns
111 MB into a few hundred kilobytes per worker, taking D's 178 MB toward the
leader's 68 MB and worth `(178/68)^0.20 = 1.21`, about **+4 points on D**.

D is still the dropped track, so that only reaches the overall if it passes B's
51.454 — which at 52.7 it would, by a small margin. Worth doing, but the larger and
more certain lever remains B, because B is the track that is actually counting.

## Ours versus the leaders

Read off the live board. The useful thing is not the gap in points but the gap in
**axes**, because each axis carries a different exponent.

| Track | Axis | Leader | Ours | Gap | Exponent |
|---|---|---|---|---|---|
| **A** | T | 0.019 s | 0.123 s | **6.5×** | 0.50 |
| | M | 18 MB | 47 MB | 2.6× | 0.20 |
| **B** | T | 0.162 s | 0.614 s | **3.8×** | 0.50 |
| | M | 41 MB | 234 MB | **5.7×** | 0.20 |
| **C** | T | 0.035 s | 0.346 s | **9.9×** | 0.50 |
| | M | 13 MB | 18 MB | 1.4× | 0.20 |
| **D** | T | 0.122 s | 1.060 s | **8.7×** | 0.50 |
| | M | 68 MB | 183 MB | 2.7× | 0.20 |

## What the table actually says

**1. Line counts are not the problem.** A: 412 code lines against 382. B: 889 against
1,198 — we are *smaller* than the leader on B and still 3.8× slower. So S is not what
separates us, and cutting lines would not close any of this. Every remaining gap is T
or M.


**2. Track C's memory is a 29.7× defect and it is ours.** The leader holds 13 MB
while reading a 202 MB CSV and emitting 240 aggregate lines. That number is the
proof that **`/data` is not charged as page cache** — I had assumed a ~202 MB
floor and designed around it, which was wrong. With no page-cache floor, our
386 MB decomposes as roughly:

```
  12 workers x 4 MiB read buffers      48 MB
  12 x per-country name sets          ~135 MB   <- the real defect
  region lookup + output + slack      ~200 MB
```

Twelve threads each keep their **own** set of distinct names per country, so the
corpus's ~94,000 distinct names are stored twelve times over. That is the single
worst number in the submission and it has one cause.

**3. Our per-line work is higher than the leader's on Track A.** Same line count,
3.9× the time. An earlier version of this section claimed three hash operations
per line; **reading the code shows that is wrong and the correction matters**,
because it points at a different fix. Track A already uses a multiply-xor
(FxHash-shaped) hasher and a SINGLE map:

```rust
type Map = HashMap<Box<[u8]>, Group, BuildHasherDefault<Fx>>;
```

So there is one hash per line, not three. What that line still does is hash a
**composite byte key of roughly 13 + 30 bytes** (the hour concatenated with the
endpoint) and it must materialise those bytes before it can look anything up.

The two candidate causes, which a profiler should separate before either is
pursued:

* **Key width.** Hashing ~43 bytes per line where an 8-byte packed
  `(hour slot, endpoint id)` would do. The hour needs no hashing at all —
  `YYYY-MM-DDTHH` is 13 fixed-width bytes, so its index is arithmetic on digits.
* **Per-line byte scanning.** Finding the three field boundaries by walking
  bytes, roughly 60 comparisons for a 66-byte line.

The arithmetic that says something is wrong: the leader does 238 MB in 0.032 s
across 12 threads, about **5 cycles per byte**, and we do about **19**. Both
figures are estimates from wall time, but a 4× per-byte difference on the same
line count is not tuning — it is a different amount of work per line.

**4. Track B is the largest single gap: 11.9× on time.** Our build is 4.5–7.3 s
of a ~5 s run, bisected to tokenise 0.51 s, +hash lookup 2.67 s, +dedupe/push
4.95 s, over 36.28 M tokens of which 28.7 M hit a needed term. The leader does the
whole index and all 50,000 queries in 0.332 s — a third of a second for 254 MB.
That is not a faster version of our design; it is a different design. Find it
before optimising ours.

## What is NOT worth doing

* **Rewriting the byte scanners.** Track A's `next_space` was a byte-at-a-time
  loop sitting right next to `nl`, which is eight-at-a-time, so I rewrote it to
  match. Correctness was never in doubt (5/5 on the published hash), and the
  interleaved A/B rejected it: **0.3550 s vs 0.3645 s, 3% slower.** The reason
  redirects the search — `buf[i..].iter().position(|&c| c == b' ')` **already
  compiles to SIMD.** rustc vectorises that pattern into a 16- or 32-byte scan, so
  the source only *looks* byte-at-a-time and my hand-written 8-byte loop processed
  fewer bytes per iteration than the compiler's. The idiom appears in all four
  tracks, so none of the per-byte gaps are in scanning.

* **Replacing Track A's tree merge with a single-level k-way merge.** The tree
  merge folds twelve lists in ~4 levels and re-copies each group's latencies at
  every level, which looked like ~180 MB of memcpy for 50 MB of data. Rewritten,
  correctness held (5/5), and the A/B was flat: **0.4955 s vs 0.4975 s.** The
  copied-bytes theory was wrong, or the cost is far below the measurement floor.
  The k-way version is also ~24 lines longer, so it is strictly worse and was
  reverted. The "45 ms merge" figure that motivated it came from a single loaded
  run and should not be trusted until it is measured on a quiet box.

* **Cutting lines.** B is already smaller than the leader's B (723 vs 790) and
  11.9× slower, so S is not what separates us.
* **Micro-tuning C.** C is dropped from the best three, so its score does not
  reach the Overall until it passes B. Its **memory** is still worth fixing on
  engineering grounds, but not for points.
* **Trading memory for time by hand**, e.g. Track D at 8 workers instead of 12:
  measured, reverted, and the sign of the local measurement was opposite to the
  board's.
* **Chasing local timings.** Six changes have now measured differently locally
  and on the board, and in both directions. Local numbers are for direction and
  for the hash; the hash is exact either way.

## The order I would work in

1. **B's index design.** 11.9× on the axis with the largest exponent, on a track
   that counts. Everything else is secondary.
2. **A's three hashes per line → one.** 3.9× available with no line-count cost.
3. **C's duplicated name sets.** 29.7× on memory; the largest defect, on the one
   track that does not currently count.
4. **D's 6.9×.** A radix heap over `BinaryHeap` is the classic 1.5–2× here.
