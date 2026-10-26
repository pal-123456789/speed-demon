# MODEL OUTPUT DIGEST — A · B · budget (external high-level model, DESIGN-ONLY)

> **Status:** PRE-KICKOFF DESIGN (rule 04/05). Answers to prompts P8, P3, P1 from
> `docs/depth/98-MODEL-PROMPTS.md`, pasted back by the team, distilled here.
> **DESIGN-ONLY:** any code fragments below are design sketches — they live in
> this doc, never in `track-*/src/` before Oct 30 18:00 UTC. Every estimate is
> the model's; the harness/bench are the authority at H0.

---

## 1. Where the points are (scoring lever table — universal)

| Factor-of-2 on... | K multiplier | speed pts | track pts (×0.65) |
|---|---|---|---|
| Runtime T | 1.41 | +7.5 | +4.89 |
| Memory M (until 50× cap) | 1.15 | +3.0 | +1.95 |
| Code lines S (if it could pay) | 1.11 | +2.3 | +1.50 |
| One extra dependency | 0.90 | −2.3 | −1.50 |

**Break-evens** (each optimization must beat these to be worth it):
- An extra crate must make you **≥1.23× faster** → stay std-only, always.
- Doubling memory must buy **≥1.32× speed**.
- Doubling code size must buy **≥1.23× speed** (golfing lines is nearly worthless).
- Halving M ≈ 0.4 doublings; halving S ≈ 0.3 doublings; 10 judge points ≈ 0.72
  doublings (≈3.6 h at B's learning rate).

T has the same 0.50 exponent on every track — *absolute reference seconds don't
matter, only the ratio*; concentration on one track is only wrong when the
water-filling solution wants more hours than the track's remaining doublings allow.

## 2. The 72-hour budget (P8 — analytic solution, supersedes the greedy)

**Model.** P_t(h) = P0_t + β·d_t(h), β = 4.892 track points per doubling,
d_t(h) = r_t·(1 − e^(−h/(H_t·r_t))) — r_t = doublings left before the floor,
H_t = hours per doubling. **KKT (water-filling):** hours go where the
log-return is equal: marginal/P_t = λ for every funded track.

**Corner lemma:** with every P_t increasing, exactly ONE track gets zero hours —
splitting across all four is never optimal (unless ties/saturation/gate-hedging).
Drop the track d maximizing Θ_d = (H + A − a_d)·H_d^(1/3) (A = Σ a_t,
a_t = P0_t·H_t/β). Limits: big budget → drop the slowest learner (max H);
small budget → drop the lowest total.

**Worked example (J = 60 assumed on every track; 60 h):** A 20.5 h → 63.7 pts,
B 33.4 h → 76.5, C 6.0 h → 42.5, **D 0 h** (baseline only); G ≈ 59.2.
Equal-split comparisons: 4×15 h = 54.7; A/B/C 20 h each = 58.6. The D-vs-C call
is close — it flips to dropping C if D's doubling time < ~6.2 h instead of 8 h.

**Recommendations adopted:**
1. Fund A, B, C; keep D as a verified baseline entry.
2. Probe C vs D in the first 2–3 h with one known 2× optimization each, then re-solve.
3. Confirm B's remaining-doublings floor before committing 33 h; if r_B < ~4,
   shift hours to A and C.
4. **Re-solve every 8 h** from measured H_t and r_t (implemented as
   `k_whatif.py --waterfill`).
5. Buy M or judge points whenever they cost fewer hours than their doubling-equivalent.

## 3. Track A — the model's engine (supersedes parts of the A deep dive)

**The timestamp does NOT need parsing.** The output needs only the hour bucket,
and the first 13 bytes (`YYYY-MM-DDTHH`) identify it:
- Hit path: an MRU cache of 13-byte keys via two overlapping u64 loads (bytes
  0–7, 5–12) + two compares (~5 cycles). Keep 2–3 entries (shard interleaving
  flips the hour occasionally).
- Miss path: 1024-slot table keyed on the 13 bytes → internal `hour_idx` (~30 cycles).
- Ordering: sort the fixed-width ISO keys **bytewise** — chronological for free,
  zero calendar arithmetic. Calendar math (days-from-civil) needed ONLY if a
  non-`Z` offset appears (grep the input first).
- Validation of the µs fraction + `Z` in one AVX2 pass over the 32-byte window:
  digit check via `min_epu8(t,'0'+0)`-style compare + punctuation `cmpeq`, ~4
  cycles, run as the fast-path guard; slow path = generic split.

**Separators (AVX2):** the timestamp is fixed-width → first space known at byte
27, status fixed 3 bytes; remaining searches are `cmpeq`+`movemask`+`tzcnt`
(method end 16 B, endpoint end 32 B loop, line end `'\n'`).

**Per-line budget (model estimate, ~95 cycles/line):** guard+hour key 10,
separators 18, endpoint hash+probe+verify 40, status `byte0=='5'` 2, latency
SWAR parse 10, count/error 5, chunk append 8 → 3.1M lines ≈ 118 ms
single-threaded @2.5 GHz, ~15 ms over 8 effective cores, T ≈ 40–50 ms wall.

**Percentiles: selection beats sorting.** Zipf s≈1 → H_1842 ≈ 8.1; the top
endpoint holds ~383k records. Select (p50, then p95 inside the upper part, then
p99 inside that) ≈ 20 cycles/element vs sort ≈ 5.5·log2(n) → crossover at
**n ≈ 12–16** (below: insertion sort). P²/t-digest have NO crossover here:
~60–100 cycles/sample/quantile, inexact, and **not merge-deterministic across
thread counts → both fail the SHA gate regardless of speed**. The exact-index
formula: k = ceil(p·n/100), idx = (p·n − 1)/100 (integer); but if the reference
uses float `ceil(0.95*n)` or `int(0.95*n)`, we must replicate ITS expression —
run a one-time sweep over n = 1..400k comparing the Python expression to the
integer form before trusting it (fixture below).

**Error rate:** keep e, t as u64; `e as f64 / t as f64` is bit-identical to
Python's `e / t` (both < 2^53). Match the reference's expression ORDER
(`(100*e)/t` rounds once; `e/t*100` rounds twice and can differ). Integer
formatting: q = (e·10^D)/t, r = (e·10^D)%t, round up if 2r > t — provably safe
for non-ties at D ≤ 6 (double error ≤ 1.1e-12 vs gap ≥ 1.6e-7); **ties
(2r == t) route through `format!("{:.D}", e as f64 / t as f64)`** — verify
against Python.

**Memory ledger (~23–25 MB target):** values 12.4 MB (3.1M × u32) · chunk links
+ slack ~3.5 · count/err slabs ~1 · read buffers 1.5 (128 KiB/thread + 512 B
overhang, atomic-counter dispatch) · endpoint tables 1.5 · merge scratch + output
window ~1.5 · baseline ~2 · output page-cache ~3. If the file is NOT roughly
time-ordered, every thread sees every hour → slack balloons: then assign hours
to threads or use count–scatter (19–20 MB but +50% compute; only wins if the
second pass adds <9% to T — it won't). Pre-size with `with_capacity`, never let
`Vec` double.

**The latency_us trap fixture (adopt verbatim):** one endpoint, one hour:
- latencies 999, 1000, 1001, 16777217 (f32 rounds it), 4294967295 (breaks i32
  parse and u32 sum);
- nearest-rank vs floor-index groups n = 1, 2, 20, 100 with latency = 1000·rank
  (at n=20: nearest-rank p95 = 19000, p50 = 10000 vs floor-index 20000/11000);
- tie group e = 1, t = 160;
- error classes 500, 599, 400, 600 (only 5xx counts).

## 4. Track B — the model's engine (supersedes parts of the B deep dive)

**Phrase queries need NO positions.** Run a **phrase automaton (Aho–Corasick over
term ids, not bytes)** in the same pass that tokenizes for BOOL:
- Alphabet = term ids; ≤ ~60k states → ~2 MB transition/failure/dict tables.
- +10–20 cycles/token; ≈ +45 ms wall across 12 threads (estimate).
- Reset state at record boundaries (phrases never span records → slab edges need
  no overlap handling).
- **Each phrase becomes a pseudo-term** in the same id space: phrase p emits
  (T_q + p, doc) with a per-phrase last-doc stamp → a PHRASE query is a single
  list lookup, no verification. Position storage drops to ZERO, independent of
  the phrase count (a positional merge over multi-million stopword lists would
  cost seconds across 19k phrases).

**Term scoping:** only query terms get postings (term ids assigned by first
appearance in the query file; open-addressing table on a hand-rolled 64-bit hash
with byte-compare on tag match — a false positive breaks the SHA gate).

**Tiered postings (blocks beat delta-varint):** varint needs a skip table (= a
worse block layout) and saves only ~0.7 B/posting → ~20 MB at 30M postings →
0.8 speed points — not worth a 2–3× slower hot path.

| Tier | df | Layout | AND behavior |
|---|---|---|---|
| tiny | < 128 | sorted u32 array | gallop, L1 |
| mid | 128 ≤ df < D/16 | 128-posting blocks, `block_max[]` + u16 offsets (raw u32 if spans ≥ 65536) | gallop on block_max, then AVX2 lower-bound |
| dense | df ≥ D/16 | D-bit bitmap | O(1) probe; bitmap AND = D/256 AVX2 ops |

Crossovers to microbenchmark: gallop vs scalar merge ~r > 8; gallop vs AVX2
merge ~r > 40; bitmap∩bitmap ~3–4× over list merge; probe-bitmap-with-small-list
always cheapest.

**Evaluation order — the correctness pitfall:** left-associative, no precedence
⇒ `A AND B OR C AND D` = `(((A∩B) ∪ C) ∩ D)`. Implementing AND-over-OR
precedence is a SHA-zero. Plan: left fold over OR steps; each maximal AND/NOT
run filters the whole accumulator, distributing (A∪B)∩C = (A∩C)∪(B∩C); positives
by df ascending, negatives by df descending; never build A∪B eagerly. A missing
term has df = 0 (AND/NOT leaves accumulator unchanged/empty; OR drops it) —
**never short-circuit the whole query** (a later OR can revive an empty accumulator).

**Build:** fixed 1 MiB slabs (start snapped to the next record delimiter;
partition independent of thread count), waves of 24 slabs (bounded buffers),
phase A scan → phase B compact (fixed ranges, prefix sums of record counts),
lists appended in file order; dispatch order never affects bytes.

**Memory budget ≈ 100–150 MB:** 40–50 MB postings (u16 blocks + bitmaps), 25–40
MB wave buffers, ~20 MB scratch, before output buffers. If wave buffers ever
double M, count–scatter (two-pass, exact sizing) is the 1.32× break-even escape.

**Parallel queries:** chunks of 64 consecutive queries (chunk size ∉ thread
count), scheduled heavy-first (Σ df), ping-pong `Vec<u32>` scratch + `Vec<u8>`
output per thread, 4-digit formatting table (40 KB), writer streams slots in
order as the contiguous prefix completes, one `write_all`.

**Rough T budget (model estimate):** build 0.1–0.15 s, queries 0.1–0.2 s, startup
ms → **T ≈ 0.3–0.5 s vs the 56.35 s reference**. Open question to the frozen
spec: the exact "id" in the output line and whether each line lists all ids or a
count.

## 5. Host & build notes adopted

- Benchmark **6 vs 12 threads** (cpuset 0-11 is likely 6 P-cores + SMT siblings);
  SMT only pays when memory-stalling. Probe with `lscpu -e` on the scoring box.
- `mmap` through hand-declared extern works std-only, but mapped pages may count
  toward cgroup peak — benchmark against `FileExt::read_at` + reusable L2-sized
  buffers. Avoid /tmp (tmpfs writes count against the cgroup).
- `std::arch` AVX2 behind `is_x86_feature_detected!` stays std-only; our
  build.sh already carries opt-level=3 / codegen-units=1 / panic=abort /
  strip=symbols (single-crate LTO adds nothing — one compilation unit).
- One `Vec<u8>` output, hand-rolled integer formatting, single write; no
  `println!`/`format!` in hot paths.
- Fixed-size blocks that don't depend on thread count; one result slot per
  block; merge in block order; integer/fixed-point math or a fixed-shape
  reduction tree for floats; own open-addressing table with a fixed hash, sort
  at the end.