# MODEL OUTPUT DIGEST — A · B · budget (external high-level model, DESIGN-ONLY)

> **Status:** PRE-KICKOFF DESIGN (rule 04/05). Answers to prompts P8, P3, P1 from
> `docs/depth/98-MODEL-PROMPTS.md`, pasted back by the team, distilled here.
> **DESIGN-ONLY:** any code fragments below are design sketches — they live in
> this doc, never in `track-*/src/` before Oct 30 15:00 UTC. Every estimate is
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
2. **/data page cache — RESOLVED by the owner (Oct 27): FREE.** The input is
   pre-warmed with `vmtouch` on the HOST before the container starts, so those
   file pages are charged outside our cgroup — reading /data adds nothing to
   M, and `posix_fadvise(DONTNEED)`/`O_DIRECT` are NOT needed. What DOES
   count: anything WE first pull into the page cache (the output file pages),
   our own allocations, and /tmp tmpfs writes.
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

---

## 9. Determinism under parallelism (P6) — the SHA-zero killer

Required property: **output = f(input) only** — thread count, chunk size,
scheduling and allocator state must never affect the bytes.

**The failure table (source → fix):**

| Source | Fix |
|---|---|
| Partitioning derived from thread count | fixed chunk units claimed via `AtomicUsize` — the unit set never depends on T |
| Completion order (Mutex<Vec>, channels, worker prints) | `results[unit_id]` slots; merge + emit in unit order after the scope |
| Non-commutative merges (first/last-wins, dedup, "first seen" ids) | merge in unit order; tie-break by `(unit, local_seq)` or file offset |
| Float reduction (non-associative) | integers/fixed point; compute rates ONCE from final integer counts |
| Checked arithmetic on partials | `i128` accumulators or wrapping adds + one final range check |
| Heap/sort tie order | total-order keys with a unique last component `(dist, node)`, `(score, doc_id)`; never `partial_cmp().unwrap()` on floats |
| Map iteration (RandomState per process!) | never iterate to emit; collect, sort on the full key, then write |
| Order-sensitive atomics (`fetch_add` term/node ids, CAS first-claimer) | derive ids from keys or prefix sums over fixed units |
| Stale per-thread state | reset discipline per unit + chaos testing that randomizes unit→thread assignment |
| Thread-count-dependent code paths | one code path with identical semantics; test SIMD vs scalar against each other |
| Early exit | define canonical order (e.g. atomic min over unit index) |
| Environment leaks (pointers as keys, ThreadId, uninitialized `set_len`) | don't; fully initialize every buffer you read |
| Output | one writer, one pass, after the join |

**Track notes:** A/C — each record owned by exactly one range starting after a
terminator; prepass cuts independent of T; dedup/last-wins need `(range_idx,
local_seq)`. B — results in `out[query_idx]`; postings ordered by doc id; global
token positions via prefix sums of per-chunk counts; **term ids by first-seen
across threads is a classic leak** — assign from sorted terms. D — distances are
unique so heap ties can't change distance-only output; risks are shared scratch,
`<=` vs `<` at the bound, the unreachable sentinel, u64 accumulation of u32
weights; parallel CSR `fetch_add` placement scrambles adjacency order (harmless
for distances, not for anything order-sensitive — use per-chunk prefix counts).

**Mechanical matrix** (implemented as `tooling/determinism-check.sh`):
```
H = sha256(reference(input))          # a SLOW CORRECT SINGLE-THREAD ref, not your own 1-thread run
for T in [1,2,3,5,6,7,11,12,13,24,32]:          # include oversubscription
  for CHUNK in [prod, 4093, 65537, 1 MiB]:        # odd sizes put edges on every special byte
    for CHAOS in [off, on(seed=1..k)]:            # random yields, shuffled claim order
      repeat N (>=50 with chaos): sha256(run) == H, else keep (T, CHUNK, seed), cmp first differing byte
```
Also: `taskset -c 0-0 / 0-5 / 0-11` (available_parallelism follows affinity);
`stress-ng --cpu 12` background load; `setarch -R` ASLR off; `MALLOC_PERTURB_=165`
for uninitialized reads; debug build `-C overflow-checks=on`; small-input fuzz
with tiny chunks; full-size run on the real file.

