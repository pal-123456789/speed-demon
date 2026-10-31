# Parser and table techniques worth adopting

Design notes from reading high-quality C++ implementations of these same four tasks.
These are standard techniques — bitmask CSV parsing, PCLMULQDQ prefix-XOR, swisstable-
style tag arrays — and they are recorded here as engineering we should do, with the
measurement that would justify each.

## Track C — the parse, and this is the big one

The best implementations do **not scan bytes**. They build a bitmask per 64-byte block
and walk fields by clearing bits.

```cpp
q_ = quote_mask(v0) | (quote_mask(v1) << 32);          // 64-bit mask of '"'
__m128i x = _mm_clmulepi64_si128(x, all_ones, 0);
u64 inside = u64(_mm_cvtsi128_si64(x)) ^ carry_;        // prefix-XOR: in-quote or not
carry_ = u64(i64(inside) >> 63);                        // carries parity across blocks
bits_ = (separator_mask) & ~inside;                     // separators OUTSIDE quotes
```

**`_mm_clmulepi64_si128` computes the carry-less product of the quote mask with
all-ones, which is exactly the prefix-XOR.** So "am I inside a quoted field?" for 64
bytes at a time costs **one instruction**, and the parity carries into the next block
with a sign-shift. Then:

```cpp
u32 i = __builtin_ctzll(bits_);      // next separator
bits_ &= bits_ - 1;                  // clear it
```

That clears the lowest set bit, so the walk is `ctz` + `and` per field and **there is
no per-byte loop anywhere**. A companion `popcount(q_ & ((1 << i) - 1))` counts the
quotes before each separator, which is what `""` unescaping needs later.

`PCLMULQDQ` is available under our existing `-C target-cpu=x86-64-v3`, so this needs
no build change. In Rust it is `core::arch::x86_64::_mm_clmulepi64_si128` behind
`unsafe`, with a scalar fallback for the tail.

**Why it matters here:** our Track C is `intern 69.6%` of the worker counters and the
parse is the rest, and our interner sits at 49.5 ns against a 6.7 ns hash-only floor.
The parse is where the bitmask walk wins outright.

## The name table: separate the probe from the entry

```
tags_   u16[1 << 17]      256 KB   cache-resident; the probe chain touches ONLY this
slots_  Slot[1 << 17]      32 B each, with 24 name bytes stored INLINE
arena_  overflow only, for names longer than 24 bytes
```

A probe computes the hash, walks **only the tag array** to find the slot, then
`_mm_prefetch`es the fat slot before reading it. Comparison is an AVX2 32-byte
`movemask` against the inline head, so **the common case never dereferences the
arena** — the name is right there in the slot.

That is the structure our Track C should have. Ours probes a table whose entries point
into an arena, so every probe that hits costs a **second dependent cache line**. The
measured cost is 49.5 ns/intern against a hash-only floor of 6.7 ns, and the gap is
exactly that second dependent load. Two cache lines per intern become one.

Cost to check before adopting: 24 inline bytes at 128 K slots is 4 MB of slots, against
our current 2.2 MB. Track C is footprint-sensitive — the workstream measured that
`SHARDS` 256 costs +2.9 MB and 512 costs +4.6 MB, both net losses — so this must be
measured, not assumed.

## Track A — the line parse

```
one 64-byte AVX2 probe at byte 28 gives a 64-bit space mask and a 64-bit newline mask
if the newline is inside that window: the whole line resolves with ctzll, no scan
sp & (sp - 1)                clears the low bit, so ctzll gives the SECOND space
ee[1] == '5'                 is the entire 5xx test -- the status's hundreds digit
record: lat << 12 | key       one u32, not eight bytes
```

**The record packing needs a side channel.** `lat << 12` only works while
`lat < 1 << 20`; larger latencies must go to a separate vector or the key bits are
silently corrupted. **Our corpus really does contain `latency == 4294967295`** (the
Track A workstream measured it, in both the full corpus and the tiny oracle), so a
packed record without that side channel is a latent wrong answer, not an optimisation.

## What NOT to take, and this is the important half

**Do not hardcode the date range.** One implementation computes its hour index as
`day * 24 + hour - 18` with `HOURS = 72` and a hardcoded month and both day-of-month
values in the label formatter. A holdout seed with a different range indexes outside
its hour array or emits a wrong label, with no fallback.

**That is the same class of bug we have already removed twice today:** an out-of-bounds
read in Track C's amount parser that the full corpus could never expose, and a cut
landing exactly on a line start that was silently dropping a document from Track B.
Both passed every board run and would have failed the holdout.

**The general rule, which is now the most valuable thing this campaign knows:** a
constant that is true of the practice corpus and not of the specification is not an
optimisation, it is a deferred zero. `HOURS = 72` is true of seed 1. It is not in the
spec.

## Ranked by expected value, with the risk stated

| Technique | Expected | Risk / cost |
|---|---|---|
| C: bitmask parse with PCLMULQDQ parity | large — the parse is the non-interner remainder of C's 70% | needs `unsafe` + a scalar tail; no build change |
| C: tag-separated table with inline heads | removes one dependent cache line per intern | +1.8 MB of slots on a footprint-sensitive track; **must be measured** |
| A: 64-byte AVX2 line probe | the scan is 67% of A | `unsafe`; keep a scalar fallback |
| A: `ee[1] == '5'` for the error flag | removes a 3-digit parse per line | none — the spec guarantees 3 digits |
| A: packed u32 record | halves the record store, 24.8 → 12.4 MB | **needs the big-latency side channel or it is wrong** |
| A: `mmap` the input | removes a 238 MB copy | ~3.6k page faults, needs `unsafe` |

**None of these is implemented.** Each needs the same bar as everything else that has
shipped: hash-verified on the full corpus plus the tiny oracle and the official sample,
an interleaved A/B on a quiet box, and reverted if it is correct but slower.
