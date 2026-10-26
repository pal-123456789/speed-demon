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

---

## 6. Track D — the model's engine (P5)

**Shape check:** 92 MB / 5M edges ≈ 18.4 B/line → ids ≈ 6 digits (n ≈ 10^6),
weights 4–5 digits, avg degree ≈ 5. Verify against the header. Search loop is
**memory-latency-bound, not SIMD**; reference ≈ 30 ms/query ≈ 10^4–10^5
relaxations/query. CH preprocessing of 10^6 nodes costs seconds against a total
budget of tenths of a second → confirmed deprioritized.

**(a) Parse + CSR, two passes, no edge buffering.**
- Fixed 1 MiB chunks (thread-count-independent), start snapped past the next
  `\n`, atomic-counter dispatch, 1 MiB buffer per thread (12 MB total).
- **SWAR digit decode** (no per-byte branches, ~12–15 cycles/number):
  `bad = ((x+0x4646464646464646)|(x-0x3030303030303030)) & 0x8080...`;
  `len = tz(bad)>>3`; left-pad; the ×2561 / ×6553601 / ×42949672960001
  multiply-shift pairs → ~40 cycles/line, ~11 ms across ~7.5 effective cores.
- **Pass 1 (parallel):** parse u only, `deg[u].fetch_add(1)`; scan →
  `cursor[u] = prefix(deg)` (row END positions, 4 MB AtomicU32 reused).
- **Pass 2 (parallel):** parse (u,v,w); `pos = cursor[u].fetch_sub(1)-1`;
  `edge[pos] = (w<<32)|v`. Row order from atomics is nondeterministic —
  harmless (only heap tie-order changes, never distances).
- **Why two passes:** buffering edges = 40–60 MB extra ≈ 2.2× memory ≈ 1.17 on
  K = 1.37× on T; pass 1 parses 1 of 3 numbers → ~5 ms. `vec![0u64; m]`
  (calloc/lazy) written through one raw-pointer wrapper; each slot written
  exactly once. Safe alt: `Vec<AtomicU64>` (+5 ms init).

**(b) Layout:** `off: [u32; n+1]` (4 MB) + `edge: [u64; m]` = `(w<<32)|v` (40 MB).
Sorted AoS = 60 MB; split to[]/wt[] touch 2 streams/row — both worse. **SIMD on
the search loop: skip** (degree 5 → lanes empty; AVX2 has no scatter for label
writes; gathers add latency) — expect 0–10%. AVX2's place is the parser.
**Prefetch:** on label improvement, read `off[v]` and `_mm_prefetch(edge[off[v]])`
→ row lands in L2 by pop time if the frontier stays ≲ 20k lines (benchmark).
**Reverse edges:** if the edge multiset is symmetric, the forward CSR serves as
the backward view (save 44 MB + ~20 ms) — verify in pass 2 via
`Σ mix(u,v,w) == Σ mix(v,u,w)` as wrapping u64 sums (collision ~2^-64);
else build a transposed CSR by counting sort on v.

**(c) Bidirectional Dijkstra — radix heap + the meet condition (with proof):**
- Keys u32, items u64 `key<<32|node`; 33 buckets; bucket = 0 if `key == last`
  else `32 - lz(key^last)`; redistribute the first non-empty bucket's min on
  pop; normalize eagerly (`top == last` → O(1) peek); lazy deletion with
  stale-skip; keep Vec capacities across queries.
- **Loop:** `mu = R+1` (or 2^64−2); `tf = F.top`, `tb = B.top`; if
  `tf + tb >= mu: break`; pop the smaller frontier; relax
  `nd = d + w` in **u64 registers**; `if nd < S.dist[v]`: update + push +
  prefetch; **if O.dist[v] finite: mu = min(mu, nd + O.dist[v]) — UNCONDITIONAL,
  even when nd did not improve.**
- **Proof (sketch):** if the loop exits with tf + tb ≥ μ then μ = D: μ is a real
  walk length ≥ D; every x with d(s,x) < tf is forward-settled and every x with
  d(x,t) < tb backward-settled; if μ > D, a shortest path P* forces a later pop
  to compute L_j + w + (D − L_{j+1}) = D ≤ μ, contradiction. Consequences:
  (1) the μ update MUST be unconditional per edge — tying it to "nd improved"
  breaks the proof on ties; (2) "stop when a node is first settled by both
  sides" is NOT a valid exit rule; (3) stale entries only shrink tf → safe.

**(d) Overflow:** u64 labels are unconditionally safe (D ≤ (n−1)·w_max < 2^64).
u32 needs (n−1)·w_max < 2^32 — 10^6 × 10^5 breaks it. **Two sums overflow u32
even when D is small:** `d + w` and `d_f + w + d_b` and `tf + tb` — compute in
u64 registers (free on x86-64). Drop labels with nd > Rcap = min(R, 2^32−1);
stored labels + heap keys then fit u32 (8 B items). **Exactness:** if D ≤ Rcap
the proof runs unchanged on the restricted graph. **Detection:** set a flag when
the prune fires on the 2^32−1 cap (not the semantic R); if flagged and μ > Rcap,
rerun that query with u64 labels (type-generic instantiation).