**What it will NOT catch** (keep these honest): (1) a consistent wrong answer
(determinism ≠ correctness — the reference is the oracle); (2) rare
interleavings — 50 clean runs bound p ≲ 6% at 95% (rule of three), p = 1%
needs ~300 → chaos matters more than run count; (3) input coverage; (4)
environment differences (kernel/glibc/CPU; `-C target-cpu=native` is a trap —
an AVX-512 build host can emit instructions the target lacks); (5) load/time-
adaptive behavior; (6) UB that happens to be stable; (7) chaos masking — keep
unperturbed runs too; (8) std behavior across toolchains — pin the toolchain
and Cargo.lock; (9) cold-cache / memory-limit effects — test under the same
`--memory`.

## 10. Track A AVX2 micro-design (P2)

- **The output barely needs the timestamp.** Bucket key = first 13 bytes
  (`YYYY-MM-DDTHH`); no calendar math; fixed-width ISO keys sort bytewise.
  Hour-of-day (24 buckets): `h = (b[11]&15)*10 + (b[12]&15)`.
- **Delimiters — one 64-byte window per record** (from `s+28`; covers ≤ 92 B
  lines): `w0 = loadu256(s+28); w1 = loadu256(s+60)`; mask of ' ' and '\n';
  `sp2, sp3, sp4, nl = tz` sequence; checks: `buf[nl]=='\n'`, `sp4 == sp3+4`
  (status exactly 3 bytes), `1 <= nl-sp4-1 <= 10` (latency length), `sp2` in
  [3..7] (method length); ~25 instructions; any failure → scalar slow path
  which re-syncs from the line start (fast path's only state = next-line
  pointer). Buffer padding: ≥ 96 zero bytes at the end, ≥ 16 at the front;
  append a trailing `\n` if the file lacks one.
- **Timestamp validation — one 32-byte compare** (~10 instr): subtract '0',
  digit check via `min_u8(t,9)==t`, separator `cmpeq` vs EXPECT ('-' at 4,7;
  'T' 10; ':' 13,16; '.' 19; 'Z' 26; ' ' 27); blend; `movemask` must be all
  ones on the digit lanes. Optional `mm`/`ss` tens ≤ 5 checks (+3 instr).
  Key = two overlapping u64 loads (`ts[0..8]`, `ts[5..13]`); MRU compare vs
  previous record, else a small hash table (calendar validation once per
  distinct hour). Safe-Rust fallback: four u64s via `from_le_bytes`,
  XOR-with-template + the `0x0606..` digit check (~35 instr).
- **latency_us (≤ 10 digits) → u32 — SWAR ~20 instr:** load u64 at `nl-8`,
  fill garbage lanes with '0', digit-check, subtract '0', then the
  `×2561/×6553601/×42949672960001` pair; if `L > 8` parse the leading 1–2
  digits scalar and `v = hi*10^8 + lo8` in u64; reject `v > u32::MAX`.
  SSE4.1 maddubs alternative (~12 instr, branch-free) — adopt only if it
  beats SWAR on a bench. Status: `is_err = b[sp3+1] == b'5'` (valid because
  the `sp4 == sp3+4` check enforces 3 digits).
- **Per-record budget (~120 instr AVX2):** delimiters 25, ts validate + key
  cache 20, method length 4, endpoint hash + exact-compare (~30; the 1,842-
  entry table is L2-resident), status 5, latency SWAR 20, group/counter 10,
  loop 5. At 12 threads on 6 P-cores ≈ 10–15 ms compute vs a DRAM+copy floor
  ≈ 15–25 ms for ~220 MB → **under ~150 instr/record you are bandwidth-bound**;
  next gains are avoiding copies and the endpoint lookup. **Decide count-then-
  fill vs two passes BEFORE tuning** (12.4 MB of latencies is most of a
  ~20 MB target).
- **Fallback placement:** chunk-level AVX2 vs scalar selection at startup
  (`is_x86_feature_detected!("avx2")`), per chunk not per record; the scalar
  chunk path is the reference of record; `#[cold] #[inline(never)]
  parse_record_slow(line)` implements full reference semantics — the fast path
  is only a recognizer that must match or bail. Unsafe surface: the padding
  invariant justifies `_mm256_loadu_si256`. Minimum unsafe set if budget is
  tight: a 3-intrinsic delimiter scan (SWAR loses most there, 4–6×).
  Differential fuzz fast vs slow on every line + single-byte mutations.

## 11. The 72-hour battle plan (P9)

