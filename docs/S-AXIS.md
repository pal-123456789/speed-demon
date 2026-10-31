# The S axis is the cheapest +3.82 available

Everything in this campaign has been chasing time. The source-size axis has been
sitting there, is worth **+3.82 on the overall**, and needs no algorithmic change, no
`unsafe`, and no new data structure — only the hash to prove nothing moved.

## The arithmetic

```
A: 434 -> 265 lines    (434/265)^0.15 = 1.0768
B: 973 -> 802 lines    (973/802)^0.15 = 1.0294
C: 685 -> 370 lines    (685/370)^0.15 = 1.0968

combined on the overall:  1.0673
56.725 -> 60.54        (+3.82)
```

Their line counts are the targets in the rightmost column. **C is the largest single
lever in the whole submission at 1.0968** — we have 685 code lines where they have 370,
against a 200-line reference.

## Where our lines are

```
track-a:   413 code   151 comment   36 blank   = 600 raw
track-c:   658 code   253 comment   61 blank   = 972 raw
```

**Comments are free.** `S` counts code lines, and both tracks carry a large amount of
prose that costs nothing. So the work is strictly about code lines — which means it is
refactoring, not documentation, and it cannot be won by deleting explanations.

Note also that `Dockerfile` (15), `build.sh` (21) and `.dockerignore` (25) all live in
the track folder and appear to be in scope: for Track A, 413 code + 15 + 21 already
exceeds the board's 434, so the board's counter applies its own exclusions and a naive
line count is not the right target. **Measure against the board, not against a local
count.**

## Why this is the safest work on the board

Every other item on the list changes behaviour and must be argued for. **This one is
provably behaviour-preserving:** the output hash either still matches on all three
corpora or it does not, and a refactor that changes it is caught immediately and
unambiguously. There is no measurement noise in the gate, no load sensitivity, and no
"it was faster on Tuesday" — the correctness check is exact.

## What actually removes lines

**1. Collapse duplicated blocks into one helper.** Track C's workstream already did this
once and took 698 -> 658 lines with no behaviour change, by deduplicating two identical
output blocks into one `line()` and adding a `comma()` helper. That one pass found 40
lines. At 658 remaining there is more of the same shape.

**2. Fold single-use `impl` blocks and wrappers into their call sites.** Free functions
that exist only to be called once from one place cost a `fn` line, a signature, a
closing brace and often a doc comment's worth of blank lines around them.

**3. Replace hand-rolled helpers with std where the cost is zero.** The integer writers
(`write`, `write_u64`, `write_i64`, `finish`) are about 25 code lines across the tracks;
where the hot path permits `write!` the helpers can go, and where it does not, the
helpers stay and the win is elsewhere.

**4. Merge the small `struct` + `impl` pairs** that carry one field and one constructor.

**5. Re-derive rather than cache.** Any field stored purely to avoid recomputation costs
a line in the struct, a line in the constructor, and a line at each assignment.

## The order, and the rule

**One track at a time, hash-verified on three corpora after each pass.** C first — it is
the largest lever at 1.0968 and its workstream has already demonstrated the technique
(698 -> 658 with no behaviour change). Then A, then B if time allows.

**Push after each track rather than batching all three.** The board keeps the best result
per track, so a partial improvement banks immediately and cannot be lost by a later
regression — and if one refactor does break something, it is isolated to one track
rather than to three simultaneous changes.

**Do not trade time or memory for lines.** The exponents are 0.50 and 0.20 against 0.15,
so a refactor that costs 3% of T to save 10% of S is a net loss. The whole value here is
that it is free.
