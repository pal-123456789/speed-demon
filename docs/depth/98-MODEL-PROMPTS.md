# EXTERNAL MODEL PROMPT PACK — paste these, get power-house answers

> How to use: copy the **CONTEXT BLOCK** below, then any prompt, into your
> high-level model (Claude/GPT/Gemini/o1/DeepSeek). Paste the raw answer back
> here or into the chat; the answers become design inputs for `docs/depth/`
> at H0.
>
> **Rule 05 (critical):** answers are DESIGN KNOWLEDGE. If a model emits code,
> it lives in `docs/` (or nowhere) — BEFORE Oct 30 15:00 UTC nothing goes into
> `track-*/src/`. Paste code back marked `# DESIGN-ONLY`, never as solution code.
>
> Machine facts every model needs: single machine, Debian 13, Intel
> Core i5-13500 (6P+8E, AVX2, no AVX-512), pinned to `--cpuset-cpus=0-11`
> (12 threads), turbo OFF (~2.5 GHz sustained), 8 GB memory cap (hard, OOM =
> failed run), page cache pre-warmed, 1 GB tmpfs at /tmp. Rust only (std, no
> crates — D = 1 + deps, one dep costs K × 0.90). Output must be byte-identical
> across runs and thread counts. Scoring: K = (T_ref/T)^0.50 (M_ref/M)^0.20
> (S_ref/S)^0.15 (D_ref/D)^0.15; speed = 50·log10(K); T uncapped, M capped 50×,
> S (code lines per track folder) capped 1.0. Timeouts = 10× reference.

## CONTEXT BLOCK (paste before every prompt)

```
Competition: Speed Demon 2026 (speeddemon.dev), 72h, Oct 30–Nov 2.
Machine: Hetzner EX44, Falkenstein fsn1, Intel Core i5-13500 (6 P-cores + 8 E-cores,
AVX2, NO AVX-512), pinned to --cpuset-cpus=0-11, turbo off (~2.5 GHz sustained),
8 GB hard memory cap, page cache pre-warmed, 1 GB tmpfs /tmp. Docker, cgroups v2.
Stack: Rust, standard library only (a single external crate multiplies K by ~0.90).
Output must be byte-identical on every run and every thread count (no HashMap order,
no float non-determinism). 5 runs, ignore fastest+slowest, mean of middle 3.
Scoring: K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15;
speed = 50*log10(K) clamped [0,100]; track total = 0.65*speed + 0.35*judge.
T has no cap; M caps at 50×; S = code lines per track folder (after formatter),
capped at 1.0; D = 1 + runtime deps. Timeout = 10× reference wall time.
Three gates: SHA-256 identical output, timeout, sandbox (no network, /data ro,
one output file). Answer in prose + pseudocode, not full programs, unless asked.
```

---

## P1 — Track A: the percentile engine (answer in prose + layout, no full program)

> Task: 3.1M log lines, fields: RFC3339 µs timestamp (ts) · method (7 values) ·
> endpoint (1,842 distinct, heavy Zipf) · status (3 digits, 5xx = error) ·
> latency_us (uint32, MICROSECONDS). Aggregate per endpoint, bucketed by hour:
> request count, p50/p95/p99 latency, error rate. Reference 32.05 s / 228 MB.
>
> Deliver: (a) the fastest way to parse RFC3339 µs timestamps in pure Rust
> without libc date functions — the exact arithmetic (days-from-civil or a
> hand table), how to validate the 6 fractional digits, cost in cycles;
> (b) whether "percentiles without a full sort" is safe here: P² algorithm,
> t-digest, or an exact method (sorted per-group once the group is small —
> with 1,842 Zipf groups, is per-group sort actually cheaper? quantify);
> (c) the error-rate representation if it must hash identically to a naive
> Python reference that computes count/total with %.2f in an unknown order —
> what integer math removes the risk;
> (d) the memory layout for 12 threads merging per-endpoint arrays so
> memory.peak stays under ~20 MB (the kernel catches transient spikes).

## P2 — Track A: timestamp/parsing micro-optimization on AVX2

> The timestamp parse is ~35–45% of the reference's time. Design an AVX2
> lane-parallel split: finding the 4 space bytes and validating the fixed
> offsets (19-digit datetime, '.', 6 digits, 'Z') — portable Rust without
> unsafe, or with a minimal `std::arch` intrinsic where it provably wins.
> Also: the latency_us field is up to 10 digits — the fastest digit→u32
> without a generic atoi. State the instruction budget per record and where
> the scalar fallback must live.

## P3 — Track B: inverted index, boolean + phrase (prose + data layout)

> Task: build an index over a ~256 MB text corpus, answer 50,000 queries
> (62% BOOL left-assoc AND/OR/NOT; 38% PHRASE "adjacent tokens, exact order",
> stopwords retained) in ONE wall clock (build + queries together). Reference
> 56.35 s. Queries are readable upfront so the index can be query-aware.
>
> Deliver: (a) posting-list representation u32 delta-encoded vs fixed-chunk
> with block maxima, and SIMD galloping for AND intersection on AVX2 —
> which wins and why at ~50k queries; (b) THE decision: store positions only
> for terms that appear in PHRASE queries (how much memory does that save?);
> (c) a build strategy where the 256 MB is streamed once and never retained;
> (d) determinism-safe parallelism for the 50k queries (per-thread buffers,
> emit in input order); (e) what "intersection order is most of the win"
> concretely means (shortest-list-first, skew handling, skipping).