**The score table (S=D=1):** 25×/5× → speed 41.9 (27 pts); 100×/12× → 60.8
(40); 300×/12× → 72.7 (47); 1000×/12× → 85.8 (56). **"25× and 5×" is a weak
target — winners are bounded by the I/O + startup floor, not the reference.**
Elasticities: 1% less time ≈ +0.11 speed pts; halving M ≈ +3.0; doubling code
size ≈ −2.3 (a change adding 10% lines must save ≥ 3% runtime to break even).
Overall title = geomean of best 3 → **lift your weakest of the three first**
(marginal value ∝ 1/total). Judge = 35% — the write-up may pay more per hour
than late micro-optimization.

**Shape:** Now→Oct 30: build the generic tooling (harness, generators, slow
oracles, scoreboard, floor baselines, the P6 runner). H0–2: read specs +
reference; write down the EXACT output/rounding/tie/order rules; pick three
tracks. **H2–12: SHA-green single-thread baselines on all three tracks BEFORE
optimizing — a slow green beats a fast gate failure; submit early if
resubmission is allowed.** H12–50: rungs round-robin by marginal points/hour
weighted by 1/total; tag every green rung, revert on red. H50–60: P6 + P7
hardening under the exact container flags. H60–72: freeze features, write up,
final timing. Two sleeps ≥ 6 h before H48, each after a green commit. Measure
after every rung: hash at 1/6/12 threads, 5-run middle-3 wall, memory.peak,
`perf stat` (instructions/cycles/IPC/faults), gap to floor, predicted speed.

**Per-track rungs (estimates; measure each):**
- **A:** ① 12-thread fixed-chunk pread + merge (6–10×) ② compact storage
  count-then-fill (M ÷3–5) ③ AVX2 delimiter window (2–3×) ④ endpoint
  open-addressing intern (1.5–2×) ⑤ ts validate + key cache (1.3×) ⑥ SWAR
  latency (1.2×) ⑦ memory pass (M ÷2–4, +3–6 pts) ⑧ radix select instead of
  full sort on big groups (1.2–1.5×) ⑨ parallel merge, one write (1.1–1.3×)
  ⑩ build flags. Stop at ≈1.5× the pread+startup floor or < 3% per rung.
- **B:** ① **query-aware index** (postings only for query terms; positions only
  for phrase terms — the big build cut) ② parallel build over document-aligned
  fixed chunks, global postings = chunk-local lists concatenated in chunk order
  (4–8×, no sort) ③ fast tokenizer, no Strings (2–3×) ④ count-then-fill
  postings ⑤ hybrid bitmaps (dense/stopwords) + u32 arrays (2–5× on BOOL)
  ⑥ **prefix-trie memoization** — 50k left-associative queries share prefixes,
  evaluate each once ⑦ phrase = doc-level AND first, then position merge
  anchored on the rarest term (2–10×) ⑧ reorder pure-AND runs smallest-first
  ⑨ parallel queries `out[query_idx]` ⑩ preformatted output, one write.
  Stop when build ≈ 1.5× the tokenize floor and queries < 10% of wall.
- **C:** ① parity prepass + fixed-range parallel parse (6–10×) ② fast path
  for name/note (skip-only; interpret columns 0/3/4) (1.5–2×) ③ SWAR money
  (1.2×) ④ join: direct array if dense else partitioned parallel build
  (2–4×) ⑤ lookup compaction (M ÷2–3) ⑥ aggregation per-thread vs atomics by
  group count ⑦ memory-sized pread buffers + sort_unstable full-key ⑧
  clmul prefix-XOR quote masks only if quoted fields dominate ⑨ one write ⑩
  flags. **Re-run the whole differential fuzz after EVERY rung**; if not clean
  well before H48, drop C and keep the other three.
- **D:** ① parallel newline-aligned parse + SWAR u32 (10–20× on parse) ②
  parallel CSR via atomics — **the one place the determinism rule relaxes**
  (distance-only output) (3–6×) ③ compact (v,w) pairs 8 B + u32 offsets ④
  per-query touched-node maps, not O(n) clears (M ÷several) ⑤ packed u64
  heap entries, lazy deletion, early exit (1.3–2×) ⑥ parallel queries ⑦
  prefetch heap-top adjacency ⑧ AVX2 delimiter scan (1.5–2×) ⑨ lazy zero
  pages ⑩ flags. **Skip bidirectional/CH unless queries > ~20% of wall** (the
  reverse CSR is a second build; the hint says CH rarely repays).

