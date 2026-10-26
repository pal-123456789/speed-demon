# TRACK A · INGEST — INSANE-LEVEL DEEP DIVE

> Pre-kickoff design. No solution code. Companion to `docs/specs/track-a-ingest.md`.
> **Engine:** latency/epoch aggregator → SLO monitoring. **Ladder goal:** L3.
> **Contract anchors:** ref T 32.05 s, M 228 MB, S 56 lines, D 1, timeout 321 s;
> dataset 3.1 M records / 239 MB / 1,842 distinct endpoints (Zipf); fields
> `ts`(RFC3339 µs) · `method`(7-value enum) · `endpoint`(var-len) ·
> `status`(u16) · `latency_us`(u32, **microseconds — the unit trap**).
> **Absolutes:** Commit T ≈ **1.07 s** (30×), M ≈ 15 MB; Stretch T ≈ 0.71 s (45×).

---

## 1. The complete feature inventory

Every feature this track can carry, priced by priority. **P0** = mandatory for
any correct entry. **P1** = buys the Commit row. **P2** = the "insane" toppings.

### P0 — correctness-critical (a wrong one is a zero)

| # | Feature | Why it exists / the trap | Oct-28 pin needed |
|---|---|---|---|
| A01 | **Byte-split line reader** (no per-line allocation, handles `\n` and `\r\n`) | The scaffold's `io.rs` already does this — keep exactly this shape | — |
| A02 | **5-field record parser** (ts, method, endpoint, status, latency) | One wrong split on an irregular line = ordering/parsing bomb. Fields are space-delimited but the endpoint may telescope | the delimiter rule |
| A03 | **RFC3339 → unix-µs civil-time parse, hand-rolled** | std date parsers cost 35–45% of the reference's time and allocate; a fixed-calendar algorithm (days-from-civil, leap table) is ~15 lines and 60× faster. Validate `-`, `:`, `.`, `Z`/offset exactly | the accepted range (all local+00:00?) |
| A04 | **latency_us as u32, never ms** | The unit is the official trap; a naive "convert to ms" silently changes percentiles | — |
| A05 | **Percentile definition** (nearest-rank vs interpolated; which index; p99/p999 list) | The single largest correctness exposure. Must match the reference EXACTLY on 1,842 sorted lists | percentile rule + index semantics |
| A06 | **Error-rate semantics** (fraction vs %, decimals, which statuses count as error) | 4xx and 5xx must be separated if the spec says so; count vs fraction formatting is a SHA difference | error-rate format |
| A07 | **Group key** (endpoint only? endpoint × hour? endpoint × status class?) | The grouping is the output's identity — the most DEFERRED value | group key + hour bucket format |
| A08 | **Explicit output sort** | ts is monotonic per shard, interleaved across shards → output ordering is NOT input order; must be an explicit stable sort | sort order |
| A09 | **Deterministic float formatting** | Percentiles print with fixed decimals and NO scientific notation; trailing-zero policy is a SHA difference | float format spec |
| A10 | **Trailing-newline / separator policy** | The last line and the field separator are bytes in the SHA | separator + trailing newline |

### P1 — the Commit row (T and M)

| # | Feature | Why | Notes |
|---|---|---|---|
| A11 | **Chunked u32 latency arrays per group** (batch allocator, no per-record alloc) | The only structure that grows with input is 4 B/record ≈ 12.4 MB total; M target 15 MB means NOTHING else may grow with input | arena-style bump chunks, reuse |
| A12 | **Endpoint intern dictionary** (1,842 keys): hash-once → dense u32 id | Zipf head has ~millions of hits; a string-keyed map grows and thrashes; after creation, lookups are id→struct, not string→struct | — |
| A13 | **Method enum (7 values) parsed as u8 via prefix match** | Ends string comparisons in the hot loop | the 7 spellings |
| A14 | **Multi-thread parse: split the file at newline boundaries into 12 shards** | The T axis is 0.50 weight; 12× on the parse loop is the single biggest lever | shard count = cpuset width (12) |
| A15 | **Per-shard group tables + deterministic merge** | Shards produce partial groups; merge by group id, then final percentile pass — merge ORDER is fixed (id ascending), never hash order | — |
| A16 | **Percentile without full sort (when the definition allows)** | Nearest-rank on preallocated sorted-per-group arrays is fine, but if P²/histogram is legal for the reference's definition, compute percentiles in one pass | depends on A05 |

### P2 — the insane toppings (only after L2 is SHA-green)

| # | Feature | Why | Risk |
|---|---|---|---|
| A17 | **SIMD newline scan** (`memchr`-style via `std::simd`/manual u64 lanes) | Splitting cost → near zero | keep the scalar oracle for the first SHA |
| A18 | **Digit-parsing unrolled per field** (status 3 digits, latency ≤ 10 digits, no generic `atoi`) | Field parse → ~0.15× | fixed-width assumptions must match spec |
| A19 | **Endpoint encoding without dict lookup in the hot loop** (intern on first seen, else id) | Zipf head becomes an id hit after warm-up | — |
| A20 | **Precomputed decimal formatting tables** (percentiles via table + fixed width) | Float→string is shockingly slow; format from integer micro-representation | must match A09 byte-for-byte |
| A21 | **Latency array in 2-level chunk pages with a free list** | Keeps M at ~13–15 MB and avoids `len==cap` realloc spikes (memory.peak reads the KERNEL peak — a doubling spike counts) | — |

