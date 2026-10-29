# PROMPT PACK — everything to ask the high-level model, in priority order

> Paste one at a time; bring the answer back and it gets triaged against the
> specs/oracle, folded into the design, and committed. **P-A is the critical
> path** — it unblocks H1. P-D can be run any time and is needed on D3.

---

## P-A · Implementation pseudocode + test vectors (CRITICAL PATH)

```
You are a principal Rust performance engineer. Give me exact pseudocode (not
prose) for four routines in std-only Rust (no crates), plus the test vectors that
prove each. Correctness is gated by SHA-256 on one output file, so any deviation
is a zero. All arithmetic must be integer; output must be byte-identical across
thread counts.

CONTEXT. Four hackathon tracks, each a separate binary, 12 hardware threads,
8 GB RAM (peak measured as cgroup v2 memory.peak AFTER exit, so page-cache pages
the process first touched are charged), AVX2 only.

1) A · nearest-rank percentiles. Group sizes n can be any value >= 1 (some
   groups have exactly 1 request, many have 2-5). Ranks are
   k_p = (p*n + 99) / 100 for p in {50,95,99}. I want ONE array of u32 latencies
   per group and the three values with minimum passes. Known trap: for n = 17, 18
   and 19, k95 == k99, so a narrowing slice that excludes the pivot is out of
   range; and full sorting must be cheaper than selection for small n. Give the
   routine, the small-n cutoff you would choose and why, and test vectors:
   n = 1..200 with duplicates, all-equal, sorted ascending, sorted descending,
   one huge outlier.

2) B · phrase matching with per-slot cursors. A phrase is k terms that must
   occupy k CONSECUTIVE token positions inside ONE document. Positions are
   document-relative. A term may repeat ("x x x" needs three consecutive x). One
   document must be counted once even if the phrase matches twice ("go go go"
   against "go go"). Queries are tokenised with the same byte-level tokeniser as
   the documents (ASCII-only lower-casing; every byte >= 0x80 separates, so
   "Zürich" is the two tokens z and rich). Give the matching routine (positions
   per term, one cursor per phrase slot) including how you handle a term that
   occurs nowhere, k = 1, and a phrase whose shortest list is long, and test
   vectors for each trap.

3) C · CSV record walker across chunk boundaries. Chunk size is fixed (64 B in
   tests, 4 MiB in production) and does not depend on the thread count. A chunk's
   starting quote state is obtained by counting '"' bytes mod 2 and prefix-XORing,
   but that is only valid if every quote is structural (a bare '"' in an unquoted
   field breaks it). Give: (a) the rule for whether an offset is a record start,
   (b) the record walker that consumes a chunk given its initial state and
   returns the state at the end, (c) the validation that detects a parity
   mismatch and the serial re-parse fallback, (d) field extraction keeping
   (start, len, quoted) slices, with BOM stripping when a BOM starts a record,
   and LF/CRLF/CR terminators where a CR inside quotes is data. Test vectors: the
   worked example (quoted comma, "" escapes, embedded LF and CRLF, mid-file BOM,
   trailing space in a name, empty vs quoted-empty name, a region id absent from
   the lookup file).

4) D · bidirectional Dijkstra. Directed graph, unsorted edges with parallel
   edges (minimum wins), self-loops and zero weights; distance-only; -1 when
   unreachable; s == t gives 0. I intend: two-pass parallel CSR (atomic degree
   counting, then fetch_sub scatter from each row's end), edges packed as
   (w << 32) | v, plus a transpose. Alternate forward/backward expansion,
   stop when tf + tb >= mu, update mu unconditionally on every relaxation, and
   break explicitly when either heap is empty. Known traps: INF arithmetic
   wrapping (use INF = u64::MAX/4 and saturating adds); s == t must return 0
   early or a cycle through s is returned; a heap key needs key < 2^32 so pack a
   21-bit node instead of 32; the touched-list reset must record the FIRST label
   write (relaxed-but-never-popped nodes would otherwise carry stale labels into
   the next query and make answers depend on thread count); mu can reach twice
   the one-direction bound so all sums are u64. Give the full routine plus test
   vectors: parallel edges (lighter listed first and last, three-way, exact
   duplicates), zero-weight cycle, self-loop, s == t, source with out-degree 0,
   target with in-degree 0, unreachable pair, and a max-weight chain that must
   not overflow.

For each: pseudocode with types, the invariants that must hold, the complexity,
and the exact test vectors as input -> expected output. Terse, no preamble.
```

---

## P-B · Where the next 10–20% is, per track

