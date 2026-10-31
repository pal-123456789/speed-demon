# Track B's memory: the postings should be two representations, not one

The board charges **213.9 MB**. Theirs is **72 MB**. The dominant term on both sides is
the postings, and the difference is entirely in how they are stored.

## What we do, and what it costs

```
22,240,530 deduped (term, doc) pairs, every one as a u32
89.0 MB of arena, plus a 10.6 MB term-id list per worker during the build
```

## What a hybrid costs

```
a bitmap over 250,000 docs = 3,907 words = 30.5 KB

~300 bitmaps (high-df terms)                    9.4 MB
the remaining third as u16 within 65,536-doc segments   14.8 MB
                                                     -------
                                                     24.2 MB      vs 89.0 MB
```

**Two mechanisms, and they are independent.**

**1. Bitmaps for high-df terms.** A term appearing in more than `docs/32` documents is
stored as a bitmap rather than a sorted list. Two thirds of all postings sit in a few
hundred such terms, so a few hundred 30.5 KB bitmaps replace roughly 59 MB of u32
lists with about 9 MB. The threshold matters: at `df > docs/32` a sorted u32 list costs
`4*df` bytes against a flat 30.5 KB, so the crossover is around `df = 7,800` — below
that the list is genuinely smaller and should stay a list.

**2. u16 doc offsets within 65,536-document segments.** Document ids run to 249,999, so
a full-width offset needs 18 bits and a u32 wastes 14. Split the document space into
four segments of 65,536 and an offset within a segment is 16 bits. **That halves the
per-posting cost from 4 bytes to 2 wherever a list is still the right shape**, which is
the 44.5 MB row above.

**Together they are 89.0 MB -> 24.2 MB.** If the arena is ~89 of the board's 213.9 MB,
that takes B to roughly 150 MB, worth `(213.9/150)^0.20 = 1.075`, about **+1.6 points
on B** — and B is our weakest counting track, so it carries the most leverage.

## And it should also help the time, which is the larger half of B's gap

A bitmap AND is `popcount(a & b)` over 3,907 words — a linear sweep with no branches. A
sorted-list intersection is a galloping merge whose cost depends on how the two lists
interleave. **For the highest-df terms — which is precisely where the current code does
the most galloping — the bitmap is not just smaller, it is the faster operation.** The
`eval_bool` path and `gallop` both get simpler for those terms.

## What has to change, and it is every consumer

`Postings` becomes an enum — `Bits { word_offset }` or `List { seg_offsets }` — and
every operation that takes postings must handle both:

```
intersect_into   bitmap & bitmap  |  bitmap & list  |  list & list
union_into       bitmap | bitmap  |  bitmap | list  |  list | list
difference_into  bitmap & !bitmap |  bitmap & list  |  list & !bitmap
gallop           no longer called for bitmap terms
eval_bool        dispatches on the term's representation
verify_phrases   uses whichever its terms have
format_result    writes the count and the ten smallest ids; a bitmap needs a
                 scan-to-first-ten step instead of a slice
```

**Roughly 150 to 200 lines.** The mixed cases are the ones that need care: a bitmap
against a sorted list wants to iterate the list and test bits, not to expand the bitmap
into a list.

## The order to do it in

**u16 offsets first, alone.** It is mechanical — every doc id becomes `(seg, off)` — it
halves the list memory with no new data structure, and it needs no mixed-case logic.
Verify the hash on three corpora, then measure on the board.

**Bitmaps second, behind the `docs/32` threshold.** This is the one that removes the
large term, and it is the one that changes `eval_bool`'s inner loop, so it goes after
the simpler change has been proven.

**Do not do both at once.** Each is hash-verifiable on its own on all three corpora, and
a combined change that fails the hash tells you nothing about which half broke it.

## The measurement rule that applies, learned the hard way today

**Peak working set is not cgroup `memory.peak`.** Track B's last memory change measured
185.5 -> 141.4 MB locally and the board charged **290 MB** — worse than the 213.9 it
replaced. This box cannot size a memory change. Land each step, push it, and read the
board's M before starting the next one.