**(e) Parallel queries:** each thread claims queries 4-at-a-time from an atomic
counter; per-thread scratch = **open-addressing table per direction**
(slot `node<<32|dist`, empty u64::MAX, Fibonacci hash, 2^16 slots = 512 KB,
load ≤ 0.5, reset via touched-slot list O(visited)); 12 threads × 2 × 0.5 MB =
**12 MB vs 96 MB for dense u32[n]** (≈1.4× T-equivalents saved) and stays
L2-resident. Manual itoa, one write. **Use 12 threads** (latency-bound loops
gain from SMT) — benchmark 6 vs 12.
**Totals:** ~44 MB CSR + 12 MB chunk buffers + 12 MB scratch ≈ **70 MB** (115 MB
if the transpose is needed); parse+build ≈ 30–40 ms. If the query phase exceeds
~70% of T, spend remaining hours on the search loop, not the parser.

## 7. Track C — the model's engine (P4)

**(a) Byte-exact oracle — the transition table (defaults for each [S] marked):**
classes Q=`"` C=`,` N=LF R=CR O=other; actions E=end field, Rec=end field+record,
L=literal quote from `""`.

| State | Q | C | N | R | O |
|---|---|---|---|---|---|
| FS | →IQ | E→FS | Rec→FS (blank-line [S]) | →CRP | →UQ |
| UQ | →UQ literal [S] | E→FS | Rec→FS | →CRP | →UQ |
| IQ | →QC | →IQ | →IQ | →IQ | →IQ |
| QC | L→IQ | E→FS | Rec→FS | →CRP | →AC [S] |
| AC | →AC [S] | E→FS | Rec→FS | →CRP | →AC [S] |
| CRP | bare CR [S] | bare CR [S] | Rec→FS (CRLF) | bare CR [S] | bare CR [S] |

- LF/CR inside quotes are data, kept verbatim; `""`→one literal quote; `""""`→one
  quote; trailing space in quotes is data (numeric columns [S]: trim/reject);
  bare CR default = record end (re-dispatch from FS); BOM at 0 skipped once;
  **mid-file BOM default = ordinary data** (if [S] strips: in FS, skip EF BB BF);
  EOF rules per state; header run through the same machine, record 0 dropped.
- Only columns 0, 3, 4 are interpreted; name/note are "scan to next `"`" —
  nothing is unescaped or copied.

**(b) Splitting at record boundaries — quote-parity prepass:**
"inside quotes" == "odd number of `"` seen" (exact while `"` never appears in
UQ/AC). Per fixed chunk (e.g. 2 MiB, thread-count-independent): track par, and
first0/first1 = first terminator seen at par 0 / par 1; sequential pass threads
S=start-state and picks first0/first1 accordingly → cuts = byte AFTER the
terminator. Edges safe (prepass reads ASCII only; CRLF peek past edge; BOM
wholly inside a range). If a literal `"` in UQ is legal ([S]), parse each range
from BOTH hypotheses and keep the one the previous range's end state selects
(2× work). **Differential fuzz:** 1/2/12/13 threads × chunk sizes 1..~300 bytes
so an edge lands on every byte of every BOM/CRLF/`""` (implemented now in
`tooling/csv-boundary-check.py`).

**(c) Money parse (i64 cents):** digits → u64 (≤16 digits needs no checks;
checked beyond), frac2 = first 2 fractional digits (pad zeros), d3 = 3rd digit,
sticky = any nonzero after d3; mag = checked(int·100+frac2); round on the
MAGNITUDE then apply the sign; accumulate sums in i128. **Negative zero:**
integers have none — print the sign only when total < 0 and format with
`unsigned_abs()` (avoids i64::MIN trap). Rounding table ([S] decides the hash):

| input | half-away | half-even | truncate | half-up |
|---|---|---|---|---|
| 0.125 | 0.13 | 0.12 | 0.12 | 0.13 |
| −0.125 | −0.13 | −0.12 | −0.12 | −0.12 |
| 0.005 | 0.01 | 0.00 | 0.00 | 0.01 |
| −0.005 | −0.01 | 0.00 | 0.00 | 0.00 |

Half-away needs only d3 ≥ 5; half-even needs sticky + last-digit parity.

