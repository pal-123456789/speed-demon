# Track A — Ingest (DEEP, target K 9–12)

**Task:** 3.1M structured log lines (239 MB) → per-endpoint counts, p50/p95/p99 latency, hourly
error counts. Reference ≈ 32.05 s / 228 MB peak. 1,842 distinct endpoints, heavy Zipf tail.
Timeout 321 s. *(These figures are the published reference numbers.)*

> All K below are **TARGET/EXPECTED** — replace T/M/S refs with the batch-local values and
> remeasure.

## Corrected K budget (recompute `∏(ref/ours)^w` on the box)

`K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15`; speed score = `50·log10(K)`,
clamped to 0–100. Wall time is uncapped; memory is capped at 50×; S and D can cost but never pay.

| Row | T | M | S | D | K | Speed |
|---|---:|---:|---:|---:|---:|---:|
| Safe | 20× | 10× | 0.60× | 1 | **6.57** | 40.9 |
| Commit | 30× | 15× | 0.62× | 1 | **8.76** | 47.1 |
| Stretch (headline) | 45× | 22× | 0.65× | 1 | **11.67** | 53.4 |

**§11 ceiling is not ~9 any more.** T is uncapped, so the old cap argument is void: at T 45×,
M 50× and S 1.0, K reaches 14.7 (speed 58.3). M 22× (≈10 MB against the 228 MB reference) is only
reachable if the input is **not** mmap-charged to `memory.peak` — favor a buffered streaming read
and measure both.

## Algorithm & data layout
- **Custom byte scanner** over the frozen line grammar; fixed-width timestamp parse; no general
  split/regex.
- **Dense IDs:** intern endpoint → `epid` (sorted/stable), hour → small int. Group key
  `gid = epid * hour_count + hour`. No hash map in the hot path or in output.
- **SoA** per-group accumulators (count, error-count, latency store).
- **Exact percentiles via two-level radix** (high-16 bits → low-16), exact for all u32 latencies —
  no t-digest/KLL (the SHA gate is exact, approximations fail it).
- Parallel: shard input by byte range on safe line boundaries; **thread-local aggregation** →
  **deterministic prefix-sum scatter** → **byte-sorted dictionary merge** (never merge in worker-
  completion order).

## Optimization ladder (PERF.md spine; keep ≥3 failed experiments)
1. Correct baseline (std split, HashMap) → SHA-green.
2. Flat arrays + dense IDs instead of per-record allocation.
3. Custom byte scanner / fixed-width timestamp.
4. 6 workers, thread-local + deterministic merge.
5. Two-level radix percentiles.
6. Arena / buffer reuse for the M axis.
Likely failed/reverted (document them): SIMD timestamp parser (branch+setup cost), lock-free shared
intern map (contention > sharding), 12 workers (SMT/cache pressure widens spread).

## Determinism checklist
- [ ] Endpoint IDs assigned by a deterministic sorted/stable rule.
- [ ] Hour buckets emitted in the frozen key order.
- [ ] Percentile rank is integer-defined (no float ranking).
- [ ] Merges follow worker-index order; error rates use integer counts + fixed formatting.
- [ ] No hash-map iteration in output; byte-identical across ≥10 runs.

## Memory & risk
- Target peak well under 8 GB; the win vs the 228 MB reference is dropping full-line retention and
  per-record allocation. **mmap may inflate `memory.peak`** — this is the main M-axis risk.
- Risk: percentile definition / tie-handling must match the frozen reference exactly (SHA).
