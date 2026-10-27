# GROUND BLUEPRINTS — every feature, to the byte (pre-kickoff design)

> **Rule-05 status: DESIGN ONLY, no solution code.** Rules 04/05 explicitly
> permit reading, designing and preparing the environment before the window.
> This file is the last level of the design pyramid:
> `docs/specs/*` (contract) → `docs/depth/track-*-DEEP.md` (feature inventory
> + ladder) → `97-MODEL-OUTPUT-DIGEST.md` (algorithms from the model) →
> **this file (the ground: layouts, offsets, constants, per-feature
> validation)**.
>
> Every contract fact here comes from the organizers' reference
> implementations (starter template) — not from guessing. Where the reference
> has a quirk, the quirk is written down, because the SHA gate compares
> against the reference, not against correctness.

---

## 0. How to use this in the window

1. H0–H2: read this with the frozen spec open. If a spec line contradicts a
   blueprint line, **the spec wins** — mark the delta in PERF.md and re-pin.
2. H2+: implement feature by feature in the order given; each feature lists
   the fixture/gate that proves it. Never advance with a red gate.
3. Keep each track's implementation **terse** (target ≤ 200 code lines in the
   folder): S is capped at 1.0 so every extra line is pure cost.

---

## 1. TRACK A — Ingest · ground blueprint

**Contract (reference-exact).** Input `requests.log`, one record per LF line,
5 space-separated fields: `ts` (RFC3339, µs), `method` (ignored), `endpoint`,
`status` (3 digits), `latency_us` (µs, u32). Output `summary.txt`: for every
(`UTC hour`, `endpoint`) group with ≥ 1 record, one line:
`{YYYY-MM-DDTHH}:00:00Z {endpoint} {count} {p50} {p95} {p99} {errs} {ppm}\n`
sorted by (hour, endpoint) bytewise. `errs` counts `500..=599` inclusive.
`ppm = errs * 1_000_000 // count` (integer floor, u64). `p` percentile =
**nearest rank**: `rank = (p*n + 99) // 100`, value = sorted[rank-1].

### A-features → sub-features → the ground