**Stop rule:** per-rung value ≈ A first 2× = 3–4 h → +4.9 pts (~1.4 pts/h);
next 1.5× ≈ 3 h → +2.9 (~1 pt/h); later 1.2× ≈ 3 h → +1.3 (~0.4 pts/h).
Weight by 1/total; **stop a track below ~0.4 pts/h, within ~1.5× of its floor,
or when a rung needs ≥ 10% more code for < 3% time** — move the hour to the
next-best rung or the write-up. If the fourth track beats your third at any
point, it takes its place.
---

## 12. STARTER ORACLE RESOLUTIONS — the DEFERRED ledger is closed (Oct 27)

The organizers' starter template (hackathon-raptors/speed-demon-starter)
ships the NAIVE REFERENCE implementations for all four tracks. Reading them
resolves the output contract completely — spec_intake now reports
0 blocking DEFERRED values. The board contract also became precise:

**Board contract (from the starter + Discord):**
- Folders: track-a .. track-d (lowercase — renamed to match).
- .speed-demon.toml is BOARD-PARSED with exactly these keys:
  track = "a", team, language, dependency_count (the D-axis key —
  harness updated to read dependency_count).
- Output filenames: summary.txt (A), results.txt (B), aggregates.txt (C),
  distances.txt (D) — renamed everywhere.
- One repo per team, speed-demon branch; the board builds every track-*
  folder; an unchanged folder is the reference itself (reported unchanged).
- Board time cap = 3x the reference; final scoring = 10x. Optimize under 3x
  for live-board numbers; 10x is the real gate.
- check.py (starter): builds with --network=none, runs under the board's
  sandbox, prints the output sha256; --expected <hash> fails on a wrong
  answer. Our pin-hash.sh + e2e + context-replay cover the same contract.
- Generators are PUBLIC (hackathon-raptors/speed-demon-2026): seed 1 at
  full scale = the board dataset, sample = 10%. Expected sample hashes
  publish Oct 30 06:00.

**Track A (from a_ingest.py):** input requests.log; group (UTC hour,
endpoint); p50/p95/p99 NEAREST RANK, rank = ceil(p*n/100) = (p*n+99)//100,
index rank-1; errors = 5xx count + ppm = errs*1_000_000//count (integer
floor); sorted by (hour, endpoint) (endpoint bytewise); single-space
separator; trailing newline every line; hour rendered %Y-%m-%dT%H:00:00Z;
method ignored; latency_us int. Input is LF-only.

**Track B (from b_index.py):** inputs docs.txt (id<TAB>text, ids 0..N-1 in
order — a violation raises) + queries.txt; output results.txt. Token =
maximal run of ASCII [a-z0-9] after ASCII-lowercasing (bytes >= 0x80 are
separators). Queries: BOOL t1 OP t2 ... / PHRASE "t1 t2 t3". BOOLEAN =
left-to-right set ops (AND=intersection, OR=union, NOT=difference, no
precedence). PHRASE = docs containing every term, then an exact adjacency
scan of the re-tokenized text (no positions kept). Result line: count sum
up-to-10-smallest-ids, space-joined; empty -> 0 0. Stopwords retained.

**Track C (from c_join.py):** inputs records.csv + regions.csv (header
region_id,country); output aggregates.txt. Quoting = Python csv module,
excel dialect (comma sep, double-quote, "" escape, QUOTE_MINIMAL; CRLF and
LF both terminate; CR/LF inside quotes kept). U+FEFF stripped from the start
of the FIRST FIELD of every record (file start or after a terminator; the
first field is never quoted). Amounts: exactly 2 decimals
((-?)([0-9]+).([0-9]{2})), no rounding. Join region_id->country; unknown ->
_UNMATCHED. Per-country aggregate: count id_sum amount_sum amount_min
amount_max distinct_names name_bytes note_bytes (name/note bytes = UTF-8
length); sorted by country (code-point = UTF-8 byte order); space-joined,
one line per country, trailing newline.

**Track D (from d_traverse.py):** inputs graph.txt (n m header, u v w u32,
not sorted by u, ids dense 0-indexed, directed, reverse edges listed
separately) + queries.txt (s t per line); output distances.txt.
Distance-only, one integer per query line in query order; unreachable = -1;
s == t = 0. Reference is per-query Dijkstra with early exit at the target;
distances are Python ints (unbounded) -> implementation must carry u64
internally. Parallel edges and zero weights fine.

**Consequences for the in-window code:** the percentile "definition" debate
is settled (nearest-rank ceil); the error rate is integer ppm, not a float;
B needs NO positions (adjacency scan) and NO stopword list; C's oracle must
reproduce the Python csv dialect + the per-record BOM strip; D is
distance-only with -1. The deep dives' P0 tables are now pinned facts, not
guesses.