**(d) Join — size-driven, not dogma:** if the lookup is ≤ ~100k rows, aggregate
by region_id FIRST, then join the tiny aggregate once. Inspect the lookup's key
shape for free while parsing it: dense 0..R−1 → direct array (13 MB @u32, beats
everything); both files sorted → merge join with galloping (the ONLY outright
sort-merge win). Otherwise **hash join wins on T and M**: open addressing, linear
probe, fixed Fibonacci multiplier, load 0.4, 2^23 slots ≈ 67 MB (8 B/slot);
shrink the value to a dense group index; **build deterministically** — radix
partition by the top 6 hash bits into 64 partitions, pass A parallel appends,
pass B one thread per partition consuming ranges in order 0..n → insertion order
= file order → the duplicate rule (first/last-wins [S]) holds on every run;
**never CAS-insert** (placement becomes timing-dependent). Probe = DRAM miss
(~80 ns; 67 MB > 24 MB L3) ≈ 22 ms; batch 8–16 records with `_mm_prefetch` →
few ms. Large G aggregation: shared `AtomicI64` + `fetch_add` (integer adds
commute → deterministic). Sort-merge generally = more memory/traffic; not worth
it. Determinism: probes are pure reads; NEVER iterate the table to emit —
collect groups, `sort_unstable_by` on the full output key with a final
tie-break, write once.

## 8. Memory.peak doctrine (P7 — cgroup v2 high-water mark)

Ranked by impact (all sizes are the model's estimates):
1. **Whole-input residency (100+ MB)** — no `fs::read`/`read_to_end`/`lines().collect()`;
   `FileExt::read_at` into per-thread 128–256 KiB buffers, carry the partial tail.
2. **The file's own page cache** — IF charged to our cgroup, no read pattern
   avoids it; levers are `posix_fadvise(DONTNEED)` (FFI, loses warm cache) or
   `O_DIRECT` (`custom_flags(0x4000)`, slow on warm cache). Measure first.
3. **Unbounded pipelines** — no unbounded `mpsc` (reader can buffer the whole
   file); workers claim chunk indices from an `AtomicUsize` inside
   `std::thread::scope`; if a producer is needed, `sync_channel(k)` with a fixed
   ring of recycled buffers; count-then-fill for data that must be kept.
4. **The ×12 multiplier** — budget ≤ ~1 MB working set per thread; merge by
   folding or key-sharding; never a second full copy.
5. **Growth doubling** — `Vec`/`String` realloc transient ≈ 3× old size;
   hashbrown `HashMap` always allocates a new table; use fixed open-addressing
   arrays, `reserve_exact`/`with_capacity` from known bounds; **over-reserving
   is nearly free** (untouched pages are virtual); no unknown-length `collect()`.
6. **glibc malloc hysteresis (10–30 MB)** — freeing a large mmapped chunk raises
   `mmap_threshold`/`trim_threshold`; peak can become the sum of phases.
   Allocate once and reuse (`clear()` + kept buffer); `mallopt` via FFI as a
   stopgap (check the rules' dep counting for an `extern "C"` libc symbol).
7. **glibc arenas × 12 threads (5–20 MB)** — per-thread arenas, free memory not
   shared; the std-only fix is a **`#[global_allocator]` bump arena**: a
   `static ARENA: [u8; N]` (.bss, costs nothing until touched) + thread-local
   slab pointers + a global `AtomicUsize` slab counter; dealloc no-op;
   realloc extends in place when last. Peak = exactly bytes touched, no
   hysteresis, no locks, ~30 lines, hard abort at the limit = dev guard.
8. **Hidden std allocations** — `sort`/`sort_by_key` allocate (use
   `sort_unstable` with full-key tie-break; `select_nth_unstable` for
   percentiles); `to_string`/`format!`/`lines()` per item (parse `&[u8]` in
   place, manual number formatting); `BTreeMap` per-node.
9. **Stack touches** — `[u8; 1<<20]` locals zero-touch 1 MB × 12; keep buffers
   on the heap; `thread::Builder::stack_size(256<<10)`.
10. **Zeroing / THP** — `vec![0; n]` takes the calloc lazy path (good);
    `with_capacity` + `resize(n,0)` touches everything; if THP = `always`, a
    touched byte can commit 2 MiB — `MADV_NOHUGEPAGE` via FFI.
11. **Floors** — Rust baseline ~1–2 MB; memcg charge batching leaves maybe
    ~256 KiB slack per active CPU; **everything written to the 1 GB tmpfs /tmp
    counts** (spilling saves nothing; keep the OUTPUT off tmpfs).

**~15 MB peak budget:** 12×256 KiB read buffers 3 MB + 12×~0.5 MB tables 6 MB +
shared 2 MB + output 64 KiB + baseline ~2 MB + slack 2 MB — only reachable with
1/3/4/5 done; 6/7 keep it from drifting up.
**Calibrate first:** (1) `fn main(){}` baseline; (2) stream with read_at and do
nothing → reveals page-cache charging; (3) with_capacity vs `vec![0; n]` vs
resize; alloc 16 MB then 24 MB to see hysteresis. `strace -f -e
trace=mmap,munmap,brk,mremap,madvise`; sample `memory.current` every 1 ms to
find the spiking phase.