| Feature | Sub-features | The ground (exact) |
|---|---|---|
| **A1 Line split** | chunk boundaries, tail records | Fixed 1 MiB raw chunks; a worker starts at the byte after the first `\n` within its chunk and ends after the `\n` at/after the chunk end → no record ever straddles two workers. Last chunk ends at EOF; if the file lacks a final `\n`, that tail is still one record. |
| **A2 Field extraction** | 4 separators, fixed-width ts, status width | ts is bytes 0..26 for the `…Z` form (`YYYY-MM-DDTHH:MM:SS.ffffffZ` = 27 B). Space at 27. Method ends at the next space. Endpoint runs to the next space. Status = exactly the 3 bytes after that space (reference relies on `int()`; the data is 3 digits). Latency = to end of line. Validation: 4 space bytes at the expected offsets, else slow path. |
| **A3 Hour key** | `Z`/`+00:00` fast path, other offsets, validation | **No calendar math needed.** The output hour is `input[0..13]` for records whose suffix is `Z` (both `Z` and `+00:00` mean the same clock reading here — the reference prints the parsed clock hour, not a UTC-converted one). Only a *non-zero* offset needs the slow path; replicate the reference: it keeps the offset's clock hour and appends a literal `Z`. Validate bytes 4/7/10/13/16/19/26 (`-`,`-`,`T`,`:`,`:`,`.`,`Z`) + 20 digits + the µs digits. |
| **A4 Endpoint interning** | local table, merge | Per-worker open-addressing table over endpoint bytes (`endpoint_id` u32). Merge across workers **by endpoint bytes**, never by table order: collect distinct endpoint byte-slices, sort them, assign global ids, then remap each worker's (local id → global id) once. |
| **A5 Error count** | status → error | `errs[g] += (status >= 500 && status <= 599) as u32`. Status is 3 digits so no parse beyond 3 ASCII digits is needed; the reference uses `int(status)` — equivalent. |
| **A6 Latency storage** | chunked, no Vec growth | Append u32 µs values to per-group **bump chunks** (e.g. 4 KiB blocks chained by an index) or a per-worker two-level table; never per-record `Vec::push` growth (that is the reference's 228 MB). |
| **A7 Percentiles** | rank formula, selection, no full sort | `rank = (p*n + 99) / 100` in u64, index `rank-1`. Compute p50 by selecting index `r50-1`, then p95 within the *upper* partition, then p99 within that (narrowing selection — 3 linear passes, no full sort). Rust `select_nth_unstable` is std. **Verify against the fixtures**: n=1/2/20/100 groups with value = 1000·rank; n=20 p95 must be 19000, p50 10000. |
| **A8 Output order** | hour+endpoint bytewise | Sort groups by (`hour13` bytes, `endpoint` bytes). Because `hour13` is fixed-width ISO, byte order == chronological — the reference's datetime sort and a byte sort agree. |
| **A9 Formatting** | integers only, single write | Hand-format: hour13 + `:00:00Z`, space, endpoint, space, decimal integers, `\n`. `ppm` is `errs*1_000_000/count` in u64 (floor). No floats anywhere in Track A. One `write_all` of a prebuilt buffer. |
| **A10 Determinism** | merge order, thread count | Per-worker results land in worker-indexed slots; merge in chunk order; global ids from sorted endpoint bytes; selection is order-independent. Byte-identical for any thread count. |
| **A11 Memory** | budget, /tmp, output pages | Reference M = 228 MB. Target ≈ 15–25 MB: 12.4 MB of latencies + interning tables + read buffers. `/data` reads are **free** for M (pre-warmed by the host); our own heap, any `/tmp` write, and the output file's first-touched pages count. |
| **A12 Validation map** | per-feature fixture | A1–A2/A4: `gen-fixtures --track A --adversarial` (published rows, `+00:00`, no-fraction, u32-max latency). A5: the `/fx/err` rows (500/599 count, 400/600 don't). A7: the `/fx/n1|n2|n20|n100` groups. A9/A8: diff against the reference output on the same file (the reference IS the oracle). A11: cgroup `memory.peak` on a Linux host. |

### A-traps (each is a zero if missed)

1. **µs, not ms.** Any `/1000` corrupts every percentile.
2. **5xx inclusive 500..599** — `>=500` alone would count 6xx (the fixture has a 600 row).
3. **Every line ends with `\n`**, including the last.
4. **Groups with 0 records are never emitted** — only keys that exist.
5. **LF only.** The reference does `rstrip("\n")`, so a `\r` stays on `latency_us` and `int()` raises; a CRLF file has no valid reference output at all.
6. **Empty-file / malformed policy**: the reference raises; our slow path should mirror the *observable* behaviour on well-formed data and never crash the run on the holdout (the holdout is generator-produced).

---

## 2. TRACK B — Index · ground blueprint

**Contract.** Inputs `docs.txt` (`id<TAB>text` per line, ids `0..N-1` in
order; a violation raises) and `queries.txt`. Output `results.txt`: one line
per query `count sum ids…` where ids are the up-to-10 smallest matching doc
ids ascending; empty result → `0 0`. Token = **maximal ASCII `[a-z0-9]` run
after ASCII-lowercasing** (bytes ≥ 0x80 are separators). `BOOL t1 OP t2 …`
applies ops left to right (AND ∩, OR ∪, NOT ∖). `PHRASE "t1 t2 …"` matches
docs containing all terms **and** an adjacent occurrence in the token stream.

### B-features → sub-features → the ground

| Feature | Sub-features | The ground |
|---|---|---|
| **B1 Class table** | 256-byte class map | One `[u8; 256]` where A–Z and a–z map to their lowercase value, 0–9 to themselves, everything else (incl. ≥ 0x80, `_`, `-`, `.`) to a separator marker. Tokenizing = walk bytes, start/end at class boundaries. This is exactly `re.findall(rb"[a-z0-9]+", text.lower())`. |
| **B2 Doc load** | id parse, tab split | Split at the **first** TAB; parse the left side as a decimal u32; assert it equals the running doc counter (mirror the reference's exception). Text = bytes after the first TAB (may contain tabs — `partition` keeps them). |
| **B3 Term ids** | interning, deterministic | Two-pass: pass 1 collects distinct terms (sorted bytewise → global ids); pass 2 assigns. **Never** first-seen order across threads (a classic nondeterminism leak). |
| **B4 Postings** | doc ids, delta/bitmap | Per term: sorted u32 doc ids. For AND-heavy work, 128-id blocks with `block_max` + galloping; dense terms (rare, stopword-like) as bitmaps (D/8 bytes). Delta-varint only if memory ever binds — it needs a skip table, which is a worse block layout. |
| **B5 Positions** | phrase support | Per term, positions as u32 (token index within the doc, or a global token counter). Total tokens ≈ 40–60M for a 256 MB corpus → ~160–240 MB; affordable under the 8 GB cap and cheaper than re-tokenizing candidates. Keep doc-id-boundary offsets so a doc's positions are one contiguous slice. |
| **B6 Query parse** | BOOL/PHRASE, no precedence | `kind, _, rest = line.partition(b" ")`. `BOOL`: `rest.split(b" ")` = `t0 OP t1 OP t2 …` — first element is a term, then (op, term) pairs, ops applied left to right. `PHRASE`: `rest` must start and end with `"`; inner bytes split on spaces. Terms are used **as written** (the reference does not re-lowercase query terms) → the generator emits lowercase. |
| **B7 BOOL eval** | set ops | Left fold: `acc = postings[t0]`; then for each (op, t): `AND` → intersection, `OR` → union, `NOT` → difference. Implement as merge walks over sorted vectors (or bitmap ops for dense terms). Empty accumulator after a NOT is legal and a later OR can revive it — never short-circuit the query. |
| **B8 PHRASE eval** | contain-all then adjacency | Candidates = ∩ postings of all terms (start from the shortest list). For each candidate doc, walk its token stream from the positions of the rarest term and require `pos[t_k] == pos[t_0] + k` (k-cursor sweep, only forward). A k-cursor sweep is O(occurrences), never a cross product. |
| **B9 Result** | count, sum, top-10 | The result set is ascending by construction (merge walks). `count = len`, `sum = Σ ids` (u64), `ids[:10]` = first ten. Empty → `0 0`. Format with a reused buffer. |
| **B10 Parallel** | queries, build | Build: fixed doc-aligned chunks, per-chunk postings, then concatenate in chunk order (doc ids already ascend across chunks → no sort). Queries: fixed chunks of consecutive queries, claimed via `AtomicUsize`, each writing its own output slot, emitted in query order. |
| **B11 Validation map** | per-feature fixture | B1: the adversarial corpus (1-byte tokens, all-stopword phrase, CRLF doc). B6–B8: the four published query lines. B9: `0 0` for a term with no postings. B10: `determinism-check.sh`. |

### B-traps

1. **Left-associative, no precedence** — `A AND B OR C AND D` is `(((A∩B)∪C)∩D)`. Implementing AND-over-OR precedence is a zero.
2. **Stopwords are ordinary tokens** (no list, no filtering).
3. **Bytes ≥ 0x80 are separators** — do not treat UTF-8 letters as tokens.
4. **Only the first TAB splits** the doc line.
5. **Missing term = empty set** (df 0), not an error.
6. **Phrase matching is on the token stream**, not raw bytes (spacing/punctuation must not matter).

---

## 3. TRACK C — Join · ground blueprint

**Contract.** Inputs `records.csv` (header `id,name,note,region_id,amount`,
5 columns) and `regions.csv` (header `region_id,country`). Output
`aggregates.txt`, one line per country, sorted by country (code-point =
UTF-8 byte order), fields space-separated:
`country count id_sum amount_sum amount_min amount_max distinct_names
name_bytes note_bytes` + `\n`. Unmatched `region_id` → `_UNMATCHED`.
Quoting = **Python `csv` excel dialect**; U+FEFF stripped from the start of
the **first field of every record** (file start or after a record
terminator; the first field is never quoted). Amounts are exactly 2 decimals
(`(-?)([0-9]+)\.([0-9]{2})`) → i64 cents, **no rounding**. `amount_min/max`
are cents; `amount_sum` is cents; `name_bytes`/`note_bytes` are UTF-8 byte
lengths of the field values.

### C-features → sub-features → the ground

| Feature | Sub-features | The ground |
|---|---|---|
| **C1 Record split** | quote parity, CRLF/LF | "Inside quotes" == odd count of `"` **as long as a quote never appears in the middle of an unquoted field** — true for generator data. Prepass per fixed chunk: (parity, first terminator at parity 0, first at parity 1); sequential pass threads the start state → cut points. Both CRLF and lone LF terminate; a lone CR also terminates per the `csv` module's universal newlines with `newline=""`… **verify against `check.py` on the sample** before relying on bare-CR lines. |
| **C2 Field machine** | 6 states | FS → IQ on `"`; UQ → E on `,`; IQ keeps everything (incl. `,` LF CR) until a `"` → QC; QC → IQ on `"` (literal), → FS on `,`, ends record on LF/CRLF, → AC otherwise; AC tolerates `"`/other (the module's lenient trailing-garbage state). Mirror this exactly. |
| **C3 BOM** | per-record strip | At a record start (file start or right after a terminator), if the **first field** begins with EF BB BF, drop those 3 bytes. Mid-file BOMs land exactly at record starts in the generator, so this one rule covers both cases. Do **not** strip a BOM that appears inside a quoted field. |
| **C4 Amount** | sign, 2 decimals, no rounding | Skip `-` (set sign), parse integer digits into u64 (checked), require `.` + exactly 2 digits, `mag = int*100 + frac`; value = ±mag as i64 (magnitude first, sign after). No rounding exists in the reference, so reject/never emit a different scale. |
| **C5 id / region_id** | fullmatch digits | Digits-only; used as u32/u64. A non-digit is a reference exception — never happens on generator data. |
| **C6 Join** | regions map | Load `regions.csv` into (region_id → country) via the same CSV machine; missing key → `_UNMATCHED`. Build the map once, share read-only across workers. |
| **C7 Aggregates** | 8 fields | Per country: `count` u32, `id_sum` u64, `amount_sum` i128 (overflow-proof), `amount_min/max` i64, `distinct_names` = |set of name bytes| (exact dedupe: arena of name bytes + per-country open-addressing set of (offset,len) with byte compare), `name_bytes` = Σ UTF-8 lengths of name fields, `note_bytes` = Σ of note lengths. |
| **C8 Read-only fast path** | skip name/note payloads | Only columns 0, 3, 4 are interpreted; name/note need their **byte length** (and name dedupe). So the field machine can *skip* copying name/note bytes, recording only (offset,len) into the raw buffer for the dedupe hash — no per-record allocation. |
| **C9 Output** | sort + format | Sort countries by byte order; format each line into one buffer; **one** `write_all`; file written as UTF-8 bytes with `\n`. |
| **C10 Parallel** | shard merge | Per-worker aggregation tables (country → Country); merge by country key (sorted union), summing integers; `distinct_names` sets merged by union with the same byte-compare. Deterministic regardless of thread count. |
| **C11 Validation map** | per-feature fixture | C1–C3: the 11-case adversarial corpus (`gen-fixtures --track C --adversarial`) + the parser/oracle differential (`tooling/csv-boundary-check.py`). C4: `-5.25`, `0.00`, `-0.01`. C7: the sample records' expected rainbow. C9–C10: `determinism-check.sh`. |

### C-traps

1. **A `,` or LF inside quotes is data** — the #1 way to fail.
2. **`""` inside a quoted field is one literal quote**, not a field end.
3. **BOM stripping is per-record**, not only at offset 0.
4. **No floats, no rounding** — cents are integers (i128 for the sum).
5. **Sort is by country bytes**, not by count/amount.
6. **`_UNMATCHED` spelling** (underscore, capitals) is part of the hash.

---

## 4. TRACK D — Traverse · ground blueprint

**Contract.** Inputs `graph.txt` (`n m` header, then `m` lines `u v w`, u32,
**not sorted by u**, ids dense 0-indexed, directed, reverse edges listed
separately) and `queries.txt` (`s t` per line). Output `distances.txt`:
**distance only**, one integer per query in query order; unreachable → `-1`;
`s == t` → `0`. Reference = per-query Dijkstra with early exit at the target;
Python ints (unbounded) → carry **u64** internally.

### D-features → sub-features → the ground

| Feature | Sub-features | The ground |
|---|---|---|
| **D1 Header** | `n m` | Parse two u32s on the first line. `n` nodes, `m` edges. |
| **D2 Edge parse** | SWAR u32 | Fixed 1 MiB chunks snapped to line starts; per line parse `u`, space, `v`, space, `w` with a digit loop or SWAR; ~5M lines ⇒ this is a top-2 cost. |
| **D3 CSR build** | counting sort | Pass 1: `deg[u] += 1` (per-thread shards + atomic or per-chunk counts). Prefix-sum → `off[0..=n]`. Pass 2: place `(v,w)` into `edge[off[u]..]`, or pack `(w<<32)|v` into one u64 array. Adjacency order may follow input order — it does not affect distances. |
| **D4 Distance store** | per-query, reused | Per worker: `dist: Vec<u64>` of length `n` plus a **touched list** to reset only what was touched (never `fill(n, INF)` per query — that alone can dominate). Stale-entry check on pop. |
| **D5 Heap** | binary → radix | Start with `BinaryHeap<Reverse<(u64, u32)>>` or a packed u64 key heap (dist<<32 | node). If queries dominate T, switch to a radix heap over u32 keys (weights are u32, distances ≤ u64 but the frontier keys fit u32 once bounded). Keep capacities across queries. |
| **D6 Early exit** | target settle | Pop `(d,u)`; if `u == t` return `d` (with the stale check first: `if d > dist[u] { continue }`). If the heap empties, return `-1`. `s == t` returns 0 before any search. |
| **D7 Overflow** | u64 labels | `nd = d + w` in u64. Never u32: `(n−1)·w_max` can exceed 2^32. Output prints the u64 exactly (the reference's Python int). |
| **D8 Parallel** | query slots | Queries are independent → fixed chunks claimed atomically; each writes its own answer slot; emit in query order. A per-worker `dist`+touched list is reused across that worker's queries. |
| **D9 Validation map** | per-feature fixture | D1–D3: the adversarial graph (published bytes, self-loops, parallel edge, zero weight, unsorted). D5–D7: `s==t`, unreachable pair, directed gap (`4 3` with no reverse edge). D8: `determinism-check.sh`. Correctness oracle: the reference's own output. |

### D-traps

1. **Distance-only** — printing a path is a wrong format.
2. **`-1`**, not `None`/`unreachable`/empty.
3. **Directed**: a reverse edge exists only if listed.
4. **Parallel edges**: keep the best; zero weights are legal.
5. **Self-loop** must not create a shorter cycle (it can't, weight ≥ 0).
6. **Dense ids are guaranteed** — no `HashMap` for node ids, use arrays.

---

## 5. Target-T table (what "fast enough" means, exactly)

With M/S/D held at 1.0 (worst case — the score then comes from T alone),
`K = sqrt(Tref/T)` so `T = Tref / K²`. Speed points = `50·log10(K)`.

| Track | Tref | K=5 → 34.9 pts | K=10 → 50 pts | K=20 → 65.1 pts | K=50 → 84.9 pts |
|---|---|---|---|---|---|
| A | 32.05 s | 1.28 s | **0.321 s** | 0.080 s | 0.0128 s |
| B | 56.35 s | 2.25 s | **0.564 s** | 0.141 s | 0.0225 s |
| C | 14.44 s | 0.578 s | **0.144 s** | 0.036 s | 0.0058 s |
| D | 72.69 s | 2.91 s | **0.727 s** | 0.182 s | 0.029 s |

Read it as: **K = 10 is reachable with T alone** if the implementation is
~100× the reference; M and S contributions then push K higher for free
(halving M ≈ +3 pts, S near 1.0 keeps its 0.15 weight intact). The board caps
a run at **3×** the reference, so anything above `3·Tref` is invisible — every
row above is far inside that.

---

## 6. THE ZERO-SCORE RISK REGISTER (100% success = every row mitigated)

| # | Failure mode | Consequence | Prevention already in the repo |
|---|---|---|---|
| 1 | Output bytes differ from the reference | track = 0 | Pins + `pin-hash.sh`; `verify`/`lockdown` hash gate; `difftest.py` vs the reference; fixtures carry the published bytes |
| 2 | Nondeterminism (threads/chunks/scheduling) | track = 0 (SHA of 5 runs) | `determinism-check.sh` (threads × chunk × chaos) as a CI gate; merge-order rules per track above |
| 3 | Timeout (10× final, 3× board) | track = 0 | `bench-rung.sh` + scoreboard; target-T table; timeout gates in verify/pre-freeze |
| 4 | Sandbox violation (write outside `/out`, network) | track = 0 | Design writes exactly one file; `e2e-sandbox.sh` asserts one output file; no network anywhere at run time |
| 5 | Build fails offline | track = 0 | `build.sh` is rustc-only (no registry); `context-replay.sh` builds from a clean copy of the folder |
| 6 | FROM not allowlisted / not digest-pinned | build refused | `check_base_pinned.py` against `tooling/base-image.txt`, digests verified against the live allowlist |
| 7 | Guardrail keyword in the Dockerfile | build refused | Comments hardened to be keyword-free; verified by scan |
| 8 | Image > 2 GB or build > 10 min | build refused | Runtime stage is `debian:13-slim` + a stripped ~1–3 MB binary; build is one rustc invocation (seconds) |
| 9 | Wrong output filename | gate fails | Official names everywhere (`summary/results/aggregates/distances.txt`); `check.py`'s own OUTPUT dict parity |
| 10 | Wrong input filename | empty/garbage output | Resolved names (`requests.log`, `docs.txt`+`queries.txt`, `records.csv`+`regions.csv`, `graph.txt`+`queries.txt`) |
| 11 | `.speed-demon.toml` wrong keys or `team` placeholder | board/registration problems | Board-parsed shape in all four; pre-flight checklist in the runbook |
| 12 | Folder not `track-a..d` | board ignores it | Renamed lowercase; every tool/harness/doc updated; case-sensitive sweep clean |
| 13 | Accidentally adding a dependency | K × 0.90 each | No `Cargo.toml`; `lockdown` asserts `dependency_count == 0` and D = 1.00; owner-confirmed D rule |
| 14 | Memory blow-up (cgroup peak, incl. output pages) | M penalty / OOM | P7 doctrine + owner's clarification; `prep/bump.rs`; per-track memory budgets |
| 15 | Overfitting to the sample | holdout failure | `gen-fixtures.py` (seeded, hostile) + the "no input recognition" rule; official generators wrapper for the real shape |
| 16 | Registration / repo mechanics forgotten | **no score at all** | Runbook "Submission logistics" + pre-flight checklist |
| 17 | Repo still private at freeze | no score | Freeze-day step in the runbook; `lockdown` checks the remote |
| 18 | An unchanged track folder | reported "unchanged", not run | Delete non-entered folders (pre-flight check) |
| 19 | Rules drift after the freeze | misinterpreted score | `contract.toml` + `spec_intake` (0 blocking) + digest §12/§13 citations |
| 20 | A hidden bug in the harness itself | false confidence | 20 harness unit tests incl. the published worked example; `k_whatif --selfcheck` pins the math |

**The win condition, stated plainly:** every track SHA-green and deterministic
(#1, #2), comfortably under 3× the reference (#3), built by a folder that
passes the board's pre-build checks (#5–#8), with the official filenames and
manifest (#9–#12) — and the submission actually registered and public on time
(#16, #17). Everything on that list is already built, tested, and gated.
---

## 7. DEEPER SUB-SYSTEMS — the optional accelerators, to the ground

These are the P2 "insane" rungs, specified so they can be implemented without
further design. Each states its precondition, its exact procedure, its cost,
and how to prove it is correct before adopting it.

### A13 · Two-level radix histogram for EXACT percentiles (alternative to selection)

- **Precondition:** a group is large enough that three selections cost more
  than two counting passes. Crossover from the deep dive: selection wins below
  ~n = 200; the histogram wins on the Zipf head groups (10^5–10^6 values).
- **Procedure (exact, no approximation):** (1) pass 1 buckets the u32 latency
  values by their top 16 bits into a 2^16 `u32`-count array; (2) prefix-sum that
  array to find which bucket holds rank `r` for each of p50/p95/p99; (3) pass 2
  scans only the values whose top 16 bits match that bucket and counts their
  low 16 bits into a second 2^16 array; (4) prefix-sum again and read the exact
  value at the residual rank. Ties are exact because it is a counting sort.
- **Cost:** 2 passes over the group + 2 × 256 KiB of counters per worker
  (reused, zeroed only on the touched buckets). Memory is O(1) in n.
- **Proof:** run both the histogram and `select_nth_unstable` on every fixture
  group and assert equality — the histogram is only adopted if it matches on
  all of them (the SHA gate would catch a mismatch, but a cheap unit check
  finds it in seconds instead of a board run).

### B12 · Bitmap BOOL evaluation for dense terms

- **Precondition:** a term whose df ≥ D/16 (a stopword-like term). Its posting
  list is larger than D/8 bytes, so a D-bit bitmap is smaller AND faster.
- **Procedure:** represent dense postings as `[u64; ceil(D/64)]`. AND = `&`,
  OR = `|`, NOT = `& !` — D/64 word ops, ~8× fewer than a merge walk. Sparse
  terms stay sorted-`u32` and are merged into the bitmap by setting bits (or
  the whole query is evaluated as bitmaps when ≥ 2 dense terms are involved).
- **Mixed case:** drive from the sparse list (iterate its doc ids and test the
  bitmap bit) when one side is sparse — that is O(|sparse|) probes, never a
  materialized union.
- **Cost:** D/8 bytes per dense term per direction; build once at index time.
- **Proof:** evaluate every BOOL fixture query with both paths (bitmap and
  merge) and diff the result sets.

### C12 · Prefix-XOR quote parity via carry-less multiply (SIMD split)

- **Precondition:** the scalar parity prepass shows up in a profile (with 12
  threads it usually does not; this is a late rung).
- **Procedure:** build a 64-bit mask per 64-byte block with a bit set at each
  `"` position (compare + movemask). The running parity at any block boundary
  is the XOR of all previous masks' **popcounts**; the prefix-XOR *mask* is a
  carry-less prefix product, computed with `_mm_clmulepi64_si128` over a
  logarithmic shift chain — 64 blocks (4 KiB) per vector op chain. Then the
  first terminator with even parity inside a block is `tzcnt(mask_of_newlines &
  ~prefix_xor)` and similarly for odd.
- **Requires:** `pclmulqdq` (SSE4.2-era; present on the i5-13500) behind
  `is_x86_feature_detected!`, std `core::arch` only.
- **Proof:** the scalar `tooling/csv-boundary-check.py` already enumerates the
  authoritative cuts for every fixture and chunk size; the SIMD path must
  produce the identical cut set, or it is not adopted.

### D10 · Radix heap — the exact bucket layout

- **Precondition:** the query phase is > ~20% of T (measure with `bench-rung`).
- **Procedure:** keys are u64 distances but the *difference* from `last` fits
  u32 once the frontier is bounded; 33 buckets `[Vec<(u32,u32)>; 33]` where
  item = `(key_delta, node)`. Bucket index = 0 if `delta == 0`, else
  `32 - leading_zeros(delta)` (i.e. `floor(log2(delta)) + 1`). `pop`: if bucket
  0 is non-empty, take its minimum (monotone order); otherwise scan for the
  first non-empty bucket, set `last += min_delta_of_that_bucket`, then
  redistribute its items downward by recomputing deltas. `top` is O(1) via
  `last + bucket0_min`.
- **Cost:** O(1) amortized per push/pop vs O(log n) for the binary heap; 33
  vectors reused across queries (keep capacity, clear length).
- **Trap:** redistribution must preserve the invariant `last <= every key`;
  an off-by-one there silently breaks Dijkstra. Prove it with the fixture
  graph plus a random-graph differential against the plain binary heap.
