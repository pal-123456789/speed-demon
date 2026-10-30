> ## SUPERSEDED — read this before using anything below
>
> An **early design exploration** (Oct 26–28), kept for its reasoning, not for its
> prescriptions. It was written before the frozen specs and harness were read, so
> its numbers are estimates where the real ones are now published, and several of
> its prescriptions were replaced.
>
> **Authority, in order:** the frozen track spec and harness under
> `reference/official/`, then `verification/contract.toml`, then
> `docs/MODULE-PLAN.md`. Where this file disagrees with any of them, this file is
> wrong.
>
> **Stale facts in this file, corrected:**
>
> | This file says | Correct |
> |---|---|
> | M_ref / S_ref **DEFERRED** | **Published.** M_ref = 367.435776 MB, S_ref = 108 lines. T_ref = 14.444059 s, timeout 144.4 s. |
> | sort-merge join with a **radix sort** of the key column (L6) | The adopted path is a **single-pass hash join** into per-country accumulators — the lookup is ~200,000 regions and the join is one probe per record. Sort-merge remains a candidate only if measurement beats it. |
> | distinct-name counting by hashing | Adopted: **partitioned dedupe by (country, high hash bits) into cache-sized buckets**, clearing per country. **Keep 128-bit hashes** unless a measured gain exceeds ~5% of T: with a holdout seed, "the output hash matched on seed 1" is not evidence that a 64-bit hash is safe. |
>
> The three oracle-deferred behaviours (unterminated quote, bytes after a closing
> quote, `-0.00` as a minimum) must be settled against the vendored reference
> before the parser is written — they are not decided in this file.
---

# TRACK C · JOIN — Design Specification

Status: pre-kickoff design document. No solution code, no function bodies, no implementation language.
Track window: baseline H0–H17 (2h charged to Track C), optimization block H43–H51 (8h charged).
Reference: T 14.44s · Mref/Sref DEFERRED to Oct 28 frozen spec · timeout 144s · input 204MB hostile CSV, 5 fixed columns.
Join key, lookup-table schema, aggregate key, and output format: **DEFERRED to Oct 28 frozen spec**.
Banned: any analytics/dataframe library.
Scoring reminder: K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15; speed_score = 50·log10(K), clamped [0,100]. The X axis is gone; only M is capped at 50×.

---

## 1. Reference Teardown

**What the reference does (from the frozen brief).** Parses a hostile multi-megabyte CSV — quoted commas, escaped quotes, embedded newlines, mixed line endings, a UTF-8 BOM on SOME chunks mid-file and not others — joins it against a lookup table, emits sorted aggregates.

**Measured reference axes.** T = 36.9s. Mref, Sref, and Xref are **DEFERRED to Oct 28 frozen spec**. The track's published reference is 36.9s; the timeout is 369s (10×).

**The exact input, as published.**

Column count is fixed: `id,name,note,region_id,amount`. Everything else about the encoding is not fixed. Bytes as they appear on disk (⟨BOM⟩ = U+FEFF, ␍␊ = CRLF, ␊ = bare LF):

```
⟨BOM⟩id,name,note,region_id,amount␍␊
1,"Kaphar, Titus","said ""no"" twice",7,1420.00␍␊
2,"Betts","a note that␊spans two lines",7,88.50␊
3,,"",12,0.00␍␊
4,"trailing space ","comma, quote "" and newline␊together",3,-5.25␊
```

**The hostile feature inventory (exhaustive from the five lines).** Every feature is visible in this sample, which is the point: the spec is showing us the full threat model.

| # | Feature | Where it appears | Why it breaks naive code |
|---|---|---|---|
| H1 | UTF-8 BOM at file start | before `id,name,...` | A UTF-8 reader keeps U+FEFF as part of the first header cell → the header is mis-parsed and the first column name is wrong |
| H2 | BOM mid-file on SOME chunks | "a UTF-8 BOM on SOME chunks mid-file and not others" | A BOM is only special at a boundary; anywhere else it is data. Any "strip all BOMs" rule corrupts data; any "keep all BOMs" rule corrupts the header |
| H3 | Quoted comma | `"Kaphar, Titus"` | Naive split on ',' yields two fields and shifts every later column |
| H4 | Escaped double quotes | `said ""no"" twice` | A quote scanner that toggles on every `"` will end the field at the first quote of the pair; the RFC-style rule is that a doubled quote inside a quoted field is one literal quote and does not close the field |
| H5 | Embedded newline inside a quoted field | `"a note that␊spans two lines"` and `newline␊together` | A line-oriented reader produces two records from one record; a record-count check is the detector |
| H6 | Mixed line endings | CRLF and bare LF in the same file | A reader that strips only '\n' leaves a trailing '\r' in the last field; a reader that strips only '\r\n' desynchronizes on bare LF |
| H7 | CRLF *inside* a quoted field | "mixed line endings" + embedded newlines | Whether a CRLF inside a quoted field is content or a terminator is a spec question. DEFERRED to Oct 28. We must decide and pin it |
| H8 | Empty quoted field vs empty unquoted field | `3,,"",12,0.00` — `note` is `""` (empty string), and `name` is absent/empty | `""` and a missing field must be distinguishable if the output format distinguishes them |
| H9 | Trailing space inside a quoted field | `"trailing space "` | Field content must be preserved byte-exactly, including the trailing space; no trimming |
| H10 | Leading BOM + CRLF header line | line 1 | The header line may be a header (skippable) or the first record |
| H11 | Negative decimal | `-5.25` | Signed decimal parsing; not an unsigned integer |
| H12 | Variable decimal scale | `1420.00`, `88.50`, `0.00` | Scale handling and rounding must match the reference's formatting exactly |