## P4 — Track C: the CSV quoting oracle, formalized (prose + state machine)

> Task: parse a hostile CSV, 204 MB, 3.32M records, 5 columns with header
> (id,name,note,region_id,amount), join vs a lookup, emit sorted aggregates.
> Hostile bytes confirmed: CRLF and bare LF mixed, "" escape inside quotes,
> embedded commas AND newlines inside quoted fields, trailing space inside
> quotes, a U+FEFF BOM at file start AND mid-file at chunk boundaries,
> negative amounts, empty quoted fields. Quoting rules are in the spec (not
> negotiable); most submissions fail the hash gate here.
>
> Deliver: (a) a byte-exact single-pass state machine (states, transitions,
> the exact handling of each hostile byte) that a scalar Rust oracle can
> implement deterministically — as a transition table, not code;
> (b) how to split the file for 12 threads at record boundaries when a
> newline INSIDE a quoted field isn't a boundary (the locator/oracle split);
> (c) money-safe decimal parsing: digits→i64 cents with scale 2 and a fixed
> rounding rule, negative zero handling, overflow checks; (d) hash join vs
> sort-merge for a lookup that fits in memory at ~3.32M records — when does
> each win, and which is byte-deterministic under parallelism?

## P5 — Track D: graph engine (prose + layout)

> Task: 5M-edge directed weighted graph, 92 MB, header "n m", edges "u v w"
> uint32, NOT sorted by u, dense 0-indexed ids, reverse edges listed
> separately; 2,000 short LOCAL queries (bounded distance); reference 72.69 s.
> Organizer hint: "fast parsing, compact layout, lean search loop decide this
> track; contraction hierarchies rarely repay this query mix."
>
> Deliver: (a) the fastest uint32 edge-list parser in std Rust — line split,
> digit decode, and building CSR (sorted-by-u) in one pass; (b) CSR vs
> array-of-structs for SIMD-friendly neighbor iteration on AVX2; (c) a lean
> bidirectional Dijkstra with radix heap (integer weights) and a correct early
> meet condition for distance-only output; prove the meet condition; (d) when
> u32 distances overflow (bound proof) and whether to carry u64 internally;
> (e) parallel query evaluation with per-thread scratch arrays, output in
> query order.

## P6 — Cross-cutting: determinism under parallelism (prose)

> For B and D (parallel queries) and A/C (parallel parsing): list every place
> thread count can change the output bytes — scheduling, merge order, float
> reduction, heap tie order, map iteration — and the standard fix for each.
> Give a checklist I can run mechanically (N runs at 1/6/12 threads, byte
> compare) and the failure modes that would NOT be caught by it.

## P7 — Cross-cutting: memory.peak tactics (prose)

> The kernel's peak counter catches TRANSIENT spikes (a 64 MB read buffer
> held for 100 ms counts). Given 8 GB cap but a target M ratio of 10–15×
> (i.e. ~15–25 MB), enumerate allocation patterns that spike peak on Linux
> (allocator hysteresis, mmap vs brk, glibc arena growth on 12 threads) and
> the std-Rust practices that keep peak flat (bump/chunk allocators, reuse,
> thread-local arenas, avoiding Vec doubling spikes). Rank by impact.

## P8 — Cross-cutting: K-optimal hour budget (prose/math)

> Four tracks, overall = geometric mean of best 3 track totals; each track
> total = 0.65×speed + 0.35×judge. Speed = 50·log10(K). Given a per-track
> "hours to double T" model and the S-axis cost of added lines, derive the
> allocation rule for the 72 hours and the conditions under which dropping
> the weakest track is optimal. Compare to the greedy rule "always spend the
> hour on the track with the highest marginal K."

## P9 — Per-track: the "what would a winner do" socratic

> For Track A (and repeat for each track): given the reference is a naive
> Python that allocates and sorts everything, and a "strong submission" runs
> 25× faster with 5× less memory, walk through the 72 hours as a senior
> performance engineer: what you build in the first 4 hours (SHA-green), the
> order of the ten most impactful optimizations after that, what you measure
> after each rung, and where you STOP (marginal points per hour).

---

## What to paste back

1. The raw answer in full (reasoning included — the model's dead ends are useful).
2. Any formulas/tables exactly as the model writes them.
3. Any code marked `# DESIGN-ONLY` — it goes in `docs/`, never `track-*/src/` before H0.

## Ranked by value for the window

P8 (budget) → P3 (B decision) → P1 (A percentiles) → P5 (D layout) →
P4 (C oracle) → P7 (memory) → P6 (determinism checklist) → P2 (AVX2) → P9 (socratic).