```
You are a performance engineer. Below is what a submission already does and the
measured profile of the naive reference it is scored against. Scoring:
K = (Tref/T)^0.5 * (Mref/M)^0.2 * (Sref/S)^0.15 * (Dref/D)^0.15, speed =
50*log10(K); T is uncapped, M is capped at 50x, S and D only cost (S = code
lines beyond the reference's, so every line must be paid for).

A: reference spends 48.7% parsing RFC3339 timestamps; we removed that entirely
(hour bucket = the first 13 bytes; microseconds are never emitted). Remaining
work: parallel line scan over 3.1M lines, interning (hour, endpoint) per thread,
counting-sort aggregation, nearest-rank percentiles, one buffered write.
Reference 32.05 s / 228 MB.

B: 250 MB corpus, 26% tokenise / 23% build / 17% phrase / 6% boolean in the
reference. We tokenise byte-level, index only query terms, match phrases on
positions, evaluate left-to-right with gallop/merge over ascending u32 postings.
Reference 56.35 s / 2.1 GB.

C: 24% amount parse / 20% aggregate / 10% CSV parse / 3.7% join. We walk chunks
with quote parity, parse cents by integer accumulation, aggregate per country in
per-thread slots. Reference 14.44 s / 350 MB.

D: 97.1% search / 0.4% edge parse. We build a parallel CSR and run Dijkstra from
a std BinaryHeap, with bidirectional as a planned upgrade.
Reference 72.69 s / 852 MB.

For EACH track: rank the next 3 optimisations by (expected T gain) / (code lines
added + correctness risk), say what measurement decides whether to take each, and
name one thing that looks attractive but will not pay under this formula. Also
say where memory.peak (0.20 weight, capped 50x) is a cheaper win than time.
```

---

## P-C · Judge-facing documentation (35% of the track total)

```
A hackathon track total is 0.65 * machine speed score + 0.35 * judge score. The
judge score is split: code quality 15%, technique 12%, PERF.md 8%. Judges see the
repository, the write-up and the live leaderboard history — not a live demo.

My submission: four Rust std-only tracks (no dependencies), each exactly one
output file gated by SHA-256, verified against the organizers' own reference and
their published expected hashes, with a CI gate wall, a differential-testing lab, a
conformance corpus covering every documented edge case, and per-commit evidence.

Tell me, concretely: (1) the exact section structure for a PERF.md that earns the
full 8% (what claims, what evidence, what methodology, how to present
before/after numbers truthfully); (2) the README structure that earns the most
code-quality credit in a 5-minute read; (3) ten specific things judges reward
that most teams omit; (4) five things that read as padding or over-claiming and
should be cut; (5) how to present the verification story so it reads as rigour
rather than noise.
```

---

## P-D · The 5-minute video (needed on D3)

```
Produce a 5-minute video plan for a hackathon submission. Structure: the first
4 minutes are slides with a voice-over; the last minute is silent screen capture
of a terminal run with classical music over it.

The project: "Speed Demon 2026" — four optimisation tracks in Rust (std only).
Track A ingests 3.1M HTTP log lines and reports per-(hour, endpoint) counts,
p50/p95/p99 latencies and 5xx rates; B indexes a 250 MB corpus and answers
boolean and phrase queries; C joins a hostile 200 MB CSV (quoted commas, escaped
quotes, embedded newlines, BOMs) to a region lookup and aggregates money exactly
in integer cents; D answers 2000 shortest-path queries on a 1.74M-node,
5M-edge graph. Scoring: K = (Tref/T)^0.5 * (Mref/M)^0.2 * (Sref/S)^0.15 *
(Dref/D)^0.15, speed = 50*log10(K); the reference implementations are naive
Python. The work was verified against the organizers' own published reference,
their test suite (58 tests), their Dockerfile guardrail, and published SHA-256
expected-output hashes.

Deliver: (1) a slide-by-slide outline for 4 minutes (about 12-16 slides) with the
speaker note for each and a suggested on-screen visual; (2) ONE continuous
narration script of about 600 words (about 4 minutes at 150 wpm) written to be
read by a text-to-speech voice — plain prose, no stage directions, no markdown,
no bullet symbols, numbers written the way they should be spoken; (3) the
1-minute terminal sequence: the exact commands, in order, and what appears on
screen, chosen to prove correctness and speed in 60 seconds. Keep every claim
truthful and specific; no hype adjectives.
```

---

## P-E · Red-team round 2 (run on P-A's answer)

```
Below is pseudocode produced by another model for four performance-critical
routines. Act as a hostile reviewer: find where it is WRONG, not where it is
clever. For every finding give the routine, the precise failure, the input that
triggers it, and the minimal fix. Pay particular attention to: off-by-one in
rank selection at small and boundary n; cursors that allow a match to cross a
document boundary; quote-state derivation that assumes every quote is structural;
integer width and sign handling in money; heap-key packing limits; INF entering
arithmetic; and state that survives between queries or chunks. Reject findings
you cannot trigger with a concrete input.
```

---

## Order of play

| When | Prompt | Why |
|---|---|---|
| before H1 | **P-A** | unblocks the four critical modules |
| on its return | **P-E** | verify the verification |
| H3–H6 | **P-B** | decides the second rung per track |
| H20 (before submitting baselines) | **P-C** | judge score is 35% and is written, not coded |
| D3 morning | **P-D** | video, slides, narration, output take |
