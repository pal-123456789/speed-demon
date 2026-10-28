# MODULE PLAN — the adopted D1 architecture (with corrections)

> Source: the high-level model's decomposition (prompt 01), reviewed against the
> official specs, the harness source and our measured profiles. **Adopted as the
> build skeleton**, with the corrections in §1 — the model did not have the
> specs, so its five errors are exactly the kind that would have cost hours.
> Build order in §3 is re-based on the assets we already have.

## 0. Strategy (adopted verbatim — it is correct)

- **Correct baselines on all four tracks first, then polish by marginal
  return.** The overall is a geometric mean of the best three, so one zeroed
  track in the top three zeroes the overall; a fourth non-zero track is cheap
  insurance.
- **Exchange rates.** `speed = 50·log10(K)`: halving T is **+7.5 speed**
  (+4.9 track points); halving M is **+3.0 speed** (+2.0). Extra lines beyond
  `S_ref` cost `r^0.15` on K, so they must buy at least `r^0.3` on T.
- **Target table the model produced** (replace with measured numbers as soon as
  each baseline runs): A 40–60 ms / ~25 MB → speed 75–80 · B 0.3–0.6 s /
  120–150 MB → 59–69 · C 0.1–0.2 s / 50–70 MB → 52–60 · D 0.15–0.5 s / ~100 MB
  → 65–74.

## 1. The five corrections (verified against the official artifacts)

| # | The model said | The truth | Evidence |
|---|---|---|---|
| 1 | "PGO is out because the build is offline." | **PGO works offline.** The allowlist carries `halloumee/speed-demon-rust:1.99.1-trixie-llvm-tools` precisely for `llvm-profdata`. Recipe: `docs/depth/93-PGO-AND-CODEGEN.md`; price it with `k_whatif --tradeoff`. | `allowlist.json`; the organizers' own announcement item 4 |
| 2 | "One binary crate per track… a path dependency may count toward D." | **We ship no cargo at all**: `build.sh` calls `rustc` directly, so there is no manifest and D comes from the official declaration branch. Adding a `Cargo.toml` would create the very risk the model warns about. | official `dependencies.py`: manifests win when present; else `dependency_count`; measured **D = 1.00** on all four by their own code |
| 3 | "Comments are *probably* free under scc's Code column… confirm with scc." | **Confirmed free, and rustfmt runs first.** S is *"code lines scc counts (no blank lines, no comments)"*, formatted by the language's canonical formatter. So document heavily in-code and in `PERF.md`/`README.md` — prose and data cost nothing. Line-golfing is pointless; what costs is *code* lines. | official `static_axes.py` docstring; our counter is aligned and reports 141 code lines/track |
| 4 | "Run the reference on fixtures plus a 1% subset." | We do better: **the official sample data is installed and its expected output hash is pinned** (`ca6311bb… / eeeec1e9… / 9035fed3… / a4c76ed0…`), and the `tiny` corpora contain **every documented edge case with the oracle's answers**. The baseline acceptance test is the official hash, not a self-made fixture. | `tooling/official-data.py`, `verification/cases/`, `tooling/spec-cases.py` |
| 5 | "A path crate may count toward D… no shared cross-track crate." | Correct inside a track, but we go further: **nothing is shared by path at all** — each track folder is self-contained (its own `src/`, no crate). Copy-in text is fine; a path dependency is not. | the same official D rule |

Also confirmed by their code, in our favour: `.sccignore` **is** honoured by the
official measurement, and registry-vendored packages are excluded from S but
counted in D. We use neither — we simply have no dependencies.

## 2. Modules and acceptance tests (adopted)

### A · INGEST

| Module | Responsibility | Must NOT | Acceptance |
|---|---|---|---|
| `slab` | file → 1 MiB line-aligned slabs via `pread` | parse; depend on thread count | slab line sets == Python's lines; 64 B vs 1 MiB slabs give the same output SHA |
| `line` | slab → `(hour = bytes[0..13], endpoint &[u8], 5xx = status[0]=='5', latency u32)` | calendar math; touch µs | fields == Python `split` on 1M fuzz lines (1- and 10-digit latencies) |
| `groups` | per-thread endpoint/hour interning (open addressing, byte-verified) → slot {count, errs, bump-chunked latencies} | iterate hash order; grow `Vec` | per-group count/errs/multiset == Python dict |
| `rank` | `&mut [u32]` → p50/p95/p99, `k = (p·n+99)//100` via `select_nth_unstable`, p99 first then narrowing | full sort above n≈16 | n ∈ 1..300 with duplicates == `sorted()[k-1]` |
| `emit` | merge; order by (hour, endpoint bytes); `ppm = errs·10^6/count` in u64 | floats | **full-file SHA == the official expected hash**, threads {1,2,12} × 3 runs |

