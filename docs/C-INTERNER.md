# Track C: the interner, not the parser

**I had the plan wrong for hours and the phase data corrects it.** The measured split of
Track C's total runtime, from its own TSC instrumentation on the least-preempted of eight
runs:

```
intern              53.9%     <- the hash table, once per record
thread scope        11.8%     (main blocking on join)
record_boundaries    9.2%     (single-threaded pre-pass)
rest of loop         8.8%
open+read            6.2%
field(note)          4.0%
field(name)          2.3%     \  everything a SIMD field parser could touch
digits+amount        2.2%     /  = 8.5% of the run
setup(regions)       1.5%
merge+output         0.2%
```

**I had been calling the SIMD field parse C's "+5 to +9 prize". It is 8.5% of the
runtime. Even a free parser buys 8%.**

## Where the time actually is

```
packed interner   49.5 ns per intern
hash-only floor    6.7 ns per intern
                  ----
                  7.4x of headroom, on 53.9% of the track
```

**If the interner reached the hash-only floor, C's time goes 0.161 -> 0.086 s.** That is
`(0.161/0.086)^0.5 = 1.368` on the T factor, C 56.63 -> about 63.4, worth **about +2.2 on
the overall.** That is where the effort belongs, and it is an order of magnitude larger
than the parser work I had written into the plan.

## Why it is slow, and what the fix is

Our slot is **one `u64`** packing `{occupied, id, len, arena_offset}`. A hit costs a
probe into the slot array, then a **second dependent load** into the byte arena to
compare the name. Two cache lines, the second dependent on the first.

The structure that removes that second line:

```
tags_    u16[1 << 17]      256 KB   cache-resident; the probe chain touches ONLY this
slots_   Slot[1 << 17], 32 B each, with 24 name BYTES STORED INLINE
arena_   overflow only, for names longer than 24 bytes
```

A probe walks the tag array alone, then prefetches the fat slot it predicted. The
comparison is a 32-byte vector `movemask` against the **inline** head, so the common case
**never touches the arena at all**. One cache line per intern instead of two.

**The cost to check before adopting: 24 inline bytes at 128 K slots is about 4 MB of slot
array, against today's 2.2 MB.** Track C is footprint-sensitive — the workstream measured
that `SHARDS` 256 costs +2.9 MB and is a net loss — so **this must be measured on the
board, not argued.** The prediction is roughly `49.5 -> 20-25 ns`, which is 25-35% off
C's total, but the same workstream's own records show two changes that looked certain and
measured as losses.

## What this changes

**Order for Track C:** the interner first (53.9%), then `record_boundaries` (9.2%, and it
is single-threaded — a parallelisable count is the second target), and the SIMD field
parse last (8.5%, and only because it is already written up).

**And for Track A the same phase data says the opposite, which is why both were measured:**
A's scan is **67%** of its runtime, so **there the SIMD parse is the right target.** The
two tracks need different work and one plan cannot cover both.

**The general lesson, which has now cost several hours twice:** read the phase split
before choosing the optimisation. The exponent weights tell you what a *point* is worth;
the phase split tells you what a *change* can buy. I had been reasoning from the first
and guessing at the second.
