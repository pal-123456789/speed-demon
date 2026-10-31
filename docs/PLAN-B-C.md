# Track B and Track C: measured, and what the fix has to be

Both tracks were profiled rather than guessed at. These are the numbers.

## Track B — the index build is 79% of the run

```
[t] parse queries       0.040s
[t] build index         7.152s      <- 79% of everything
[t] phrase candidates   7.366s
[t] phrase verify       8.230s
[t] write               9.026s
peak working set        138.1 MB    (the board charges 132 MB)
```

Everything except the build — phrase candidates, phrase verification, answer
assembly and write — is **1.9 s combined**. Optimising any of it is worth at most
21%. The build is the track.

The leader answers everything, index and all 50,000 queries, in **0.329 s** for
254 MB with 40 MB. Their build must be on the order of 0.15 s, so on the build
alone we are roughly **48×** behind.

### Why it cannot be a tuning problem

Our own bisect of the build: tokenise 0.51 s, +hash lookup 2.67 s, +dedupe/push
4.95 s, over 36.28 M tokens. That is

```
2.16 s / 36.28 M lookups  =  60 ns  = ~180 cycles per lookup
2.28 s / 28.7 M  pushes   =  79 ns  = ~240 cycles per push
```

**Those are memory-latency numbers, not arithmetic numbers.** A hash probe into a
75,747-slot table plus a term-arena compare is two cache-line touches, and a push
into one of 75,747 separate `Vec<u32>` headers is a third.

### But that is NOT the main story — and an earlier version of this section was wrong

An earlier revision of this file concluded that the 48× gap required abandoning
the hash probe for a flat-append plus radix sort. **That diagnosis was wrong, and
one grep overturns it: Track B contains no threads at all.** No
`thread::scope`, no `spawn`, no `available_parallelism`. It is one pass over
254 MB on one core, while Track A uses twelve threads, Track C twelve and Track D
eight.

The arithmetic is unambiguous:

```
ours  : 254 MB in 3.938 s  =  64.5 MB/s   on ONE core
leader: 254 MB in 0.329 s  = 772  MB/s   772 / 64.5 = 11.97
```

**11.97 is twelve.** The leader is not doing radically less work per byte; it is
doing the same work on about twelve cores. The gap is the parallelism we never
added to this track.

### So the fix is the one already applied twice

Parallelise `build_index` exactly the way Track A and Track C were parallelised:

* `docs.txt` is `id TAB text LF` with no quoting, so it splits at **newline
  boundaries** — far simpler than Track C's quote-parity proof.
* Each worker takes a byte range, its own `postings: Vec<Vec<u32>>` and its own
  `stamp` array (75,747 `u32`, 303 KB — cheap to duplicate).
* Merge is a **concatenation per term**, in worker order. Because each worker's
  posting list is already doc-ascending and the ranges are increasing, the
  concatenation comes out globally ascending with no sort and no dedupe.

Memory cost is only the duplicated `Vec` headers: 8 × 75,747 × 24 B = 14 MB.

Priced, with the build at 7.15 s of a 9.03 s run and 79% of it parallel:
`9.03 → ~2.8 s`, so the board's 3.938 s should land near **1.25 s**.

```
T factor: (3.938/1.25)^0.5 = 1.77   ->  K_B 4.863 -> 8.6  ->  PTS 34.35 -> 46.5
overall : (60.08 x 46.5 x 42.34)^(1/3) = 49.1   against 44.375 now
```

The 50,000 queries are the remaining 1.9 s and are embarrassingly parallel too, so
there is more behind that.

**This is a ~60-line change using a pattern already proven twice in this repository,
not a radix-sort rewrite.** The lesson is worth recording: I profiled the loop,
found real cache misses, and then explained a 12× gap with them — when the simpler
explanation was that we had simply never used more than one core.

## Track C — the peak is per-thread parse state, and it is released

> **RESOLVED, and the diagnosis below turned out to be WRONG in an instructive way.**
> This section is kept as written because the reasoning is the point.
>
> Implemented and shipped. The board now reports **M 16.4 MB** against the leader's
> 12 MB, down from **386 MB** — a factor of 23.5 — with T 0.903 -> 0.1816 s, and
> the track went **25.071 -> 54.977 points**, moving from dropped to counting.
>
> **But not for the reason given here.** The paragraph below says "the corpus's
> ~105,000 distinct names stored twelve times", and that is the number I briefed a
> workstream with. It measured the claim instead of accepting it:
>
> ```
> 3,260,049 per-thread (country, name) entries
> vs 2,980,424 distinct (country, name) pairs globally
> = 1.09x duplication
> ```
>
> **There was nothing to union away.** The sets were not twelve copies of one
> another — each `(country, thread)` set held about 1,130 *almost-unique* names,
> because 3,320,000 records spread flat over 240 countries and 12 threads gives
> every set its own names. The whole win came from the global name pool being
> 100,520 and from de-duplicating against it, not from removing cross-thread
> duplication. **A shared interner plus per-country bitmaps, not a merge.**
>
> The lesson is in the numbers: I had "12x duplication" and "105,000 names" both
> written down, and neither was the operative fact.

```
11.6 -> 62 -> 107 -> 253 -> 267.9 (mid-run peak) -> 96 -> 12.2 MB
peak 295.8 MB locally      (the board charged 386 MB at the time)
```

Memory **rises through the parallel parse and falls the moment the workers join**.
So the peak is entirely per-thread parse state, and the dominant term is that each
of twelve workers keeps its **own** set of distinct names per country: the
corpus's ~105,000 distinct names stored twelve times.

That is the diagnosis confirmed by measurement rather than inferred, and it is
why the leader's 13 MB is reachable in principle.

### What the fix has to be, and why it does not pay yet

> The reasoning below about *why* it would not pay was also overtaken. It was
> correct that de-duplicating across threads was capped at ~9%, and wrong that this
> made the memory unreachable — the reachable win was the shared pool, not the
> duplication, and it paid 23.5x.

The duplication is structural to splitting the file by byte range: each worker
must dedupe within its own range, and the union happens at merge. Removing it
means either a shared interning arena (needs synchronisation on the hot path) or
storing something smaller than the name bytes per entry.

**But Track C is dropped from our score.** It contributes nothing until it passes
Track B's 34.347, which needs +37% on C. A memory fix alone is worth about +12%.
So C's defect is worth fixing **on engineering grounds** — a 29× memory gap is not
a defensible thing to leave in a submission — but it will not move the number
until C also gets faster.

**Order therefore matters: B first, then C, and C only if its time comes down far
enough to overtake B.**

## Order of work

1. **B's build: append + radix sort.** Worth ~+18 points and it is the only
   change that addresses a 48× gap. Everything else is second.
2. **B's remaining 1.9 s** only after the build is fixed.
3. **C's duplicated name sets** — real defect, no points until C overtakes B.
4. **C's time**, if the memory fix lands and C is within reach of B.