**Why the reference takes 36.9s — structural reading (hypothesis, to be measured in H0–H2).**
- A robust CSV parser must track a quoting state per byte. At 204MB that is 204M state transitions, each with a branch. A naive parser (split on newlines, then split on commas) is fast but *wrong*; a generic library is correct but slow and also sets D far above 1 (and analytics/dataframe libraries are banned outright).
- The reference must materialize fields: `name` and `note` are free text of unbounded length. Copying them into per-field strings for 204MB of input is where M goes.
- The join: 204MB of records against a lookup table. Whether that is a hash join or a sort-merge join is the brief's central question ("hash join vs sort-merge on data where the answer isn't obvious").
- The aggregate: emitted sorted, so a sort of the aggregate space is required.

**Gate implications for this track — the organizers explicitly warn that most disqualifications happen here.**
- Gate 1 is a *byte* hash of the output. One wrong record ordering, one wrong rounding, one mishandled BOM, one field truncated by a stray CR → zero for the track. A parser that is 3× faster and wrong scores nothing, so the brief's advice is explicit: budget time for verification rather than discovering the problem at hour 68.
- Gate 4: the dataset mount is read-only, so we cannot rewrite the input in place. And a copy of the 204MB input staged in /tmp (1GB tmpfs) would be charged against the 8GB cgroup limit *and* inflate `memory.peak` — it would destroy the M axis. **Scratchless.**
- Timeout 369s; a robust parser is far faster than that, so a hang (not slowness) is the only timeout risk.

---

## 2. Axis Budget

**Anchor rows.**

| Row | T ratio | M ratio | S ratio | D ratio | X ratio |
|---|---|---|---|---|---|
| Safe | 20× | 10× | 0.60× | 1.00× | 1.00× |
| Commit | 30× | 12× | 0.62× | 1.00× | 1.10× |
| Stretch | 35× | 12× | 0.62× | 1.00× | 1.10× |

**Full arithmetic.**

- Safe: T 20× ^0.40 = 3.3145; M 10× ^0.20 = 1.5849; S 0.60× ^0.15 = 0.9262; D 1.00×; X 1.00× ^0.10 = 1.0000.
  K = 3.3145 × 1.5849 × 0.9262 × 1.0000 × 1.0000 = **4.8656**.
  speed = 100·ln(4.8656)/ln(25) = **49.15**.
  Absolute read: T = 36.9/20 = 1.845s, M = Mref/10.
- Commit: T 30× ^0.40 = 3.8981; M 12× ^0.20 = 1.6438; S 0.62× ^0.15 = 0.9308; D 1.00×; X 1.10× ^0.10 = 1.0096.
  K = 3.8981 × 1.6438 × 0.9308 × 1.0000 × 1.0096 = **6.0212**.
  speed = 100·ln(6.0212)/ln(25) = 100 × 1.79493/3.21888 = **55.77**.
  Absolute read: T = 36.9/30 = 1.230s, M = Mref/12.
- Stretch: T 35× ^0.40 = 4.1460; M 12× ^0.20 = 1.6438; S 0.62× ^0.15 = 0.9308; D 1.00×; X 1.10× ^0.10 = 1.0096.
  K = 4.1460 × 1.6438 × 0.9308 × 1.0000 × 1.0096 = **6.4041**.
  speed = 100·ln(6.4041)/ln(25) = 100 × 1.85667/3.21888 = **57.69**.
  Absolute read: T = 36.9/35 = 1.054s, M = Mref/12.

