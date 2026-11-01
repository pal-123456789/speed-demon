# Track C: the fast structural parse buys nothing, and where the time really is

## The measurement

Two scratch builds, same source, same corpus on ext4, real Linux ELF, `SD_TIME` lap timers on.
`allslow` forces `fast()` to decline on every record, so every record goes through the literal
scalar parser:

```
base     parse = 0.141 s
allslow  parse = 0.137 s     <- fast() forced to decline on every record
```

**Forcing the structural path off makes the parse very slightly FASTER.** So `fast()` is not
contributing. Either it declines on most records, or it succeeds and costs about what the scalar
parser costs; the two builds cannot be told apart, and for deciding what to do they are the same
answer: the structural path is not earning its place on this corpus.

## Why that is plausible rather than surprising

A record here is about 61 bytes with short fields. A byte-at-a-time parse over 61 bytes is roughly
100 cycles; five 64-byte window lookups plus the prefix XOR and the digit folds are roughly the
same. **The structural fast path was designed for long fields, and this corpus does not have any.**

The corpus, measured:

```
records.csv   201,935,059 bytes    3,724,714 LFs    ~61 bytes/record
quotes        16,036,792            <- about 4.3 quotes per record
CR            1,659,401             mixed CRLF and LF terminators
tabs          0
BOM           357 273 277 at offset 0, on the header line only
```

Every record has a quoted name and a quoted note, many with `""` pairs, e.g.

```
3,"Jata ""Ji"" Givo","said open desk with signed after day",705802,0.05
```

## Where the 455 ns per record actually is

`parse` is 86% of the run at 0.141 s over 12 threads, and there are 3.72 M records, so one thread
spends about **455 ns -- some 1,365 cycles -- per record.** That is far above what parsing 61 bytes
costs, whichever parser runs, so it is not the parsing. The remaining candidates, all measured or
directly visible in the source:

```
per-country mark (fetch_or, 12 threads contending)   15% of the parse   [priced]
name confirmation (2nd random read into 202 MB)       6% of the parse   [priced]
16-record pipeline lookahead                          3% of the parse   [priced]
AVX2/PCLMUL dispatch is WORTH                          8.5%             [priced]
the rest                                              ~67%              unaccounted
```

Within that 67%, the source points at shared-state contention rather than computation: one
2^18-slot name table and one per-country bitset, both probed or written by all twelve threads on
every record, with the name confirmation then chasing a second random offset into the mapped file.

## What to do next, in order

1. **PRICE THE CONTENTION.** Build a scratch variant with `nt` forced to 1 (accept the slowdown
   from lost parallelism) and one with the name table replicated per thread, and compare the
   per-record cost. If twelve threads are fighting over the table and the bitset, the fix is to
   shard them by thread and merge at the end -- but the merge must be exact, and the last three
   "cheap bytes" changes on these tracks each cost 30 MB, so measure memory too.
2. **DECIDE THE STRUCTURAL PATH'S FATE.** It is not helping. Either it is rewritten so it actually
   skips work (the scalar parser is already the faster of the two), or it is deleted and the track
   keeps the scalar parser alone -- which would also remove a large amount of source, and source is
   this track's second-largest gap at 2.79x.
3. **DO NOT** chase the pre-pass (14% of the run, measured) or the pipeline (3%, measured).

## Method note

`allslow` answers WRONGLY by construction and was never committed. That is the point: a scratch
build that is allowed to be wrong prices one component, and pricing a component before changing it
is the technique that has worked on every track. Every number above came from a scratch build or a
committed lap timer, not from reading the code.