`endpoint` is compared bytewise; the hour key is the first 13 bytes — this is
what removes the reference's 48.7%.

### D · TRAVERSE

| Module | Responsibility | Must NOT | Acceptance |
|---|---|---|---|
| `csr` | two-pass parse (pass 1 degrees via `AtomicU32`, pass 2 `fetch_sub` scatter) → `off[n+1]`, `edge[m] = (w<<32\|v)`, `wmax` | sort rows; buffer parsed edges | `(u,v,w)` multiset == Python parse |
| `rheap` | monotone radix heap, u64 items `key<<32\|node` | know the graph; decrease-key | 10^6 random monotone ops == `heapq` |
| `scratch` | per-thread dist table, reset via a touched list | memset per query | 10^5 consecutive queries == fresh-state results |
| `search` | uni-Dijkstra first, then bidirectional (stop at `tf+tb ≥ µ`, µ updated unconditionally per edge, empty heap ⇒ top = ∞) → i64 | allocate per query | all-pairs on 3k-node graphs with zero weights, parallel edges, self-loops, dead ends vs `heapq`; plus 50 full-size queries |
| `drive` | atomic claim (4 at a time) → `ans[qi]` → one write | emit in completion order | full SHA == official hash, threads {1,2,12} |

Adopted sizing: `off` 7 MB, `edge` 40 MB, transpose ≈ 47 MB. **u32 labels are
provably safe when `(n−1)·wmax < 2^32`** (i.e. `wmax ≤ 2468`); otherwise u64 in
one instantiation, never generics. Baseline heap = std `BinaryHeap`.

### C · JOIN

| Module | Responsibility | Must NOT | Acceptance |
|---|---|---|---|
| `regions` | regions.csv → id → country index; countries bytewise; `_UNMATCHED` last | touch the big file | lookups == Python for present, missing, duplicate ids |
| `parity` | per 4 MiB chunk, count of `"` mod 2 → prefix XOR gives the quote state at each chunk start | parse | state at every chunk start == the sequential state machine (chunk = 64 B) |
| `records` | chunk + state → records; BOM strip at record starts; LF/CRLF/CR terminators; fields as raw slice + quoted flag; `""` lazily | allocate per field | records == Python `csv.reader` on the hostile fixtures |
| `cents` | field → i64 cents (`-?\d+\.\d\d`), `-0.00` → 0 | floats | fuzz == `Decimal × 100` |
| `agg` | per-thread country slot {count, id_sum, sum, min, max, name_bytes, note_bytes, 128-bit name hashes}; merge by sort+dedup | `HashSet` iteration | 8-tuple per country == Python; chunk sweep {64 B, 4 KiB, 4 MiB} × threads {1,2,12} identical SHA |
| `emit` | cents → text; `_UNMATCHED` last | print `-0.00` | cents ∈ {0, −1, −5, −99, −100} |

Quote parity is exact: a `""` pair toggles twice, every other `"` once. Settle
these against the reference before coding: CRLF inside quotes in `name_bytes`,
empty vs quoted-empty in the distinct count, duplicate region ids.

### B · INDEX

| Module | Responsibility | Must NOT | Acceptance |
|---|---|---|---|
| `tok` | bytes → `[a-z0-9]+` (256-entry lowercase table; ≥0x80 separates) | allocate strings; tokenise the id field | per-doc tokens == `re.findall(rb'[a-z0-9]+', text.lower())` **on bytes** |
| `queries` | queries.txt → interned terms + ordered op list | reorder across OR | AST == Python parse |
| `build` | wave pipeline → postings for query terms only; docs renumbered so ordinal order = id order when file ids are not ascending | retain the corpus; keep positions | per-term sets == Python dict-of-sets on a 1 MB subset |
| `phrase` | consecutive-position matching (k-cursor over positions; Aho–Corasick over term ids is the optional upgrade), reset at doc start | cross documents | traps: crosses `.`; must NOT cross a doc boundary; `a a a`; length-1 phrases |
| `eval` | left fold; AND/NOT-runs distributed over the OR chain → (count, Σid in u64, ten smallest **ids**) | AND-over-OR precedence | random 2–6-term queries, all operators, == Python left-to-right |
| `drive` | 64-query chunks, results in slots, ordered write | emit in completion order | full SHA == official hash, threads {1,2,12} |

