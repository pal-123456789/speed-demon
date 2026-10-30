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
into one of 75,747 separate `Vec<u32>` headers is a third, a fourth and sometimes
a fifth when the `Vec` reallocates.

48× is not achievable by shaving cycles off a per-token random access. **It
requires not doing a random access per token at all.**

### The shape that fits the budget

Append, then sort. Per token, append a fixed-width record to a flat buffer —
sequential writes, one or two cycles, no probe, no per-term `Vec`:

```
records: 28.7 M (term id, doc id) pairs, packed into u64  =  230 MB
append:  ~2 cycles per token                              =  ~25 ms
radix:   8 passes of 8 bits, each a sequential stream      = ~370 ms of traffic
```

Every byte of that is sequential, which is also why it would scale where the
current design cannot. Compare the cost of the thing being replaced: 36.28 M
independent ~200-cycle stalls is on the order of seconds.

**The trade, priced on the real formula.** Memory rises from 132 MB to roughly
230 MB plus the query side, so the M factor falls:

```
M:  (2199.96/132)^0.2 = 1.76   ->   (2199.96/260)^0.2 = 1.53      = -13%
T:  (3.938/1.0)^0.5   = 1.00   ->   (3.938/1.2)^0.5   = 2.09      = +109%
K_B: 4.863 -> ~11.2      PTS 34.35 -> 52.5                        = +18 points
```

Overall becomes `(60.08 × 52.5 × 42.34)^(1/3) = 51.1`, against 44.375 now.

**That single change is worth more than everything else on this list combined**,
and it is the only one that closes a 48× gap.

## Track C — the peak is per-thread parse state, and it is released

```
11.6 -> 62 -> 107 -> 253 -> 267.9 (mid-run peak) -> 96 -> 12.2 MB
peak 295.8 MB locally      (the board charges 386 MB)
```

Memory **rises through the parallel parse and falls the moment the workers join**.
So the peak is entirely per-thread parse state, and the dominant term is that each
of twelve workers keeps its **own** set of distinct names per country: the
corpus's ~105,000 distinct names stored twelve times.

That is the diagnosis confirmed by measurement rather than inferred, and it is
why the leader's 13 MB is reachable in principle.

### What the fix has to be, and why it does not pay yet

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
