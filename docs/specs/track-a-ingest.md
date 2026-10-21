# TRACK A · INGEST — Design Specification

Status: pre-kickoff design document. No solution code, no function bodies, no implementation language.
Track window: baseline H0–H17 (2h charged to Track A), optimization block H18–H28 (10h charged).
Reference: T 41.2s · M 2940MB · S 3180B · D 1 · timeout 412s · dataset 3.1M lines / 312MB.
Scoring reminder: K = (Tref/T)^0.40 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15 × (Xref/X)^0.10, each ratio capped at 50×; speed_score = 100·ln(K)/ln(25), clamped [0,100].

---

## 1. Reference Teardown

**What the reference does (from the frozen brief).** Read lines, split on delimiters, parse timestamps with a date library, push latencies into a growing per-endpoint list, sort each list at the end.

**Measured reference axes.** T = 41.2s, M = 2940MB, S = 3180B, D = 1, X = DEFERRED to Oct 28 frozen spec.

**Structural facts we can exploit (all from the frozen brief, not invented).**
- 3.1M records, 312MB → ~100 bytes/record average.
- 1,842 distinct endpoints, heavy Zipf tail → a small dictionary, highly skewed lookup frequencies.
- Fields: `ts` (RFC3339, microsecond precision), `method` (enum, 7 values), `endpoint` (variable length), `status` (uint16), `latency_us` (uint32, MICROSECONDS — the unit is a trap for anyone reading it as milliseconds).
- ts is monotonic *per shard* but interleaved *across shards* → within a shard the stream is sorted; across shards it is not. If the dataset is delivered as multiple shard files, per-file order is monotonic and global order is not. Line ordering of the OUTPUT is therefore not derivable from input order; it must be an explicit sort (spec DEFERRED: exact output ordering and float formatting).
- Reference M = 2940MB = 9.4× the 312MB input size. Whatever the reference allocates, it holds far more than the data. Our design goal is structural, not incremental: **no structure in our design grows with the input except the latency array, and that array is 4 bytes per record (12.4MB for 3.1M records).**

**Where the 41.2s probably goes (HYPOTHESIS, to be measured in the H1–H2 baseline window; no absolute seconds are claimed).**

| Component | Hypothesis share of T | Structural reason |
|---|---|---|
| Date-library timestamp parse | 35–45% | General-purpose civil-time parsing per record, with allocation and calendar logic per call |
| Record split + field extraction | 15–25% | Generic delimited reader, per-line tokenizer with per-token overhead |
| Per-endpoint latency list growth | 15–25% | 1,842 separately-grown vectors, repeated reallocation and copy |
| Final per-endpoint sort | 15–25% | Comparison sort over every list, including the Zipf head list |
| Endpoint dictionary + string handling | 5–10% | Per-record endpoint key hashing/compare against string-keyed map |

The organizer hints name the same three hot spots: custom line parsing, percentiles without sorting everything, and the allocator. We treat the allocator hypothesis as the memory story (M axis) and the timestamp hypothesis as the second-biggest time cost.

**Gate implications for this track.**
- Gate 1 (hash) is binary: a record-ordering difference or a float-formatting difference is a zero. Percentile definition is DEFERRED to the Oct 28 frozen spec (nearest-rank vs interpolated; which index; how 0 endpoints or empty lists are spelled). This is the single largest correctness exposure in the track.
- Gate 2 (speed floor) only constrains the overall title, not the track score, but a 10× floor means a sub-10× entry is worthless; our Commit anchor is 30×.
- Gate 3 timeout is 412s. Our target runtime is 1–3s, i.e. >100× headroom. The only realistic timeout failure is a hang caused by a parser bug on malformed input, not slowness.
- Gate 4 (sandbox) forbids writing to /data (read-only mount) and forbids precomputed answers in the image. Consequence: **scratchless design** — no temp files, no /tmp staging, because a 1GB tmpfs staging copy of the dataset would also inflate `memory.peak` and destroy the M axis.
- Reference drift: the reference is re-run before and after every batch and a >2% drift voids the batch. All our T ratios must be computed against a reference measured in the same session; never against a stale number copied into a spreadsheet. Our own measurement protocol mirrors the official one: 5 runs, discard max and min, mean of middle 3.

---

## 2. Axis Budget

