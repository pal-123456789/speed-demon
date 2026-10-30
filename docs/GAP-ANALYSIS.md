# Where the points are: per-track gap analysis

Read off the live board. The leader's numbers are the target, and the useful
thing about them is not the gap in points but the gap in **axes** — because each
axis has a different exponent and a different cost to move.

## Ours versus the leader

| Track | Axis | Leader | Ours | Gap | Exponent |
|---|---|---|---|---|---|
| **A** | T | 0.032 s | 0.126 s | **3.9×** | 0.50 |
| | M | 20 MB | 63 MB | 3.2× | 0.20 |
| | S | 310 lines | 321 lines | — | 0.15 |
| **B** | T | 0.332 s | 3.938 s | **11.9×** | 0.50 |
| | M | 40 MB | 132 MB | 3.3× | 0.20 |
| | S | 790 lines | 723 lines | — | 0.15 |
| **C** | T | 0.050 s | 0.903 s | 18.1× | 0.50 |
| | M | 13 MB | 386 MB | **29.7×** | 0.20 |
| **D** | T | 0.230 s | 1.579 s | 6.9× | 0.50 |
| | M | 77 MB | 236 MB | 3.1× | 0.20 |

## What the table actually says

**1. Line counts are not the problem.** A: 321 vs 310. B: 723 vs 790 — we are
*smaller* than the leader on B and still 11.9× slower. So S is not what separates
us, and cutting lines would not close any of this. Every remaining gap is T or M.

**2. Track C's memory is a 29.7× defect and it is ours.** The leader holds 13 MB
while reading a 202 MB CSV and emitting 240 aggregate lines. That number is the
proof that **`/data` is not charged as page cache** — I had assumed a ~202 MB
floor and designed around it, which was wrong. With no page-cache floor, our
386 MB decomposes as roughly:

```
  12 workers x 4 MiB read buffers      48 MB
  12 x per-country name sets          ~135 MB   <- the real defect
  region lookup + output + slack      ~200 MB
```

Twelve threads each keep their **own** set of distinct names per country, so the
corpus's ~94,000 distinct names are stored twelve times over. That is the single
worst number in the submission and it has one cause.

**3. Our per-line work is higher than the leader's on Track A.** Same line count,
3.9× the time. An earlier version of this section claimed three hash operations
per line; **reading the code shows that is wrong and the correction matters**,
because it points at a different fix. Track A already uses a multiply-xor
(FxHash-shaped) hasher and a SINGLE map:

```rust
type Map = HashMap<Box<[u8]>, Group, BuildHasherDefault<Fx>>;
```

So there is one hash per line, not three. What that line still does is hash a
**composite byte key of roughly 13 + 30 bytes** (the hour concatenated with the
endpoint) and it must materialise those bytes before it can look anything up.

The two candidate causes, which a profiler should separate before either is
pursued:

* **Key width.** Hashing ~43 bytes per line where an 8-byte packed
  `(hour slot, endpoint id)` would do. The hour needs no hashing at all —
  `YYYY-MM-DDTHH` is 13 fixed-width bytes, so its index is arithmetic on digits.
* **Per-line byte scanning.** Finding the three field boundaries by walking
  bytes, roughly 60 comparisons for a 66-byte line.

The arithmetic that says something is wrong: the leader does 238 MB in 0.032 s
across 12 threads, about **5 cycles per byte**, and we do about **19**. Both
figures are estimates from wall time, but a 4× per-byte difference on the same
line count is not tuning — it is a different amount of work per line.

**4. Track B is the largest single gap: 11.9× on time.** Our build is 4.5–7.3 s
of a ~5 s run, bisected to tokenise 0.51 s, +hash lookup 2.67 s, +dedupe/push
4.95 s, over 36.28 M tokens of which 28.7 M hit a needed term. The leader does the
whole index and all 50,000 queries in 0.332 s — a third of a second for 254 MB.
That is not a faster version of our design; it is a different design. Find it
before optimising ours.

## What is NOT worth doing

* **Rewriting the byte scanners.** Track A's `next_space` was a byte-at-a-time
  loop sitting right next to `nl`, which is eight-at-a-time, so I rewrote it to
  match. Correctness was never in doubt (5/5 on the published hash), and the
  interleaved A/B rejected it: **0.3550 s vs 0.3645 s, 3% slower.** The reason
  redirects the search — `buf[i..].iter().position(|&c| c == b' ')` **already
  compiles to SIMD.** rustc vectorises that pattern into a 16- or 32-byte scan, so
  the source only *looks* byte-at-a-time and my hand-written 8-byte loop processed
  fewer bytes per iteration than the compiler's. The idiom appears in all four
  tracks, so none of the per-byte gaps are in scanning.

* **Replacing Track A's tree merge with a single-level k-way merge.** The tree
  merge folds twelve lists in ~4 levels and re-copies each group's latencies at
  every level, which looked like ~180 MB of memcpy for 50 MB of data. Rewritten,
  correctness held (5/5), and the A/B was flat: **0.4955 s vs 0.4975 s.** The
  copied-bytes theory was wrong, or the cost is far below the measurement floor.
  The k-way version is also ~24 lines longer, so it is strictly worse and was
  reverted. The "45 ms merge" figure that motivated it came from a single loaded
  run and should not be trusted until it is measured on a quiet box.

* **Cutting lines.** B is already smaller than the leader's B (723 vs 790) and
  11.9× slower, so S is not what separates us.
* **Micro-tuning C.** C is dropped from the best three, so its score does not
  reach the Overall until it passes B. Its **memory** is still worth fixing on
  engineering grounds, but not for points.
* **Trading memory for time by hand**, e.g. Track D at 8 workers instead of 12:
  measured, reverted, and the sign of the local measurement was opposite to the
  board's.
* **Chasing local timings.** Six changes have now measured differently locally
  and on the board, and in both directions. Local numbers are for direction and
  for the hash; the hash is exact either way.

## The order I would work in

1. **B's index design.** 11.9× on the axis with the largest exponent, on a track
   that counts. Everything else is secondary.
2. **A's three hashes per line → one.** 3.9× available with no line-count cost.
3. **C's duplicated name sets.** 29.7× on memory; the largest defect, on the one
   track that does not currently count.
4. **D's 6.9×.** A radix heap over `BinaryHeap` is the classic 1.5–2× here.
