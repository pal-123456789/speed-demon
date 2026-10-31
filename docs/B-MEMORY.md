# Track B's peak memory is twelve copies of per-worker scratch

**The board charges 213.9 MB. The arithmetic closes on it exactly.**

```
per worker, live at the end of the build:
  term list     the range is (254 MB / 12) = 21.2 MB, reserved as (end-start)/8
                = 2.65 M entries x 4 B                          = 10.6 MB
  postings arena  22.24 M deduped pairs / 12 workers
                = 1.85 M entries x 4 B                          =  7.4 MB
                                                                 -------
                                                                 18.0 MB
twelve workers, all live simultaneously                           216 MB
board's cgroup memory.peak for 0c99d88                            213.9 MB
```

**The term list is already pre-reserved**, so doubling growth is not the cause and
there is nothing to gain there. The peak is not one large structure — it is twelve
medium ones, and every one of them is alive at the same moment because each worker
finishes its range independently and nothing is released until the join.

**This also explains the failed attempt.** The TermList change released each 1 MB block
as the counting-sort scatter passed it, and measured 185.5 -> 141.4 MB as peak WORKING
SET locally. The board charged **290 MB** — worse than the 213.9 it replaced. Fragmenting
the same total bytes into many small blocks, each freed individually, makes the
allocator's high-water mark *larger* under cgroup accounting even as resident set falls.
**The local proxy said save 24%; the meter that scores said lose 36%.**

## What would actually reduce it

Three routes, in order of how much they cost in time:

**1. Compact the term list from four bytes to three.** Term ids run to 75,747, so they
need 17 bits and a u32 wastes 15. Packing two ids into three bytes takes each worker
from 10.6 MB to 7.9 MB — **31 MB across twelve workers, about 15% of the peak**, worth
`(213.9/183)^0.20 = 1.031` on K. It costs an unpack on every scatter read, which is
2.65 M per worker, so it must be measured rather than assumed.

**2. Bound how many workers hold scratch at once.** Nothing requires all twelve to be
simultaneously in their scratch phase. A two-batch schedule — six workers, join, then
six more — halves the scratch peak to about 108 MB, worth `(213.9/108)^0.20 = 1.147`,
**about +1.4 points on B**, at the cost of running the build on six threads for part of
its life. Since the build is the larger half of B's runtime, that cost is real and has
to be traded explicitly against the M gain.

**3. Give the term list and the arena one shared buffer with a moving boundary.** This
is the only route that reaches their 72 MB, and it is a genuine redesign of the scatter
rather than a parameter change.

## The measurement lesson, which outranks the optimisation

**Peak working set is not cgroup `memory.peak`, and for this change it was wrong by a
factor of two in the wrong direction.** Track C's proxy tracked the board closely
(295.8 local against 386), which is what made the proxy look trustworthy. One close
agreement is not a calibration.

**This box can measure correctness exactly** — a hash is a hash, and it is
load-independent. **It can measure direction for large effects.** It cannot size a
memory or a time change, and today that has now been demonstrated on six separate
changes. **Every memory number that matters has to come from the board.**