**Anchor rows (the campaign's committed targets).**

| Row | T ratio | M ratio | S ratio | D ratio | X ratio |
|---|---|---|---|---|---|
| Safe | 20× | 10× | 0.60× | 1.00× | 1.00× |
| Commit | 30× | 15× | 0.62× | 1.00× | 1.05× |
| Stretch | 45× | 22× | 0.65× | 1.00× | 1.10× |

**Full arithmetic for each row.**

- Safe: T 20× ^0.40 = 3.3145; M 10× ^0.20 = 1.5849; S 0.60× ^0.15 = 0.9262; D 1× ^0.15 = 1.0000; X 1.00× ^0.10 = 1.0000.
  K = 3.3145 × 1.5849 × 0.9262 × 1.0000 × 1.0000 = **4.8656**.
  speed = 100·ln(4.8656)/ln(25) = 100 × 1.58246/3.21888 = **49.15**.
  Absolute read: T = 41.2/20 = 2.06s, M = 2940/10 = 294MB.
- Commit: T 30× ^0.40 = 3.8981; M 15× ^0.20 = 1.7188; S 0.62× ^0.15 = 0.9308; D 1.00×; X 1.05× ^0.10 = 1.0049.
  K = 3.8981 × 1.7188 × 0.9308 × 1.0000 × 1.0049 = **6.2668**.
  speed = 100·ln(6.2668)/ln(25) = 100 × 1.83526/3.21888 = **57.02**.
  Absolute read: T = 41.2/30 = 1.373s, M = 2940/15 = 196MB.
- Stretch: T 45× ^0.40 = 4.5844; M 22× ^0.20 = 1.8556; S 0.65× ^0.15 = 0.9374; D 1.00×; X 1.10× ^0.10 = 1.0096.
  K = 4.5844 × 1.8556 × 0.9374 × 1.0000 × 1.0096 = **8.0509**.
  speed = 100·ln(8.0509)/ln(25) = 100 × 2.08622/3.21888 = **64.80**.
  Absolute read: T = 41.2/45 = 0.916s, M = 2940/22 = 134MB.

**Sensitivity: which axis is worth working hours on.** From ln K = 0.40·ln(T ratio) + 0.20·ln(M ratio) + 0.15·ln(S ratio) + 0.15·ln(D ratio) + 0.10·ln(X ratio):

- Doubling the T ratio adds 0.40·ln2 = 0.2773 to ln K → K ×1.3195 → speed +6.7 points.
- Doubling the M ratio adds 0.20·ln2 = 0.1386 → K ×1.1487 → speed +2.9 points.
- Increasing source size by 1.5× costs 0.15·ln1.5 = 0.0608 → K ×0.9410 → speed −1.5 points.
- Increasing source size by 2.0× costs 0.15·ln2 = 0.1039 → K ×0.9013 → speed −2.1 points.
- The D axis: D = 1 + non-stdlib runtime deps + vendored bytes/50,000. Rust std-only ⇒ D = 1.000 exactly ⇒ 1.0000 factor, i.e. 0.15·ln(1) = 0 left on the table. The published reference is also D = 1, so this axis is a tie, not a win. Any dependency (memchr, rayon, a date crate, libc for mmap) would push D above 1 and cost 0.15·ln(D) of K — e.g. one vendored 200KB dependency adds 4 to D, costing 0.15·ln(5) = 0.241 → K ×0.786 → speed −4.9 points. **Decision: std-only, no crates, no vendored source, no FFI.** `core::arch` intrinsics are in-tree and do not change D.
- **Exchange rate: source bytes are cheap, time is expensive.** A 2× larger source costs 2.1 points; buying 1.5× more T speed (say 20× → 30×) gains 0.40·ln(1.5) = 0.1622 → K ×1.176 → speed +3.3 points. Conclusion: **spend source bytes to buy speed, never the reverse.** The Commit row deliberately sits at S 0.62 (≈1.61× the reference's 3,180 normalized bytes ≈ 5,130 bytes) while the T target is 30×.

**Why M should land far above the 15× target.** Design peak memory budget (Commit):

| Structure | Bytes |
|---|---|
| Per-thread input buffers, 12 threads × 2MB | 25.2 MB |
| Per-thread (endpoint-id, latency) pair arrays, 12 × 2 × 3.1M × 4B | 297.6 MB |
| Flat per-endpoint latency arena (one copy) | 12.4 MB |
| Radix-sort scratch | 12.4 MB |
| Endpoint dictionary + term table (1,842 entries) | < 1 MB |
| **Total** | **≈ 349 MB** |

That budget is wrong as written: the per-thread pair arrays are the dominant term and they exist only to merge. They can be avoided by having each thread write into a *shared* arena with atomic reservation of per-endpoint blocks is complex; the simpler fix is to have each thread own a contiguous slice of a single global (endpoint-id, latency) array by first computing per-thread per-endpoint counts — but counts require a first pass. Two options, both cheap:
- **Option 1 (two-pass, no merge):** pass 1 counts records per endpoint per thread (streaming, no latency array), prefix-sum into per-thread per-endpoint slots, pass 2 scatters latencies directly into their final per-endpoint slot. Cost: parsing the file twice (T penalty) but zero merge. Not attractive.
- **Option 2 (one pass, per-thread arenas, merge at the end):** each thread keeps its own (id, latency) pair array; after the pass, a merge stage groups by endpoint and writes the final arena. Peak = per-thread arrays (297.6MB as above) + arena + scratch. To cut this, store the pair as a single u64 (id in the high 32 bits, latency in the low 32 bits) → 12 × 3.1M × 8B = 297.6MB is exactly what that is. Still 297MB.
- **Option 3 (preferred):** each thread writes latencies into a per-endpoint *growable* structure but with a **two-level index**: 1,842 endpoint slots, each holding a chunked list of fixed-capacity blocks (e.g. 4,096 latencies per block) allocated from a global bump arena. Per-thread chunks are never merged in the classic sense: a per-endpoint block list is a small array of block pointers; blocks are filled in arrival order; per-endpoint latency order is irrelevant to us because we sort for quantiles anyway. The **latency is the only payload**; the endpoint id lives in the block list index, not in the data. Peak = 3.1M × 4B = 12.4MB of blocks + 1,842 × (block pointer array) + block metadata. With a 4,096-entry block = 16KB, the Zipf head endpoint needs ~blocks of 16KB each; total block overhead is 3.1M×4B plus ≤1 block per endpoint per thread. **Peak ≈ 25.2MB (buffers) + 12.4MB (blocks) + 1.5MB (index) ≈ 40MB.** M ratio = 2940/40 = 73× → capped at 50× → M factor = 50^0.20 = **2.1867**, better than the Commit target of 1.7188.

Recomputed M ceiling for the Commit row if M lands at 50×: K = 3.8981 × 2.1867 × 0.9308 × 1.0000 × 1.0049 = 8.4662 → speed = 100·ln(8.4662)/3.21888 = 100 × 2.13624/3.21888 = **66.37**. We therefore plan against the 15× target but treat 50× as the realistic M outcome, and we do NOT build the plan around the extra 0.6 points.

**Page-cache caveat (must be measured at H1).** If the read-only dataset's page cache is charged to the container's cgroup, `memory.peak` will include up to 312MB of file-backed pages and the M ratio collapses to ~2940/340 ≈ 8.6× (M factor 1.5442, K ×0.898 → −2.0 points). Mitigation is bounded, non-overlapping per-thread buffers and sequential reads (bounded readahead), and an immediate H1 measurement. If page cache is charged, we do not chase M further; we re-anchor the axis table and take the 2-point loss. This is tracked as risk R-A2.

---

## 3. Algorithm & Data Layout

**Pipeline shape (5 stages, all described at the level of decisions, not code).**

1. **Range split.** Partition the input into 12 ranges (one per hardware thread) on byte boundaries. For each range, locate the first record boundary by scanning forward to the next newline; the record that straddles the range start is handled by the *previous* range. Boundary discovery is a byte scan, not a parse.
2. **Per-thread streaming parse.** Each thread reads its range through its own 2MB buffer, seeking to the next newline at buffer end. Records are parsed in-place; no line is ever copied.
3. **Field extraction.** For each record: locate the four field delimiters; `ts` is fixed-width RFC3339 (parse digits positionally, no date library, no allocation); `method` is resolved from a 7-entry table keyed by the token bytes; `endpoint` is hashed and looked up in a per-thread copy of the dictionary to obtain a 32-bit endpoint id; `status` is parsed as digits; `latency_us` is parsed as digits. Everything is integer arithmetic; no floating point appears anywhere in the pipeline.
4. **Endpoint dictionary.** One shared dictionary (1,842 entries) with an open-addressed table, sized to a power of two ≥ 4× the entry count, storing a 64-bit hash, the endpoint byte range (offset+length into a concatenated endpoint arena) and the assigned id. Lookups compare the stored hash first and only then the bytes, so a hit usually costs one hash plus a short compare. The dictionary is built dynamically as endpoints are first seen; ids are assigned in first-seen order and are then re-keyed to a canonical order for output (output order is DEFERRED to the Oct 28 spec).
5. **Latency accumulation.** As described in §2 Option 3: per-endpoint chunked block lists over a global bump arena. Bump allocation is per-thread (each thread owns its own arena region) so there is no cross-thread allocator contention — this is the "where most teams discover their allocator" hint answered head-on.
6. **Quantiles.** After the parse, each endpoint's latency set is materialized contiguously (blocks are copied into one flat array in endpoint order, or sorted in place per block-group) and then ordered. Ordering strategy is chosen in §5 (L4).

**Timestamp handling (the designated second-biggest cost).**
- The format is fixed-width: `YYYY-MM-DDTHH:MM:SS.ffffffZ`. Every field has a known digit count, so parsing is pure positional digit accumulation — no delimiter scanning, no sign handling, no validation branches in the hot path.
- Converting a calendar timestamp to a number is a pure integer problem: days-from-civil via a branch-light day-count computation (the standard civil-days algorithm: accumulate days by year with a leap-year correction, then by month with a cumulative-day table, then add the day-of-month offset). No library, no division by non-constant divisors in the hot path.
- The month offset table is 12 constant values; the leap-year rule is the standard one. Microseconds are carried as a 6-digit fraction multiplied by 10^0..10^5 as a constant table lookup rather than by repeated multiply-add.
- **Hour bucketing:** the error-rate buckets are by hour. The bucket index is derived from the timestamp as (absolute seconds since epoch) divided by 3600 — an exact integer division by a constant, and since the epoch is hour-aligned the bucket boundaries are exact. The per-thread state keeps a "current hour bucket" and the hour boundaries are monotone per shard, so a per-thread counter array indexed by bucket is enough; the bucket index itself is needed only for the output rows, and the number of distinct hours is small (the dataset spans a single day per the sample timestamps; DEFERRED to the Oct 28 spec for the exact bucket range and spelling).
- **Monotonicity shortcut:** ts is monotonic per shard. A per-thread "last parsed timestamp" lets the parser detect the hour rollover and skip re-deriving the date for the majority of records — only the time-of-day part changes within an hour. This is the single cheapest timestamp win and it is legal because monotonicity is stated in the frozen brief.
- **Validation is not free but is required:** the hash gate means a mis-parse is a zero. We validate the fixed-width structure cheaply (digit-range checks on the fixed positions) and count violations; a nonzero violation count is a hard failure signal during development, not a runtime branch in the release path.

**Endpoint dictionary sizing and the Zipf tail.**
- 1,842 distinct endpoints is tiny. The dictionary is ~1,842 × (8B hash + 8B span + 4B id + padding) ≈ 40KB, plus a 4,096-slot table of 4B/8B indices ≈ 32KB, plus the endpoint arena (1,842 × ~20B ≈ 37KB). Total < 150KB, cache-resident.
- Zipf skew means the hottest endpoint appears a large fraction of the time. A one-entry "last endpoint" memo (compare the incoming token bytes against the last id's bytes) catches the most frequent repeat pattern for free; the hash lookup handles the rest.
- Because the endpoint field is variable-length, the token compare is a bounded memcmp of the candidate bytes; the hash check rejects almost all mismatches before the compare.

**Memory layout principles.**
- All hot per-record state is in flat, index-addressed arrays: no `HashMap` on the hot path, no per-record allocation, no `Vec` growth in the hot path.
- Buffers are per-thread and fixed-size; total 25.2MB.
- The latency payload is 4 bytes per record; blocks are 16KB; the arena is bump-allocated per thread.
- No output buffering of the whole result: the output is written incrementally to /out (the expected size is small — 1,842 endpoint rows plus hourly rows).

**Output.**
- Rows are produced in a deterministic order: endpoints sorted by the ordering the Oct 28 spec pins (assumed: by endpoint name; DEFERRED), hourly rows sorted by bucket index.
- Float formatting: the error rate is the only float-like value; DEFERRED as to the exact format (percentage vs fraction, decimal places). Because the gate is a byte hash, the format is a single-point-of-change module, and the H0–H2 baseline exists partly to pin it against the sample's published hash.
- Latency percentiles are emitted as integers (the source data is integer microseconds), so no float formatting risk on that axis.

---

## 4. Hash-Gate Correctness & Determinism

**The rule we adopt: output must be a pure function of the input bytes.** No dependence on thread count, thread scheduling, allocation addresses, iteration order of a hash container, wall-clock time, or environment.

**Determinism hazards and their specific eliminations.**

| Hazard | Elimination |
|---|---|
| Per-thread partial aggregates merged in completion order | Merge in fixed thread index order; every merge step is an integer add |
| Floating-point accumulation order | No float accumulation anywhere: counts are integers, error rate is computed once at output time as a single division of two integers |
| Hash container iteration order | We use our own open-addressed table with a fixed probe sequence; ids are assigned in first-seen order, and output ordering is an explicit sort, not table order |
| Endpoint id assignment differing by thread scheduling | Endpoint ids are assigned from a single global dictionary protected by a lock, so id assignment order is the order of first insertion globally. If the Oct 28 spec pins output by name, ids are internal only and order is irrelevant; if it pins output by id, we must re-key to a canonical order before output. **DEFERRED to Oct 28** — the design carries a "re-key to canonical order" step that is enabled only if needed, so it costs nothing in the common case |
| Sort instability affecting equal keys | Quantile selection is by value; equal latencies are indistinguishable. Where a stable order is required (e.g. emitting a stable list), sort by the full key tuple, never by pointer or by hash |
| Timestamp parse differences from local timezone | All time handling is UTC-only integer arithmetic; the dataset's `Z` suffix is fixed |
| Locale-dependent formatting | No locale-sensitive formatting anywhere; decimal formatting is hand-rolled digit emission |
| Uninitialized memory affecting output | No uninitialized reads; every output field is written on every path |

**Verification ladder (budgeted, because Track C's warning — "most disqualifications happen in the correctness track" — applies here too).**
- **V1 (baseline, H0–H2):** the 10% sample ships with its expected hash. The baseline implementation must reproduce that hash exactly before any optimization is attempted. This is a hard gate on the whole track: no optimization starts before V1 passes.
- **V2 (differential oracle):** a second, deliberately naive implementation (the "reference-style" one: split on delimiters, full sort per endpoint, library-style date handling) is written in the baseline window and kept as an oracle. Every optimization stage must agree with the oracle on the sample, and later on the full dataset, on every field — not just on the final hash, but field-by-field, so a mismatch localizes the bug.
- **V3 (thread-count invariance):** the same binary is run with 1, 6, and 12 workers on the sample and the full dataset, and the SHA-256 of the output must be identical. This is the direct test of the "nondeterminism is a correctness bug" rule. Any mismatch is a stop-the-line event.
- **V4 (repeated-run invariance):** 5 consecutive runs, identical hashes, and the run-to-run spread is checked against the 5% re-run threshold so our own measurements do not drift into the reference-drift failure mode.
- **V5 (fuzz):** mutation testing on the sample — truncate at every byte offset in a window, corrupt delimiters, drop the final newline, duplicate a record, re-encode a timestamp with a bad digit — and require that the program either produces a valid output or fails loudly (violation count nonzero), and never panics, hangs, or produces nondeterministic bytes. A hang is a failed run (Gate 3), so a panic-free path matters more than any single speed.
- **V6 (format isolation):** the exact output byte format is produced by one module, and the sample hash is used to confirm the module's choices (ordering, decimal places, row separators, trailing newline) before they are relied upon.

**Gate 4 compliance (sandbox).** The program reads only from the dataset mount, writes only to /out, uses no network, creates no files in the image, and never writes to /tmp (a tmpfs copy of the 312MB input would both violate the spirit of the scratch rule and destroy the M axis via `memory.peak` on tmpfs pages). No answers are precomputed into the image: the program computes everything from the mounted dataset at run time. The dataset mount is read-only and is opened read-only; we never attempt a write to it.

**Gate 3 compliance.** Target runtime is 1–3s against a 412s kill. A hang is the only realistic failure; the parser therefore never loops on a "find next field" without a guaranteed forward-progress condition, and the range-split logic guarantees every byte is either parsed or belongs to a boundary record that is parsed exactly once.

**Reference-drift hygiene.** Ratios in §2 are computed against a reference measured in the same batch. The reference is re-run before and after each batch by the organizers; if it drifts >2% the batch is void. We therefore log our reference timings with the same 5-run protocol and re-derive ratios from the batch-local reference, never from a value pasted in a document.

---

## 5. Optimization Ladder

Each rung states the hypothesis, the expected axis effect, the risk, and the acceptance rule. Acceptance rule for all rungs: the 10%-sample hash still matches, the full-dataset hash still matches, the field-by-field oracle diff is empty, and the measured mean of the middle 3 of 5 runs improves by ≥3%. A rung that fails any correctness check is reverted immediately, not debugged in place.

| # | Rung | Hypothesis / expected effect | Risk | Acceptance |
|---|---|---|---|---|
| L0 | Baseline: delimiter split + sort-based percentiles, std-only, no threads | Establishes the oracle and the V1 hash; expected to land near the reference's structure (T ≈ 1/1 of reference) | Low | Hash matches the published sample hash |
| L1 | Custom record scanner: newline-delimited byte scan, no generic reader, no per-line allocation | Removes the generic-reader overhead; expected 15–25% of T | Low (pure locality change) | ≥3% T gain, hash unchanged |
| L2 | Custom timestamp: positional digit parse + civil-days integer conversion + monotonic-hour shortcut | Removes the date library; expected the largest single win (organizer hint: "second-biggest cost") | Medium (calendar arithmetic bugs) | ≥10% T gain, oracle agrees on all timestamps |
| L3 | Packed per-endpoint block lists (Option 3) instead of 1,842 grown vectors | Kills reallocation and the allocator hot spot; expected the largest M win (M ratio 10× → 50× cap) | Low | memory.peak drops below 100MB, hash unchanged |
| L4 | Quantiles without a global sort: per-endpoint order statistics via a two-pass selection over the flat arena (find p50, then p95 among survivors, then p99) | Removes O(n log n) comparison sort; expected 5–15% of T | Medium (percentile definition ambiguity) | Oracle equality on every endpoint's three quantiles |
| L4b | Alternative to L4: per-endpoint radix ordering (byte-wise passes over the flat arena, chunked to stay in cache) | Sorts only once, but with O(n) passes instead of O(n log n) comparisons | Medium (cache behavior) | Adopt only if it beats L4 by ≥3% |
| L5 | 12-thread range split with per-thread arenas and an ordered merge | Expected 2.5–4× wall-clock on 6 physical cores; the only rung that moves the T anchor from 20× to 30×+ | Medium (nondeterminism) | Thread-count invariance V3 passes at 1/6/12 workers |
| L6 | Endpoint lookup hardening: 64-bit hash + hash-first compare + one-entry memo for the Zipf head | Removes most string compares | Low | ≥2% T gain |
| L7 | Branchless digit accumulation for `status` and `latency_us`; table-driven `method` decode | Removes per-digit branches in 3.1M records × 2 fields | Low | ≥2% T gain |
| L8 | Branch-free field-boundary detection (SIMD-style byte-class masks via word-at-a-time tricks, using only in-tree `core::arch` intrinsics) | Cuts the scan cost of L1; expected 5–12% of T | Medium (intrinsics without a crate) | ≥3% T gain; must stay D = 1 |
| L9 | Output: single-pass emission, preallocated row buffer, hand-rolled integer/decimal formatting | Small T win, but it also removes the last source of float-format ambiguity | Low | Hash unchanged; field-by-field oracle equality |
| L10 | Measurement pass: X-scaling probe at 1×/2×/4× and the memory.peak confirmation | Converts assumptions into numbers before the freeze | Low | Documented in PERF.md |
| L11 | Stretch attempts (only after L0–L10 are green): deeper unrolling, per-thread software prefetch of the next buffer block, 2× split count | Diminishing; only pursued if the Commit row is already exceeded | Medium (stability) | ≥3% T gain, no regression in M |

**Explicitly rejected rungs (these go in PERF.md as failed experiments).**
- Hand-written SIMD for the *timestamp* parse before the scalar parse is in place: SIMD pays only on the digit accumulation, and the calendar conversion is inherently serial per record. Expect it to be slower than the scalar path until L7 lands.
- Sorting latencies globally: the naive per-endpoint sort is only 1,842 lists; a global sort is strictly worse.
- Keeping the whole file in memory to avoid re-reads: doubles the M cost for a sub-1% T win, because the read is already served from the page cache.
- Interleaving output with parsing: irrelevant to T, adds nondeterminism risk. Rejected on principle.
- Using a hash map with string keys: rejected (allocation + iteration-order hazard).

**Plan for PERF.md (8% of the track score, reviewed without the leaderboard).** Each rung gets one line in the form: hypothesis → measured change → kept/reverted. Numbers only: baseline mean, per-rung mean, memory.peak, and the ratio versus the batch-local reference. The failed experiments above are logged with their measured deltas, because "the failed experiments count".

---

## 6. X-Scaling Safeguard

**What X measures.** X = (runtime at 10× input) ÷ (runtime at 1×), compared against the reference's own 10×/1× ratio. Weight 0.10. The context states the intent plainly: "you are not trying to win on X, you are trying not to lose on it." A ratio of 1.00× on X means we scale exactly as the reference does; the Commit target is 1.05×, the Stretch target 1.10×, and anything above that is a pure loss.

**Where superlinearity can come from in this design.**
1. *Latency-array work.* The payload is 3.1M × 4B = 12.4MB at 1× and 124MB at 10×. Sorting/selecting is O(n log n) — mildly superlinear, but at 124MB it still fits comfortably in the 24MiB L3? No: 124MB exceeds L3, so the 10× run becomes memory-bandwidth-bound where the 1× run was L2/L3-resident. Expect a 10× input to cost somewhat more than 10× the 1× time. Mitigation: the selection pass (L4) is designed to be bandwidth-friendly — it sweeps the array in a linear, streaming manner, twice or three times, rather than a random-access sort.
2. *Dictionary growth.* Endpoint count grows with input; at 10× input there are up to 18,420 endpoints (if the corpus is 10× distinct data) — the dictionary stays cache-resident either way. Negligible.
3. *Bucket arrays.* Hourly error buckets are indexed by hour; at 10× input the hour range is 10× wider. The array stays small (kilobytes). Negligible.
4. *Buffer and buffer refill.* Fixed-size buffers, so no superlinearity. But a larger file means more read syscalls: 10× the bytes at the same buffer size = 10× the syscalls, and page-cache behavior at 3.12GB may differ from 312MB. Expect a small superlinearity here on the 10× probe; a larger buffer (4MB) amortizes it.
5. *Block allocation.* Per-endpoint block lists grow in block count; allocation is a bump pointer, so it is linear.

**The one real hazard: memory at 10×.** Our 1× peak is ~40MB; at 10× the latency payload alone is 124MB, plus 25MB of buffers, plus radix scratch (124MB if we keep a full copy) → up to ~300MB. Still far below the 8GB cap. The reference, by contrast, holds the file plus per-record structures and would scale to multiple GB at 10× — meaning the reference's own 10×/1× ratio is likely worse than 1.0 (or the reference OOMs and the organizers' X baseline reflects that). Either way we should land at or below 1.05, not above 1.10.

**Safeguards, in order of priority.**
- **S1.** Keep every buffer fixed-size; never size a buffer from the input size.
- **S2.** Keep the only input-scaling structure the 4-byte-per-record latency payload. Target: 4 bytes of state per input record, i.e. ≤1/25 of the input size. This is the design's signature claim: at 10× input, our working set grows 10× but stays under 400MB.
- **S3.** Selection/sort must be streaming-linear in the payload, not a random-access quicksort over a 124MB array. This is the single biggest X lever; it is why L4 is specified as a selection pass rather than a sort.
- **S4.** Never read the whole file into memory. Never stage it in /tmp (tmpfs pages count against `memory.peak` and against the 8GB cap).
- **S5.** Probing method: the 10× input is DEFERRED (the organizers' construction of it is not published). We build our own 10× probe by concatenating the dataset into a dev scratch file on the host scratch volume (not /tmp, not /data, not /out) and run the same binary against it. The probe must use the same code path with no input-size special-casing.
- **S6.** Acceptance rule for the 10× probe: X_local = T(10×)/T(1×) must be ≤ 1.10 and the 10× peak must be ≤ 800MB. If X_local > 1.15, the cause is identified within one hour (by ablation: payload-only scaling, then buffer scaling) and either fixed or the affected rung is reverted.
- **S7.** If, after all fixes, X_local sits at 1.05–1.15, we accept it: 0.10 weight, and a 0.10 X penalty costs 0.10·ln(1.10) = 0.0953 → K ×0.909 → about −1.9 points, versus the 3+ points a 20% T gain would return. **Never trade a verified T win for a marginal X fix.**

---

## 7. Hour Budget

Total charged to Track A: **12h = 2h baseline + 10h optimization**, per the master schedule. The baseline deliverable is due inside the H0–H17 window; the optimization block is H18–H28.

**Baseline block (2h, charged).** Scheduled at the head of the window (nominal H0–H2); the H0–H17 window exists because the baseline is a *deliverable* that must be green before the rest of the campaign commits to this track's structure. The remainder of H2–H17 is not charged to Track A: the team works other tracks and shared infrastructure, and Track A code is frozen at the baseline state during that period except for correctness fixes.

| Hour | Activity | Exit criterion |
|---|---|---|
| H0–H0.5 | Read the Oct 28 frozen spec for Track A when it lands; write down the pinned output format, percentile definition, ordering, and the input file layout | Format sheet complete; every unknown is either answered or flagged DEFERRED |
| H0.5–H1.5 | Baseline implementation (std-only, no threads): parse, per-endpoint latency list, full sort, quantiles, output in the pinned format | Compiles and runs on the 10% sample |
| H1.5–H2 | V1 verification: sample hash matches; measure baseline T, M (memory.peak), and the 5-run spread; measure the page-cache charge question (R-A2) | Published sample hash reproduced; baseline ratios recorded |

**Optimization block (10h, charged, H18–H28).**

| Hour | Activity | Exit criterion |
|---|---|---|
| H18 | L1 (custom scanner) + L2 (timestamp) | Sample + full hash unchanged; ≥20% combined T gain |
| H19 | L3 (packed block lists) | memory.peak < 100MB; hash unchanged; M ratio measured |
| H20 | L4/L4b (quantile selection, choose one) + L6 (dictionary hardening) | Oracle equality on all quantiles; ≥5% further T gain |
| H21 | L5 (12-thread split) | V3 thread-count invariance passes at 1/6/12 workers |
| H22 | L7 (branchless numerics) + L8 (word-at-a-time boundary detection) | ≥5% T gain; D still = 1 |
| H23 | L9 (output path) + L10 (X probe and memory confirmation) | X_local ≤ 1.10 measured; 10× peak ≤ 800MB |
| H24 | **Verification day** (V2 oracle diff on the full dataset, V4 repeat runs, V5 fuzz sweep) | All three green; this hour is not optional and is not traded for speed |
| H25 | L11 stretch attempt 1 (only if Commit row already exceeded) | ≥3% gain or reverted |
| H26 | L11 stretch attempt 2 / second-pass optimization on the top-2 cost centres from the profile | ≥3% gain or reverted |
| H27 | Re-anchor the axis table against the batch-local reference; recompute K; confirm the Commit or Stretch row; 5-run final measurement | Axis table updated with batch-local reference numbers |
| H28 | Freeze: last green hash, PERF.md written, source normalized and its byte count measured against the 3,180B reference (confirm S ratio ≈ 0.60–0.65) | Submission frozen; no further code changes |

**Budget hygiene rules.**
- 2h of the 12h (H24 plus part of H27) is reserved for verification and freeze. If the track is behind at H24, the optimization is cut, not the verification.
- Every hour is spent on the top two cost centres identified by measurement, never on a speculative rewrite.
- If the Commit row (T 30×, M 15×, S 0.62, X 1.05 → K 6.27, 57.0 points) is met by H24, the remaining hours go to (a) raising T toward the Stretch row, and (b) PERF.md quality, which is worth 8% of the track total and is entirely within our control.
- The 35% judges' share is not in our K arithmetic but in the track total: code quality 15%, technique 12%, PERF.md 8%. A dense, explained implementation with a measurement log is worth more than 3 extra points of K.

---

## 8. Risk Register & Stop Rule

| ID | Risk | Likelihood | Impact | Detection | Mitigation | Stop rule / trigger |
|---|---|---|---|---|---|---|
| R-A1 | Percentile definition and float format differ from the reference (DEFERRED to Oct 28) | High | **Total** (Gate 1 = 0) | V1 sample-hash mismatch after the baseline | Format sheet written at H0; one-point-of-change output module; field-by-field oracle diff | If the definition is still unknown at H20, freeze the most conservative interpretation (exact nearest-rank on the sorted array) and accept the risk explicitly |
| R-A2 | Dataset page cache is charged to the container cgroup, so `memory.peak` ≥ 312MB and the M ratio collapses to ~8.6× | Medium | Low (−2.0 points) | Measure memory.peak at H1 on a streaming read | Bounded per-thread buffers; sequential reads; no whole-file read; no /tmp staging | If M ratio < 8× at H22, stop optimizing M, re-anchor the axis table, and spend the hours on T instead |
| R-A3 | Nondeterminism from threading (merge order, dictionary insertion order) | Medium | **Total** | V3 thread-count invariance test | Integer-only arithmetic; ordered merge; explicit output sort; ids internal-only | Any hash mismatch across worker counts → immediately disable threading (fall back to single-worker) and re-verify |
| R-A4 | Civil-days / timestamp arithmetic bug (leap year, month offsets) | Medium | **Total** | Oracle diff on timestamps; a dedicated timestamp vector test | Standard civil-days algorithm; fixed-width positional parse; unit test on known boundaries (Feb 29, month ends, year ends, 23:59:59.999999) | If the oracle disagrees and the cause is not found within 60 minutes, revert to a more conservative timestamp path and re-verify |
| R-A5 | Record splitting bug at range boundaries (lost or duplicated record) | Medium | **Total** | Record count check: parsed records must equal 3.1M exactly; sum of per-endpoint counts must equal 3.1M | Boundary scan that always advances to the next newline; per-thread count reconciliation | Count mismatch → revert L5 and re-verify; never ship a threaded variant that has not reconciled counts |
| R-A6 | Source-size overrun: the S axis degrades below 0.60 | Medium | Low (−1 to −2 points) | Measure normalized source bytes after every rung | Hard budget of ≤5,300 normalized bytes; features are cut in order (L8 intrinsics first, then L6, then L4b) | If normalized bytes exceed 1.75× the reference, cut features until under 1.65× |
| R-A7 | Output ordering assumption wrong (sorted by name vs first-seen vs id) | Medium | **Total** | V1 hash | Keep ids internal; make the ordering a single switch | If still unknown at H20, emit all three orderings to separate files and keep only the one whose hash matches |
| R-A8 | A parser hang on malformed input (Gate 3: a hang is a failed run) | Low | **Total** | V5 fuzz sweep with truncation/corruption | Every scan loop has a guaranteed forward-progress condition; no unbounded lookahead; a byte budget per record | Any fuzz case that hangs → the affected rung is reverted the same hour |
| R-A9 | Reference drift >2% voids a batch and our ratios become fiction | Medium | Medium | Track the batch-local reference timing per batch | Re-derive all ratios from the batch-local reference; never mix sessions | If the reference drifts, discard the batch's measurements and re-measure; do not re-tune the design to a drifted number |
| R-A10 | The 10× X probe cannot be constructed because the 10× input is unpublished (DEFERRED) | Medium | Low | — | Build our own concatenated 10× dev corpus on host scratch; keep the code path size-agnostic | If the local probe is unavailable, reason from the 4-bytes-per-record invariant and document the reasoning in PERF.md |
| R-A11 | Speed-floor gate: a rival submission is >10× faster than us, so we lose the overall-title eligibility | Low | Low (track score unaffected) | Leaderboard is not visible; we track our own ratios | Our Commit is 30× and Stretch 45×; being 3× off a rival is unlikely | No action; note that gate 2 affects only the title, and the 35% judges' share is unaffected |

**Global stop rule for Track A (applies in this order).**
1. **Correctness stop:** if the output hash does not match at H24, the optimization phase ends immediately. The last green state is submitted, even if it is the baseline. A 0 is strictly worse than a low score.
2. **Determinism stop:** if V3 (1/6/12 workers) fails at any point, threading is disabled and stays disabled until it passes. A correct slow submission outranks a fast nondeterministic one.
3. **Diminishing-returns stop:** if two consecutive rungs each yield <3%, the ladder stops; the remaining hours go to verification, PERF.md, and freeze.
4. **Memory stop:** if memory.peak exceeds 800MB in the 10× probe or 512MB at 1×, block sizes and buffer counts are reduced before any further speed work.
5. **Timeout stop:** irrelevant at our runtimes, but if any test run exceeds 60s (vs a 1–3s target), the input path is considered pathological and is debugged before anything else.
6. **Freeze:** at H28 the last green, verified, deterministic build is submitted, together with the PERF.md measurement log. No post-freeze changes.

**Definition of done for Track A.** The published 10% sample hash is reproduced; the full-dataset output hash matches the expected hash on 5 of 5 runs; the output is byte-identical across 1, 6, and 12 workers; memory.peak is recorded and under 512MB at 1×; the 10× probe reports X_local ≤ 1.10; PERF.md contains the per-rung measurements and the failed experiments; and the committed axis table is re-anchored to the batch-local reference.
