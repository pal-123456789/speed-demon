# Track D: the fused-row design, and what it is worth

Read end to end from a 72-point D entry's own research note and source. Recorded as technique and
measurements, with nothing copied.

## Their own cost model, per thread, single-threaded

```
parse 120   raw CSR 40   degree-2 bypass 88   BFS grow 54   rest 19
gather 70   reverse 79   queries 300
```

Construction is ~61% of all work; queries are the single largest phase at 300. Their search is
**5.38 M settles** on the contracted graph, and **66% of query time is in the 170 queries that
settle 10k-50k vertices** -- so query cost is concentrated, not uniform.

## The two structural ideas

### 1. A fixed 4-slot row per vertex

`slot[v * 4 + k]` holds an edge as `head << wbits | weight` -- head in the HIGH bits, weight in the
LOW. A vertex with degree <= 3 keeps all its edges inline plus a terminator; degree 4 fits exactly;
above that the fourth slot stores a **spill code** (`degree - 3`) and the remaining edges live in a
list handed out in chunks.

Why it matters: **expanding a vertex becomes ONE cache line touch.** Our layout is
`off[u]..off[u+1]` indexing into `adj[]`, which is two dependent random reads per vertex -- the
offset array, then the adjacency array. Four u32 is 16 bytes, so four vertices share a 64-byte
line and the row is in the same line as its neighbours' rows. This is cheap for low degree, which
is what a road-like graph has.

### 2. A monotone bucket queue with a `take`, so relaxation is branch-free

A ring of `2^shift` buckets; a jump larger than the ring width waits in a `take` slot rather than
branching away, so **the relaxation path has no data-dependent branch**. Settling is FIFO within a
bucket, and the next entry's row and label are prefetched before its expansion.

Their measured effect on the query engine, thread-ms:

```
branch-free relaxation                     294 -> 244   (-17%)
FIFO bucket drain + row/label prefetch     268 -> 230   (-14%)
4-slot search rows                         -3%
```

## Construction wins, each measured

```
SIMD separator parsing                     120 -> ~97  thread-ms
atomic-free transpose (reverse graph)       79 -> ~57
separate scatter pass for raw->region      grow 120 -> 104
false-sharing padding of per-worker
  counters (Region and Binner adjacent)    grow  58 ->  45  at 4 workers
private first-check bitmap                 grow  65 ->  55  at 4 workers
plain-store claims with duplicate
  tolerance instead of lock cmpxchg        removed serialisation on pending random stores
```

Two of those apply directly to us. **Our reverse-graph build uses `AtomicU32::fetch_add` per edge
to count in-degrees** -- their "atomic-free transpose" is 79 -> 57, a 28% cut of that phase. And
**our blob-claim uses `compare_exchange` per node**; their "plain-store claims with duplicate
tolerance" removed a serialisation that we are plausibly paying for now.

## What they tried and rejected, so we do not repeat it

```
merged forward conversion + reverse count      no time gain, peak 62 -> 85 MB
byte-sized region degrees                      grow +5 ms
prefetching region buffers                     no gain
interleaved forward/backward label pages       +11%: the two searches cover disjoint
                                               regions, doubling the footprint while the other
                                               side's lookups hit the shared blank page
processing stale entries without the check     16% of pops are stale, +6%
prefetching neighbours' labels                 +18%
bucket width 2^6 / 2^8, label pages 2^6/2^8    no gain after the redesign
ALT landmarks (2 landmarks = 4 SSSPs)          ~220 thread-ms to save less than that
CH / witness searches beyond degree 2          prep exceeds the query phase it would shorten
degree-3 elimination on an independent set     -10-15% query for +40-60 thread-ms prep
```

Their axis readings: cgroup peak 62 MB for the fused build against 80 MB for their champion, and
**S ~775 by a clang-format non-blank non-comment count**. Our S is 536 and our M is 70.57 MB, so
**memory and source already favour us; the gap is time.**

## What to implement on ours, in value order

1. **Atomic-free transpose for the reverse graph.** Our `bump()` is an `AtomicU32::fetch_add` per
   edge over 5 M edges; theirs is 79 -> 57 thread-ms. Counting per-thread into private arrays and
   summing them removes 5 M atomics and is exactly the "deletion" kind of change that has worked
   on every track. Measure memory: private arrays are `nt x n x 4` bytes.
2. **False-sharing padding of the per-thread counters**, if any two per-worker arrays are adjacent
   in a `Vec` -- theirs measured 58 -> 45 ms from padding alone.
3. **Branch-free relaxation in the search loop** (-17% of query time).
4. **FIFO bucket drain with row and label prefetch before expansion** (-14%).
5. **4-slot search rows** (-3%), then the 4-slot adjacency if 1-4 pay.

Do 1 and 2 first: they are small, they are deletions of atomics, and they need no new data
structure. Do 3 and 4 next, as a pair, on the search loop. Leave the full 4-slot adjacency until
the smaller ones have been measured, because it is a rewrite of the CSR and everything that reads
it.

## The rule this file exists to enforce

Every number above is somebody else's measurement on somebody else's hardware. **Ours must be
taken here**: build the Linux ELF under WSL with the corpus on ext4, read anon-only RSS (never
total RSS -- the mapped input is charged to us and not to the board), take the smallest of several
runs, and treat the result as a ratio. See docs/MEASURING-LOCALLY.md. A technique that measured
-28% for them can measure 0 for us; that has now happened four times this session and every one
was caught by measuring first.