**Note on the Commit→Stretch delta.** T 30× → 35× is only +1.9 points (0.40·ln(35/30) = 0.0617 on ln K ⇒ K ×1.0637). The M and S axes are identical between the two rows, so **the entire stretch increment is bought with 0.27s of runtime**. That is a bad trade in absolute terms but the cheapest available one in this track, because every other axis is already at its anchor. The decision rule that follows: once the Commit row is green, the next hours go to the highest-confidence T win; if no T win of ≥3% materializes, the hours go to verification and PERF.md instead (8% of the track total, and the 35% judges' share), not to chasing +1.9 points.

**Sensitivity of the whole table to the two DEFERRED unknowns.**
- Mref is unpublished. Our M design target is expressed as an *absolute* peak, so the ratio can be computed the moment Mref is known. Absolute peak target: ≤ 48MB (see §3) — that is 48MB vs a 204MB input, i.e. we never hold more than a quarter of the input. If Mref = 600MB → 12.5× (anchor met). If Mref = 400MB → 8.3× (anchor missed by 30%; the fix is the buffer pool, §3). If Mref = 1.2GB → 25× (anchor exceeded). **We therefore do not chase M until Mref is known; we hold the absolute budget and convert later.**
- The aggregate key and output format are unpublished, and they determine whether `name`/`note` must be materialized at all. If the output aggregates by region and prints only counts and sums, we never need to copy free text, and the M axis is won outright (we scan the free-text fields without copying them). If the output echoes names, we must copy them and the M axis becomes materially harder. **The materialization policy is a single switch, and it is the biggest M decision in the track.** It is DEFERRED to Oct 28.

**S axis.** A byte-level quoting state machine plus a join plus a sort plus exact decimal formatting is substantially more code than the reference. S target 0.60–0.62 means our normalized source is 1.61–1.67× the reference's. As elsewhere, source bytes are cheap relative to time: doubling S costs 2.1 points; a 1.5× T gain is worth 3.3 points. The code we must write for correctness (the quoting state machine) is not optional, so the S axis is a known, accepted cost.

**D axis.** D = 1 requires std-only. No CSV library, no dataframe library (both banned or forbidden), no `memchr`, no `rust_decimal`, no `rayon`. Decimal handling must be hand-rolled: parse the amount as a scaled integer (see §3) rather than through a library.

---

## 3. Algorithm & Data Layout

**Stage 0 — decide what we must materialize (the column-selective principle).** The parser extracts the five columns, but it only *copies* the columns that reach the output or the join. Under the most likely output (aggregates keyed by the join/aggregate key, with counts and sums), `name` and `note` are scanned but never copied; `id` is used for the record count or the join; `region_id` (or whatever the join key is) is the small integer that drives everything. This single decision is the difference between a 48MB peak and a 500MB peak. It is a *spec-dependent* switch, resolved on Oct 28, and it is the highest-leverage design item in the track.

**Stage 1 — the byte-level quoting state machine.** A single pass over the file's bytes, maintaining a small state:
- OUTSIDE a quoted field: `,` ends a field; a newline sequence ends a record; `"` at the *start* of a field begins a quoted field; every other byte is field content.
- INSIDE a quoted field: a `"` is either a closing quote or the first half of a doubled (escaped) quote — decided by what follows it; a newline is *content*, not a record terminator; a `,` is content.
Two decisions are pinned by the spec and are DEFERRED to Oct 28: (a) whether a `"` appearing in the *middle* of an unquoted field opens a quote or is literal content; (b) whether CRLF inside a quoted field is content or a terminator. Our default readings, to be confirmed: (a) a quote is only special at field start (RFC 4180 behavior); (b) a newline inside a quoted field is always content, including a CRLF pair, because the sample shows a bare LF inside a field and the spec says the quoting rules are "not negotiable" — the conservative reading is that quoted content is opaque until the closing quote.
Implementation shape: a byte-class table (256 entries) classifying each byte as DELIM, QUOTE, NEWLINE, CR, or ORDINARY, so the hot loop is a table lookup and a switch rather than a chain of comparisons. Inside a quoted field we can run a tight "scan to the next quote" loop using the same table, which is where most of the bytes are.

**Stage 2 — record framing and the embedded-newline problem.** A record ends at a newline that is *outside* a quoted field. This is the whole difficulty of the track, and it determines the parallelization strategy.

- **Sequential option (S):** one thread scans the whole file with the state machine, emitting records into a bounded queue. Simple, obviously correct, and the parse is a single linear byte pass.
- **Parallel option (P):** split the file into 12 byte ranges and parse them concurrently. This requires knowing, for each range start, whether we are inside a quoted field. The classic solution: a **quote-parity prefix scan** — a fast pass that counts quote bytes per fixed-size block (e.g. 64KB) in parallel, giving a prefix count of quotes before each block. Since a doubled quote contributes two quote bytes, the parity of the quote count determines the in/out state at any byte offset, provided every quote byte toggles the state. Caveat: this requires the assumption that every `"` byte toggles the in/out state, which is true under reading (a) above only if quotes never appear in the middle of unquoted fields as literals. If the spec's quoting rules differ, parity scanning is invalid and we must fall back to option (S) or to a re-parse-from-the-last-record-start approach.
  **Decision: implement the parity scan (it is a 10-minute, ~20ms job) and *validate* it against a sequential scan of the first 64MB at H44. If the two disagree on any record boundary, disable the parallel path.** That validation is the safety net that lets us take the speed.
- **Fallback parallel option (P2):** each worker begins at a candidate newline near its range start, parses forward, and if it discovers it started mid-field it re-parses from the previous record start. This is correct without any assumption about quote semantics, at the cost of a small re-parse. It is the fallback if the parity assumption fails.

**Stage 3 — the join.** Hash join vs sort-merge join. The brief says the answer "isn't obvious" and this is a genuine decision point.
- The lookup table's size and key type are DEFERRED. If the lookup table is small (say ≤ 10M rows), it fits in a hash table and the hash join is O(N) with one probe per input record — clearly better.
- If the lookup table is large (≫ the input's distinct keys), a sort-merge join over the input's key column wins on memory.
- **Design: choose at runtime.** Read the lookup table first (it is a separate mounted input, DEFERRED path), measure its row count, and select the join strategy accordingly. Both strategies are implemented; the selector is one line of logic and one measurement. This is a strong "evidence of decisions" item for the judges' Technique score.
- Hash join details: the key is `region_id` (or the published join key). The lookup table is loaded into an open-addressed hash table with a deterministic probe sequence; the input probe is a direct integer probe if the key is an integer (the published sample uses small integers like 7, 12, 3) — an integer-keyed direct-address table (a flat array indexed by the key) is dramatically faster than hashing and is obviously correct when the key range is small. **Decision: if the join key is a small-range integer, use a direct-address array; if it is a string, use a hash table.** The direct-address path is the fast path and it is trivial to verify.
- Sort-merge details: extract the key column into a compact array of (key, payload), sort by key (radix on the key, parallel), then merge against the sorted lookup table. Peak memory = the extracted column, which is much smaller than the full records.

**Stage 4 — the aggregate.**
- The aggregate key is DEFERRED. Likely a small integer (region) or a string. The aggregate measures are DEFERRED (count, sum of amount, maybe min/max, maybe a per-status breakdown).
- **The amount is the correctness trap.** The published values are `1420.00`, `88.50`, `0.00`, `-5.25` — two decimal places, with a sign. Summing them in binary floating point and formatting can differ from the reference by a rounding step, and the output is a byte hash. **Design: parse the amount into a scaled integer (minor units, scale 2 — and if the spec allows other scales, keep the scale and the digit count) and sum integers; format the integer back with the exact decimal representation.** Integer addition is associative and exact, so the sum is independent of the order of aggregation — which also removes a parallelism determinism hazard.
  Risk: if the reference sums in f64 and prints with a formatter that rounds, an exact integer sum could still differ (e.g. 0.1 + 0.2 = 0.30000000000000004 in binary). **Mitigation: implement both an exact-decimal path and an f64 path, and use the 10% sample hash to determine which matches.** That comparison costs one afternoon of the budget and is the highest-value verification activity in the track.
- Output ordering is DEFERRED ("emit sorted aggregates"). Sorted by the aggregate key; if the key is an integer, numeric order; if a string, byte-lexicographic order. The spec pins it; we implement one comparator, selectable.
- Output formatting: the number of decimal places, the thousands separator (if any), the sign, and the line terminator are all DEFERRED and all byte-visible. One formatting module, one switch.

**Stage 5 — output.** Aggregates are emitted directly to /out in a single streaming pass from a pre-sized buffer. The aggregate space is small (a few hundred to a few million rows), so the output path is not a bottleneck.

**Memory budget (the "materialization policy = minimal" case).**

| Structure | Bytes |
|---|---|
| Read buffer pool, 12 workers × 1MB | 12.6 MB |
| Record assembly (only the fields we materialize) | ~0 (fields are consumed in place) |
| Lookup table: direct-address array for small integer keys, or hash table for string keys | 0.5–20 MB (DEFERRED) |
| Aggregate table (key → count, sum) | < 8 MB (DEFERRED) |
| Decimal parse scratch | negligible |
| **Total** | **≈ 24–48 MB** |

At 24–48MB against a 204MB input, Mref/M ranges from ~8× to ~50× depending on Mref. If Mref lands at 400MB, our 48MB is 8.3× and we are short of the 12× anchor; the fix is to shrink the read buffer pool (12 × 256KB = 3MB) and, if needed, to parse with a smaller working set. **The M axis is essentially decided by the materialization policy, not by micro-optimization.**

---

## 4. Hash-Gate Correctness & Determinism

**Determinism rule.** The output must be a pure function of the input bytes. Every aggregate is computed in integers; every ordering is explicit; every worker writes to a fixed slot.

| Hazard | Elimination |
|---|---|
| Floating-point summation order | Amounts are summed as scaled integers. If the f64 path is required by the spec, the summation order is fixed (single-threaded reduce in a defined order) and the f64 path is the last resort |
| Parallel aggregation order | Integer sums are order-independent; counts are integers. Any non-associative quantity (a floating-point sum) is reduced in a fixed worker order |
| Hash-table iteration order affecting output | The aggregate table is iterated through an explicit sort on the aggregate key, never through table order |
| Embedded newlines splitting one record into two | The state machine distinguishes in-field from out-of-field newlines; a record counter must reconcile to the expected record count (the expected count is known from the spec or measured on the sample) |
| BOM handling differing by chunk boundary | BOM removal happens at exactly two defined places: byte 0 of the file, and a chunk start that the parity scan identifies as a record start. A BOM anywhere else is data. **One function, two call sites, no other** |
| Trailing CR left on the last field | Line terminators are recognized as CRLF or bare LF; the CR is consumed as part of the terminator, never as field content. Inside a quoted field, a CR is content — pinned by the spec, DEFERRED |
| Whitespace trimming | No trimming. `"trailing space "` keeps its trailing space |
| `""` vs absent field | Distinguished by a per-field "was quoted" flag, retained only if the output format needs it |
| Field-count mismatch on a malformed row | A row with the wrong number of columns is a hard error: counted, reported, and (per the spec's rules, DEFERRED) either rejected or handled by a documented policy. Never silently shifted |
| Header row treated as data | The header is identified by its position (first record) and by its content; a policy switch. The published sample's first line is a header, and whether headers are present in the scored input is DEFERRED |
| Decimal scale differences (2 dp vs 1 dp vs 0 dp) | The parsed value carries its scale and the exact digit string; the output formatter is spec-driven. If the aggregate mixes scales, a fixed promotion rule (to the maximum observed scale) is used, and the promotion is verified against the sample hash |
| Rounding mode | Integer formatting only: no rounding of stored values. If the spec requires rounding at output, the rule is pinned (half-up vs half-even is DEFERRED and must be read off the spec) |
| Locale | No locale-sensitive formatting; digits emitted directly |

**Verification ladder (the track where the budget goes).**
- **V1 (baseline, H0–H2):** the 10% sample ships with its expected hash. The baseline implementation must reproduce it before anything else starts.
- **V2 (differential oracle):** a second implementation, written in the baseline window, that uses an obviously-correct-but-slow strategy: split the file into records with a simple, heavily commented state machine that makes no assumptions (one byte at a time, explicit rules), and compute the aggregates with a straightforward hash map. Every optimized path must match the oracle field-by-field, record-by-record, not just on the final hash. **For this track the oracle is the product.**
- **V3 (hostile-feature test matrix):** a table-driven test that runs both implementations over a synthetic corpus exercising each of H1–H12 in isolation and in combination:
  - BOM at byte 0 only; BOM at byte 0 and mid-file; BOM mid-file only; BOM inside a quoted field (must be preserved as data).
  - CRLF-only file; LF-only file; mixed file; a file whose last line has no terminator.
  - A quoted field containing a comma; a quoted field containing a doubled quote; a quoted field containing a bare LF; a quoted field containing a CRLF (the DEFERRED case, tested both ways so the switch is proven to work).
  - Empty quoted field, empty unquoted field, a field with only a space, a field with a trailing space.
  - Negative amount, amount with one decimal, amount with no decimals, amount with a leading plus, amount with leading zeros.
  - A record with a missing trailing column, and a record with an extra column.
  Expected outputs are hand-derived from the five published lines and their variants.
- **V4 (record-count reconciliation):** the number of parsed records must equal the number of records the oracle finds, and the sum of per-aggregate counts must equal the total record count. This single invariant catches the most common silent failure (an embedded newline or a BOM creating a phantom record).
- **V5 (parallel-vs-sequential equality):** the parallel parse (with the parity scan) must produce exactly the same record sequence as the sequential parse. This validates the parity assumption directly rather than by inference.
- **V6 (repeat runs and worker counts):** identical hashes across 1, 6, and 12 workers and across 5 runs.
- **V7 (spec-conformance sweep at H44):** a deliberate re-read of the Oct 28 spec against the implementation, line by line, for the quoting rules, the BOM rules, the number format, the ordering, and the header policy. Two people (or two passes separated in time) read the spec; disagreements are resolved before optimization continues. This is cheap and it is where a disqualification is actually caught.

**Gate 4 compliance.** No network. The dataset mount is read-only and we never attempt to write to it. The root filesystem is read-only. Scratch: we use **no scratch at all** — no temp files, no /tmp staging, because tmpfs pages count against the 8GB cgroup limit and against `memory.peak` and would erase the M axis. The only writes are the output file to /out. No answers are precomputed into the image: every value is computed at run time from the mounted input.

**Gate 3 compliance.** 369s timeout versus a 1.0–1.9s target. A hang is the only risk, and the only plausible source is a state-machine loop that fails to advance (e.g. a quoted field whose closing quote is missing). Mitigation: a per-record byte budget; if a record exceeds a sane limit, the parse aborts loudly with a diagnostic instead of spinning.

---

## 5. Optimization Ladder

| # | Rung | Hypothesis / expected effect | Risk | Acceptance |
|---|---|---|---|---|
| L0 | Baseline: robust byte-level state machine, one thread, hash join, integer amount sum, explicit sort of aggregates | The oracle; V1 hash; T ≈ reference class | Low | Sample hash matches; record count reconciles |
| L1 | Column-selective materialization: never copy `name`/`note` unless the output needs them | The dominant M win (potentially 8× vs 25× on the M factor); also removes the copy cost from the T axis | Medium (depends on the output format) | Oracle agreement; memory.peak recorded |
| L2 | Byte-class table for the parser hot loop (one lookup per byte instead of a comparison chain) | Expected 1.5–2.5× on the parse phase; the parse is the largest single time component | Low | Oracle agreement; ≥3% T gain |
| L3 | In-quoted-field fast scan: skip to the next quote/newline using the class table without per-byte state updates | The bulk of the file is inside quoted free-text fields; this is where the win is | Medium (escape handling at the boundary) | Oracle agreement, including the doubled-quote case |
| L4 | Parallel record framing via the quote-parity prefix scan, with the sequential scan as the validated fallback | 6 physical cores → 2.5–4× on the parse phase; this is the rung that turns a 20× T anchor into 30× | **High** (the parity assumption) | V5 parallel-vs-sequential equality; parity scan validated against a sequential scan of the first 64MB |
| L5 | Direct-address join table for small integer keys (no hashing at all) | The join is a probe per record; direct addressing removes the hash and the compare | Low (if the key is a small integer) | Oracle agreement on every aggregate |
| L6 | Alternative: sort-merge join (radix sort of the extracted key column, then a linear merge) | The brief says the choice "isn't obvious"; the runtime selector picks whichever wins | Medium | Adopt only if it beats L5 by ≥3% on the measured input |
| L7 | Integer decimal parsing and formatting (minor units, exact digits) | Removes float parsing and formatting from the hot path; also removes a determinism hazard | Medium (scale/rounding mismatches) | Sample hash equality on the amount columns |
| L8 | Parallel aggregation: per-worker hash tables merged in a fixed worker order | The aggregate phase parallelizes if the join phase does | Low (integer sums are order-independent) | Oracle agreement; V6 |
| L9 | Output path: streaming emission, hand-rolled decimal formatting, one formatting module | Small T win; removes the last float-formatting risk | Low | Hash unchanged |
| L10 | Profiling-driven second pass on the top-2 remaining cost centres | Diminishing but cheap | Low | ≥3% T gain each, or reverted |

**Explicitly rejected rungs (for PERF.md).**
- **Splitting on newlines and commas (the classic naive approach):** it is what the organizers call "3× faster and wrong" — it fails on H3, H5, and H6 simultaneously. It is implemented only inside the oracle, in a deliberately naive form, and is labeled as such.
- **A generic CSV library:** banned, and it would set D above 1 as well.
- **A dataframe/analytics library:** banned explicitly by the brief.
- **Writing the parsed records to /tmp as an intermediate (a "stage the file" design):** it would be simpler to parallelize, but it charges tmpfs against the 8GB cap and destroys the M axis. Rejected on measured grounds, not on principle alone — but it is a real principle here.
- **Stripping all BOMs globally:** corrupts a BOM that is legitimate field content. Rejected on correctness grounds.
- **Floating-point accumulation of the amount column:** accepted only as a fallback if the integer path provably disagrees with the sample hash.

**PERF.md content plan.** A per-rung table: parse time, join time, aggregate time, blended T, memory.peak, plus a "what broke" column. A dedicated section listing the twelve hostile features and, for each, the test that covers it. A dedicated section showing the parallel-vs-sequential validation for the parity scan. Judges read this without the leaderboard, so the document must be self-explanatory: the hypothesis, the measurement, and the decision.

---

## 6. X-Scaling Safeguard

**What X measures.** X = (runtime at 10× input) ÷ (runtime at 1×) versus the reference's own ratio. Weight 0.10. Targets 1.00 (Safe) to 1.10 (Commit/Stretch) — the tightest X budget in the campaign, because Track C's natural scaling is the most linear of the five: it is a streaming, single-pass, O(1)-per-record design.

**Scaling analysis of each stage.**
1. *Parse.* A single linear pass over the bytes: strictly linear. The parity scan is linear and parallel. **No superlinearity.**
2. *Record emission.* Linear in the number of records. Aggregates grow with the number of *distinct keys*, which grows sublinearly with the input (the key space is bounded by the lookup table's key set). **Sublinear, favorable.**
3. *Join.* A probe per record: linear. The direct-address table is sized by the key range, which is fixed by the lookup table, not by the input size. **Linear; the table does not grow with the input.**
4. *Sort of the aggregate space.* The aggregate space is bounded by the number of distinct keys — essentially fixed as the input grows. **Constant.**
5. *Output.* Bounded by the aggregate space. **Constant.**
6. *Memory.* Only the read-buffer pool scales with... nothing; it is fixed. The aggregate table is bounded by the key space. **Memory is essentially constant as the input grows** — this is the strongest X profile in the campaign and a genuine M-axis advantage.

**The one hazard: the 10× input's record distribution.** If the 10× input is the same file concatenated ten times, the aggregate space does not grow and everything is linear → X_local ≈ 1.00–1.03. If the 10× input has ten times more distinct keys, the aggregate table grows 10× and the sort grows n log n → X_local ≈ 1.05–1.10. Both are inside our target band.

**The second hazard: the reference's own X.** The reference materializes every field and builds a per-record structure; at 10× input (2.04GB) its memory grows roughly linearly, and if it approaches the 8GB cap it may OOM or thrash, making its 10×/1× ratio much worse than 1.0. In that case our X ratio is *better* than the reference's and the X axis becomes a small positive contribution rather than a neutral one. That is the intended shape of the axis.

**Safeguards.**
- **S1.** Fixed-size read buffers; the buffer pool size is a constant, not a function of the input size.
- **S2.** The aggregate table is bounded by the lookup table's key set; we never build a per-record structure.
- **S3.** The direct-address join table is sized from the lookup table, never from the input.
- **S4.** The parse remains strictly single-pass and streaming: we never materialize the record set, at 1× or 10×.
- **S5.** Probe construction is DEFERRED (the organizers' 10× input is unpublished). We build our own 10× probe by concatenating the file into dev scratch on the host volume — never /tmp, because 2.04GB of tmpfs would exceed the 1GB tmpfs limit outright and would also charge the cgroup. Concatenation is a faithful probe for this track: it reproduces the byte-level hostility (BOM, mixed terminators, embedded newlines) ten times over and stresses exactly the paths we care about.
- **S6.** Acceptance: X_local ≤ 1.10 and 10× peak ≤ 1GB. If X_local > 1.15, ablate: parse only, join only, aggregate only — identified within one hour, since the expected answer is "the aggregate table".
- **S7.** Never trade a verified T win for an X fix: 0.10 weight means a 0.10·ln(1.15) = 0.014 penalty on ln K (K ×0.986, about −0.3 points) against 3+ points for a 20% T gain.

---

## 7. Hour Budget

Total charged to Track C: **10h = 2h baseline + 8h optimization**. The baseline is written inside the H0–H17 window; the optimization block is H43–H51. This track gets the second-largest optimization block because it carries the highest disqualification risk and the highest verification cost.

**Baseline block (2h, charged), H0–H2 nominal.**

| Hour | Activity | Exit criterion |
|---|---|---|
| H0–H0.5 | Read the Oct 28 frozen spec for Track C: quoting rules, BOM rules, header presence, join key and lookup schema, aggregate key and measures, output format and number formatting, record count | Spec sheet complete; every unknown listed as DEFERRED with a named owner |
| H0.5–H1.5 | Baseline implementation: one-thread byte-level state machine, integer amount sum, hash join, explicit aggregate sort, spec-driven output | Runs end to end on the 10% sample |
| H1.5–H2 | V1: sample hash matches; record count reconciled; measure T and memory.peak with the 5-run protocol | Sample hash reproduced; baseline recorded |

**Optimization block (8h, charged, H43–H51).**

| Hour | Activity | Exit criterion |
|---|---|---|
| H43 | L2 (byte-class table) + L3 (in-field fast scan) | Oracle agreement; ≥15% T gain on the parse phase |
| H44 | L1 (column-selective materialization) + L5 (direct-address join) — **and the L4 parity-scan prototype validated against a sequential scan of the first 64MB** | Oracle agreement; memory.peak down; parity assumption proven or the parallel path is disabled |
| H45 | L4 (parallel framing) + L8 (parallel aggregation) | V5 parallel-vs-sequential equality; V6 worker-count invariance |
| H46 | L7 (integer decimal path) and the **f64-vs-integer comparison** against the sample hash; switch to whichever matches | Sample hash equality on the amount columns; the comparison is recorded in PERF.md |
| H47 | L6 (sort-merge join alternative, measured head-to-head against the hash/direct join) | Keep the winner only if it wins by ≥3%; record both numbers |
| H48 | L9 (output path) + L10 (profiling-driven second pass) | Each change measured independently |
| H49 | **Verification day part 1:** V3 (hostile-feature matrix, 20+ cases), V4 (record-count reconciliation), V7 (spec-conformance re-read) | All green; no unknown spec behavior remains untested |
| H50 | **Verification day part 2:** V2 oracle diff over the full dataset, V6 (5 runs, worker counts), the 10× X probe, memory.peak at 1× and 10× | All green; X_local ≤ 1.10 |
| H51 | Freeze: re-anchor the axis table to the batch-local reference; recompute K; write PERF.md including the hostile-feature table and the failed experiments; measure normalized source bytes | Submission frozen |

**Budget hygiene.**
- **Two of the eight optimization hours (H49–H50) are verification.** That is deliberate and non-negotiable: the organizers say most disqualifications happen in this track, and a zero here is worse than a 45-point submission anywhere else.
- The decimal-path decision (H46) is scheduled early enough that if the integer path is wrong we have H47–H48 to implement the fallback, and H49–H50 to verify it.
- The parity-scan validation is scheduled at H44, the *first* hour it can be built, so that a failure of the assumption costs one hour and not six.
- If the track is behind at H49, the optimization stops and the verification proceeds. The last green state ships.

---

## 8. Risk Register & Stop Rule

| ID | Risk | Likelihood | Impact | Detection | Mitigation | Stop rule / trigger |
|---|---|---|---|---|---|---|
| R-C1 | The quoting rules are misread (quote in mid-field, CRLF inside a quoted field, doubled-quote semantics) | **High** | **Total** (Gate 1 = 0) | V3 matrix; V7 spec re-read; oracle diff | The spec sheet is written before the code; every rule is one named constant in one place; the matrix tests each rule both ways | If a rule is still ambiguous at H44, implement both readings behind a switch, and use the sample hash to select — but only if the sample exercises the case; if not, choose the reading the spec's wording supports and document the risk |
| R-C2 | BOM policy wrong (which BOMs to strip) | Medium | **Total** | V3 cases; oracle diff | Exactly two strip points (file start, validated chunk start); BOM elsewhere is data | Any disagreement → fix the strip policy, re-verify |
| R-C3 | The amount's decimal scale or the reference's float summation differs from our integer path | **High** | **Total** | H46 comparison against the sample hash; oracle diff | Both paths implemented; the sample hash decides | If neither path matches at H47, we ship the path that matches the *reference re-run* behaviour observed in a local re-implementation of the reference's formatter — and we re-verify in H49–H50 |
| R-C4 | The output format (ordering, decimal places, terminators) is wrong or unknown | Medium | **Total** | V1 hash; V7 re-read | One rendering module with selectable policies | If unresolved at H49, render multiple candidates and keep the hash-matching one |
| R-C5 | The join key / lookup schema / aggregate key are misidentified (DEFERRED) | Medium | **Total** | V1 hash; oracle diff | Read the spec at H0; confirm against the published sample (region_id 7, 12, 3) | If the sample is ambiguous, implement the aggregate over the most plausible key and re-check at H49 |
| R-C6 | The parity-scan assumption is invalid (a quote byte in the middle of an unquoted field) | Medium | High (correctness or lost parallelism) | V5 parallel-vs-sequential equality; the 64MB cross-validation at H44 | Validate the assumption explicitly before enabling the parallel path; keep the sequential path as the fallback | If validation fails at H44, disable L4 permanently and re-plan the T anchor (the Commit row's 30× becomes 20–25×) |
| R-C7 | Embedded-newline record boundary error (a phantom or split record) | Medium | **Total** | V4 record-count reconciliation; oracle diff | The state machine distinguishes in-field and out-of-field newlines; a count invariant | Any count mismatch → stop, fix the framing, re-verify; never ship an unreconciled parse |
| R-C8 | Memory blowup from materializing free text | Medium | Medium (−2 to −8 points) | memory.peak at H1 and H44 | Column-selective materialization; no whole-file residency; no /tmp staging | If memory.peak exceeds 128MB at 1×, cut materialization further before any speed work |
| R-C9 | Hash join vs sort-merge join is the wrong choice for this input | Medium | Medium | Separate join-phase timings | Runtime selector between both strategies, both implemented | If the selected strategy is within 3% of the other, keep the simpler one (direct/hash) |
| R-C10 | Parallel aggregation introduces nondeterminism | Low | **Total** | V6 | Integer sums are order-independent; fixed worker merge order | Any mismatch → single-threaded aggregation |
| R-C11 | The 10× probe cannot be staged (2.04GB dev scratch) | Medium | Low | Probe setup at H50 | Host scratch volume, never /tmp; a 2× probe as a fallback with linear extrapolation | If impossible, document the linear-scaling argument in PERF.md |
| R-C12 | Source-size overrun (S ratio below 0.60) | Medium | Low (−2 points) | Normalized byte count per rung | Budget ≤1.7× the reference; cut L10 and L6 first | Over budget → cut features until within 1.7× |
| R-C13 | Reference drift >2% voids a batch | Medium | Medium | Batch-local reference timings | Re-derive ratios from the batch-local reference; re-measure after any drift | Discard the batch, re-measure, do not re-tune to a drifted number |
| R-C14 | Time runs out because the team spent the block on verification and never optimized | Low | Medium | Hourly tracking against the ladder | Verification is budgeted (H49–H50), not deferred; the ladder's early rungs (L2, L3, L1) are high-yield and cheap | If at H48 the T anchor is unmet and verification is green, spend H48–H49 on the top rung only, and hold H50 for verification regardless |

**Global stop rule for Track C (in priority order).**
1. **Correctness stop:** if at H49 the oracle diff is non-empty for any record or any aggregate, the optimization ends and the last green state ships. **A fast wrong answer scores zero; a slow right answer scores 45+.**
2. **Spec-conformance stop:** if the V7 re-read finds any divergence between the implementation and the frozen spec, fixing it takes priority over every remaining performance item, regardless of the hour.
3. **Parallelism stop:** if the parity scan cannot be validated, the parallel parse path is disabled — permanently, not provisionally.
4. **Diminishing-returns stop:** two consecutive rungs below 3% ⇒ stop the ladder.
5. **Memory stop:** memory.peak above 128MB at 1× ⇒ cut materialization before any speed work.
6. **Freeze:** at H51 the last green, verified, deterministic build ships with the re-anchored axis table and a PERF.md that documents the hostile-feature matrix.

**Definition of done for Track C.** The published 10% sample hash is reproduced; the full-dataset output hash matches on 5 of 5 runs; the record count reconciles; the optimized result matches the baseline oracle field-by-field over the whole dataset; the hostile-feature matrix (H1–H12) passes in both the baseline and the optimized implementation; memory.peak is recorded and ≤128MB; the 10× probe reports X_local ≤ 1.10; PERF.md contains the per-rung timings, the hostile-feature coverage table, the integer-vs-float comparison, and the failed experiments.