## 2. The ladder (each rung gated: SHA-green → clean-tree → 5-run bench vs same-session ref)

```
L0  reference-equivalent (python, ~32 s)        H0–H1   — the SHA oracle, the diff target
L1  correct single-thread Rust (A01–A10)        H1–H3   — target ~3 s, SHA-green, first T ratio
L2  parallel + clean memory (A11–A16)           H3–H8   — target ~1.1 s, M ≈ 15 MB
L3  insane (A17–A21)                            H8+     — target ~0.6 s; stop when the hour
                                                        buys < 0.3 speed points
```

## 3. Determinism traps (specific to A)

- **HashMap iteration order** in group emission — always collect group ids, sort, emit.
- **Floating-point in percentile computation** — if the definition allows nearest-rank on integers, keep latency as u32 and compute indexes in integers; format from an integer scaled value. NO float reduction across threads (sums differ by order).
- **Thread-count-dependent output** — the merge must be order-stable regardless of shard scheduling.
- **Unit slip** — latency_us u32; one `* 1000`/`/ 1000` anywhere is a silent wrong-percentile zero.
- **DFT (decimal formatting)** — printing `1.0` vs `1` vs `1.00` changes the SHA.

## 4. Memory budget (8 GB cap; our target ≈ 15 MB at Commit)

| Structure | Size |
|---|---|
| Latency chunks (3.1 M × u32) | 12.4 MB |
| Group table (1,842 × ~64 B) | ~0.2 MB |
| Intern dictionary (bounded) | ~0.5 MB |
| Parse shard buffers (12 × 1 MiB) | 12 MB** — the real M story: keep shard buffers SMALL and reused, or memory.peak reads the transient spike |
| Everything else | must be O(1) in input |

**Key M insight:** the kernel peak counters every high-water mark in the run, so
a 64 MiB read buffer borrowed for 100 ms becomes M = 64+ MB. Target total
transient < 16 MB at all times — read 1 MiB chunks, reuse, free shards at merge.

## 5. Parallelism plan

- 12 worker threads, one per `--cpuset-cpus` slot; main thread reads and hands
  1 MiB blocks; workers find the nearest newline from both ends of a handed-off
  block so records never split.
- Per-worker group tables keyed by interned endpoint id (u32) → chunked arrays.
- Merge: gather per-group joined arrays, sort each by id (stable), run the
  percentile + error-rate pass on the merged arrays (single-threaded final pass
  is fine — ~11 k values/group at the Zipf head).

## 6. Edge-case matrix (each must be decided + fixture-tested)

| Input | Decision to make at Oct 28 | Fallback if unspecified |
|---|---|---|
| Empty file / no records | output an empty file, or a header? | empty output, exit 0 |
| A group with zero records | spelling: emit `0`? skip the line? | emit nothing |
| Malformed line (wrong field count) | skip, or fail the run? | skip and count (log to stderr) |
| Endpoint with spaces/tabs | how is the record tokenized? | spec's field rule |
| Timestamp with `Z` vs `+00:00` | normalize both to µs? | accept both |
| status as string vs int | — | parse as u16 |
| Negative latency | impossible? clamp or skip | skip |
| Trailing newline absent on last record | — | reader handles EOF record |

## 7. Validation plan

- `difftest` against the L0 reference output on: the shipped sample, the hostile
  fixture corpus (`gen-fixtures --all`), and mutated samples (missing newline,
  CRLF-only, 1-byte fields, giant single line).
- The SHA gate pins `ingest.txt` — already scaffold-pinned; the pin changes the
  moment L1 lands (that's the intended flow — the gate forces a conscious re-pin
  per rung).
- Live-board runs after every P1 rung; PERF.md steps cite run IDs.

## 8. H0 window runbook (first 6 hours)

1. **H0–H1:** read the frozen spec's percentile + ordering + formatting pins
   FIRST. If they differ from any assumption above, this doc's P0 table is the
   change list. Tag `H0-spec-notes`.
2. **H1–H3:** L1 — get SHA-green with the *simplest correct* implementation.
   Re-pin `ingest.txt.sha256`. Push `baseline-green`.
3. **H3–H8:** L2 (parallel parse + chunked arrays + merge). Bench vs ref each
   rung; stop climbing L2 at ~1.1 s.
4. **H8+:** L3 only while the SHA stays green and the hour buys ≥ 0.3 points.
5. **Freeze-day:** final SHA + determinism ×3 runs on the machine-warm dataset;
   PERF.md ladder table complete with run IDs.

## 9. Open questions → Oct 28

- Exact percentile definition and index (A05).
- Group key / hour buckets / error-rate format / separators / sort (A06–A10).
- Accepted timestamp range and timezone spellings (A03).
- Whether the median is wanted in addition to p99 (affects the final pass).