---

## 13. OWNER CLARIFICATIONS (official Discord, Oct 27) — the grey areas are closed

**Q1 — does a DSL-as-library count as a dependency?**
"Yes. D is just the count of direct non-stdlib runtime dependencies in your
manifest, whatever the library does, so an effects DSL pulled in as a
dependency counts like any other: one dependency is a 0.90 factor on your
score. Write the effects layer yourself and it's your own source, so it lands
in S instead (capped at 1.0, so source can cost you but never win it). You've
read the thread-pool case right: since S can't pay, pulling one in only costs
you on D, so there's no reason to."

- Consequence: D = 1 + direct deps is the WHOLE story; no library category is
  exempt. Every crate = K x 0.90. Std-only stays optimal, always.
- "Write it yourself → it lands in S" — our prep/ kit (bump/lines/parse_u32/
  itoa) is exactly that: generic primitives as OUR source, not a dependency.

**Q2 — which peak-memory counter, and do /data pages count?**
"It's the container's cgroup memory.peak (cgroup v2), read on the host after
your container exits, not ru_maxrss. So M is the peak of everything charged
to your run's own cgroup: what your process allocates, what you write into
the 1 GB /tmp tmpfs, and any page your process is first to pull into the page
cache. The input in /data does not count: it's pre-warmed into the page cache
with vmtouch on the host before your container starts, so those file pages
are already charged outside your cgroup."

- Consequence 1 (M counter): our harness's cgroup.rs already reads
  `/sys/fs/cgroup/.../memory.peak` — correct per the owner. Never measure with
  ru_maxrss or `ps` RSS.
- Consequence 2 (/data free): buffered reads of /data cost ZERO M — no
  fadvise/O_DIRECT games needed; read buffers are purely a T decision.
- Consequence 3 (what DOES count): our own heap, /tmp tmpfs writes, AND the
  OUTPUT FILE's first-touched pages. Keep the output write minimal and
  single-shot (it is — one file, hand-formatted, one write_all). Never spill
  intermediates to /tmp (counts AND defeats the point of scratchless).

---

## 14. OFFICIAL PAGE — FULL DUMP (Oct 27/28) — the last new facts

Fetched/confirmed from the event page and Discord. All are now encoded:

**Change #23 — Rust PGO is possible offline.** The allowlist gained
`halloumee/speed-demon-rust:1.99.1-trixie-llvm-tools@sha256:13e4ee7135d8eac6c56476c7d1aee201f36479f4745fd5d0e6b4529f4e13251a`
= `rust:1-trixie` (same compiler, 1.99.1) **with the llvm-tools component**, so
`llvm-profdata` matches `rustc`. Build time is NOT scored ("spend six of your
ten minutes on PGO, that is free"). Recipe + cost model + risk controls:
`93-PGO-AND-CODEGEN.md`. Decision tool: `k_whatif.py --tradeoff`.

**Change #24 — native CPU builds are supported.** "A track that compiles for
the native CPU is built on the scoring box itself, so native means the scoring
CPU, a Core i5-13500 with AVX2 and no AVX-512." Our `build.sh` now carries
`-C target-cpu=x86-64-v3` (AVX2/BMI2/FMA — exactly the scoring CPU's level,
risk-free on any modern host); `native` is a supported H24+ option.

**Judge split (the 35%).** Code quality **15%**, technique **12%**, PERF.md
**8%** — "would you merge this", "is the approach considered", "measurements
not adjectives, each step citing the live-board run ID". Judges see code +
write-up + **live-board run history**, and they never see final scores before
submitting forms.

**Deliverables per track.** Dockerfile · PERF.md · README.md ·
.speed-demon.toml · src/ · plus **demo.mp4 (optional, < 3 min, unscored)**; the
repo carries the OSI licence and goes public at freeze. Storyboard:
`../DEMO-STORYBOARD.md`.

**Other confirmations that matched what we had:** 5 runs middle-3, >5% spread
re-runs, >2% reference drift voids a batch, serialized runs, `--memory=8g
--memory-swap=8g` disables swap, container fs diff after every run, one output
file named by the spec, `io_uring` blocked but `mmap` works, ties broken by
raw K then wall time then earliest commit, overall = geometric mean of best
three, `$50` highest-K per track (specialist-friendly), write-up side quest
`$100 x2`.
