# Head-to-head: 56.677 against 72.222, decomposed

Their overall is **72.222**; ours is **56.677**. The product must rise **1.274x**.
Two teams, four tracks, same tasks:

| | A pts | A T | A M | A S | B pts | B T | B M | B S | C pts | C T | C M | C S | D pts | D T | D M | D S |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **them** | 79.985 | 0.033 | 20 | 265 | 64.944 | 0.290 | 72 | 802 | 72.522 | 0.047 | 13 | 370 | 58.727 | 0.325 | 161 | 425 |
| **us** | 62.216 | 0.111 | 41 | 434 | 52.442 | 0.551 | 224 | 973 | 55.800 | 0.175 | 15 | 685 | 48.604 | 0.837 | 178 | 354 |

## The gap per track, split into the three axes that produce it

| Track | K ratio | from T | from M | from S |
|---|---|---|---|---|
| **A** | **2.27x** | 1.834 (**81%**) | 1.154 (19%) | 1.077 (**8%**) |
| **B** | **1.78x** | 1.378 (**59%**) | 1.255 (**41%**) | 1.029 (3%) |
| **C** | **2.16x** | 1.930 (**87%**) | 1.029 (3%) | 1.096 (**10%**) |

**The single most important fact in this table: B is only 1.78x off, and B is our
WEAKEST COUNTING TRACK.** The overall is the geometric mean of the best three, so the
weakest track is where a fixed amount of work returns the most, and B is both the
weakest *and* the closest.

## What closing each track is actually worth

```
close B entirely   52.442 -> 64.944   overall 56.677 -> 60.85   (+4.2)
close C entirely   55.800 -> 72.522   overall 56.677 -> 61.88   (+5.2)
close A entirely   62.216 -> 79.985   overall 56.677 -> 61.63   (+4.9)
close all three                        overall          -> 72.22  (their score)
```

**We do not need to beat them on any single track. We need all three closer at once**,
because a fully-closed track is only +4 to +5 on the overall and reaching 72.222 needs
all three of them closed simultaneously.

## Where the work actually goes, in order of return

**1. B's memory — and it is already pushed, waiting on the board.** B sits at a
commit whose peak is 224 MB; the workstream has since measured **185.5 -> 141.4 MB
(-23.8%)** with CPU neutral, verified on all three corpora. On the formula:

```
B's M factor at 224 MB   = (2199.96/224)^0.20 = 1.508
B's M factor at 141 MB   = (2199.96/141)^0.20 = 1.616
                            -> K x 1.071
B's remaining M gap vs their 72 MB:  1.255 -> 1.163
```

**That takes B's total gap from 1.78x to 1.65x, and B from 52.442 to about 63.3 —
roughly +3.5 on the overall, from work that has already shipped.**

**2. B's time — 1.378x, and it is the smallest time gap anywhere in this comparison.**
Their B is 0.290 s against our 0.551 s. This is the only track where the time gap is
under 4x, and it is on the track with the most leverage.

**3. C's source size — 685 lines against their 370.** That is `(685/370)^0.15 = 1.096`,
**10% of C's entire gap**, from refactoring with the hash as the safety net. Our 685 is
more than three times the 200-line reference and nearly double theirs.

**4. A's memory — 41 MB against their 20.** `1.154`, 19% of A's gap. The peak is the
tree merge: two levels of `Group` coexist (~14 MB) and a split group's `Vec` is
extended with another worker's, so both copies live (~25 MB). A single-level k-way
merge removes the levels — and it was already written once and **rejected on TIME**
(flat, 0.4955 vs 0.4975 s) when its real benefit was always memory.

**5. The time gaps on A and C — 1.83x and 1.93x.** These are the largest terms and the
hardest. The concrete route is recorded in `docs/PARSER-TECHNIQUES.md`: a bitmask field
walk with hardware quote-parity for C, a 64-byte probe for A.

## One thing we already have over them

**Track D: ours is 354 lines against their 425, and 0.837 s against their 0.325 s.**
D is dropped for both of us, so it does not score — but it is the one axis where we are
ahead, and it is worth knowing that our dropped track is smaller than theirs.

## The honest arithmetic

**Reaching 72.222 needs all three counting tracks closed at once.** B's memory gets
roughly a third of B's M gap. C's lines get 10% of C's gap. A's merge gets 19% of A's.
**Those three together are worth about +6, to roughly 63.** The remaining difference is
the 1.8-to-1.9x time gap on A and C, which is real work that has not been done yet.

**And the asymmetry worth stating:** all three of our near-misses today — the Track C
out-of-bounds read, the Track B line-start cut, and the packed-record flag — are the
kind of thing that passes every board run and fails the holdout. Their A hardcodes
`HOURS = 72` and a date range with no fallback. **A wrong answer on the holdout is
zero, not a few points, so the record of never having shipped one is worth more than
any single track's points — and it is the one asset that cannot be recovered once lost.**
