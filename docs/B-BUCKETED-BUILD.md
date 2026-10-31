# Track B's build: bucket the postings instead of listing and scattering them

**This is the second half of B's fix, and it is larger than the first.** The
bitmap/list hybrid in the sections below reduces the *arena*. This removes the
*term list* entirely, and the term list is 89 MB of pure scratch — the same size as
the arena it exists to build.

## The two-pass shape we have, and why it costs double

```
pass 1  scan 254 MB, intern each token, append its term id to a per-worker list
        -> 22.2 M u32 entries, 89 MB, held until the join
pass 2  count global df per term, prefix-sum the offsets, scan again, scatter each
        posting into the flat arena at its term's offset
        -> 22.2 M u32 postings, 89 MB
```

**Every posting is paid for twice** — once to remember which term it was, once to
store it — and the second pass exists only because a single-pass scatter needs the
global offsets, which need a full count, which needs the first pass.

## The shape that pays once

```
one pass, no counting, no scatter, no term list:

  for each token:
      id = table.intern(token)              // as today
      doc = the document being scanned      // as today
      delta = doc - last_doc[id]            // 4 bits usually, more rarely
      buckets[id % NBUCKETS].push(unit(delta, id))

NBUCKETS = 32 parallel Vec<u16>, one per residue class of the term id
```

**A posting becomes one `u16`, not one `u32`, and there is no second array.**

```
unit = id_in_bucket << 4 | min(delta, 15)
if delta >= 15 { push a second u16 carrying the full delta }
```

**The 12-bit field addresses one bucket, so it holds 4,096 distinct terms; a bucket
that overflows re-homes into a spill list.** With 75,748 terms over 32 buckets that
is about 2,370 terms per bucket, which fits with room to spare — **but the field
width must be asserted at run time, not assumed**, because the term count follows
the query set and a holdout seed can move it.

**Why the gap encoding is safe:** a term's postings are appended in document order
within a worker's range, so `delta` is small almost always; the escape exists for
the case where it is not, and it is not an assumption about the data. **A 4-bit gap
that silently truncated would corrupt every subsequent posting in the list** — the
same class as the unguarded record shift that Track A already removed.

## What this costs and what it saves, by the arithmetic already in this file

```
term list  removed          -89 MB
arena      u32 -> u16       -44 MB
                            -------
B peak     209.7 -> ~77 MB      (209.7/77)^0.20 = 1.22 on K
```

**Against that, the escape units and the per-bucket overhead, which are small.**
The time effect is expected to be positive too: one scan instead of two, and half
the posting bytes written. **Both effects point the same way, which is unusual
here** — most of the changes measured in this campaign traded one axis for another.

## What has to change

1. **`build_range`** — delete the term list, the `starts` array and the scatter
   loop; append units to `buckets[id % NBUCKETS]` inside the token loop.
2. **A decoder** — `units(bucket, id) -> impl Iterator<Item = u32>` walking u16
   units and accumulating deltas, with the escape case handled.
3. **`Postings`** — keep `offs`/`arena` for the regularised case, or replace both
   with the bucket array plus a per-term index. **This is the decision that decides
   how large the change is**, and it should be made before writing code.
4. **The seven consumers** — `intersect_into`, `union_into`, `difference_into`,
   `gallop`, `eval_bool`, `verify_phrases`, `format_result`. Every one currently
   takes `&[u32]` from `postings.get(t)`.
5. **The bitmap path** — terms above `df > docs/32` should stay bitmaps, because a
   bitmap OR is far cheaper than walking two unit lists, and this is what makes the
   hybrid worthwhile rather than merely smaller. Sections below.

## The order to do it in, and the stop conditions

**Do the unit encoding first, alone, and push it.** It is the larger saving and it
is independent of the bitmaps. Then the bitmaps. **Do not do both at once: a
combined failure says nothing about which half broke it.**

**Stop and revert if** the board shows T worse, or M not lower, or the source grows
by more than the arithmetic repays. At S^0.15 and 973 lines, **+38 lines costs
0.6%, so a change has to buy more than 1.2% of time before it is worth writing
down.** Five changes in this campaign were correct in principle and lost on exactly
this exchange rate.

## The measurement rule that applies, learned the hard way today

**This box cannot size a 5 MB memory change or a 5% time change.** It disagreed
with the board by 11% *in the opposite direction* on one attempt today, and it has
been wrong about a small delta eight times. **Push it and read the board's M and T;
the board keeps the best result per track, so a wrong guess costs one run and not
points.** The local box is for correctness and for finding large structural waste —
it found Track C's 386 → 14 MB — and for nothing below that.