Baseline first: sorted `Vec<u32>` with merge/gallop. Add bitmap/block tiers only
if `eval` exceeds ~25% of the profile.

## 3. Build order, re-based on what we already have

The model budgeted **H0–3 for a harness that already exists** (oracle runner,
fixture generators, memory.peak reader, SHA sweep over threads, scc-aligned
counter: all built and gated). That is ~3 hours returned to the baselines.

| Hours | Work |
|---|---|
| **H0–1** | bootstrap: repo + skeleton push, `official-data.py`, first green loop on the scaffold, board app + form |
| **H1–21** | **correct baselines, all four**, SHA-green against the official expected hash: A ~6 h, D (std heap) ~3 h, C sequential ~6 h, B simple lists ~6 h |
| **H21** | **gate**: any baseline red ⇒ stop polishing the others and fix it |
| **H21–45** | polish by measured return: D radix heap + bidirectional (~5 h), A parallel `select_nth_unstable` (~3 h), C quote-parity parallel + SWAR cents (~6 h), B phrase + parallel build/eval (~10 h) |
| **H45** | **gate**: if B lags, keep its baseline as insurance and move hours to the weakest of the top three |
| **H45–56** | profile, trim `memory.peak`, run the exchange-rate test on every remaining idea (`k_whatif --tradeoff`) |
| **H56–66** | optional PGO round (correction #1) — priced first; no features after H66 |
| **H66–72** | **D3 wrap-up** (only here): determinism soak, offline Docker build, docs audit, freeze/tag, video, submission |

## 4. Risks (adopted, with our mitigations already in place)

1. **Hostile-input semantics.** *Mitigation already built:* the `tiny`
   conformance corpus carries every documented edge case plus the oracle's
   answers; `spec-cases --run` is the millisecond loop.
2. **Non-determinism.** Fixed partitions as constants; sort at every merge; a
   SHA sweep over threads {1,2,6,12} × 3 runs on every commit —
   `determinism-check.sh` does 36 configs × 3 runs today; ban `HashMap`/`HashSet`
   iteration order and `f64` in the hot path.
3. **`memory.peak` spikes.** `with_capacity`, `pread`, no `/tmp` scratch, log
   the peak per commit, alarm at 2× budget.
4. **Crash/hang/non-zero exit.** Worst-case fixtures per track, `thread::scope`
   without channels, input-dependent paths free of `unwrap`, explicit success
   path only.
5. **Build and sandbox.** `x86-64-v3` (not `native`) keeps the image valid
   anywhere while using AVX2; intrinsics behind `is_x86_feature_detected!` with
   a scalar path; the full offline container flow re-run at H10 and H66.

## 5. What NOT to build (adopted, with one reversal)

- **D:** contraction hierarchies / ALT (preprocessing costs more than the run);
  SIMD neighbour iteration (mean degree 2.9; AVX2 has no scatter); bidirectional
  before the baseline's ball sizes are measured.
- **A:** t-digest/histograms (inexact ⇒ SHA fail); calendar/epoch parsing (the
  key is 13 bytes and µs is never emitted).
- **B:** a positional index before phrase share justifies it; varint/PFor
  compression (20 MB ≈ 0.8 speed points); query-result caching.
- **C:** a general CSV library or trait-based field abstraction; one specialised
  state machine plus quote parity covers it.
- **Infrastructure:** work-stealing pools or channels (atomic claim +
  `thread::scope`); `extern "C"` mmap/hugepages/affinity (unsafe lines, pages
  charged, cpuset already pins); a shared cross-track crate; custom allocators.
- **Line golfing:** rustfmt re-expands formatting and comments are free — count
  *code* lines, not characters.
- **REVERSAL — PGO is IN**, not out (correction #1): it is available offline and
  the `--tradeoff` tool prices whether a given training run pays for itself.
