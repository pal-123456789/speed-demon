# TRACK C · JOIN — INSANE-LEVEL DEEP DIVE

> Pre-kickoff design. No solution code. Companion to `docs/specs/track-c-join.md`.
> **Engine:** hostile-CSV ETL join + money-safe group aggregation → billing pipeline.
> **Ladder goal:** L3. **Contract anchors:** ref **T 14.44 s**, timeout 144 s;
> input 204 MB hostile CSV, **3.32M records, 5 fixed columns WITH a header row
> `id,name,note,region_id,amount`** (events page sample), join vs a lookup
> table, **emit sorted aggregates**; join key, lookup schema, and output format
> DEFERRED to Oct 28; banned: any analytics / dataframe library. **Absolutes:**
> Commit T ≈ **0.48 s** (30×), Stretch ≈ 0.32 s (45×).

---

## 1. The complete feature inventory

### P0 — correctness-critical (the quoting rules ARE the task)

| # | Feature | Why it exists / the trap | Oct-28 pin needed |
|---|---|---|---|
| C01 | **The quoting ORACLE — a scalar, byte-exact CSV state machine** | Camps yes/no on: quote toggling, `""` escape inside quotes, delimiters and newlines inside quoted fields, CRLF handling, trailing spaces inside quotes, a bare quote in an unquoted field, a BOM mid-file. Every organisers' DQ lands here. The oracle decides record boundaries and field values | every rule in `quoting`, `bom_handling`, `newline_in_quotes` |
| C02 | **Record separation respects quotes**: a newline inside a quoted field is data, not a record end | The #1 correctness separation — a SIMD naive LF-split corrupts everything | newline-in-quotes |
| C03 | **5 fixed columns parsed in order**; the join-key column identified | The events page pins them: **`id,name,note,region_id,amount` with a header row**; column misalignment = silent wrong join | join-key position (frozen spec) |
| C04 | **Amount as DECIMAL, integer math only** (scale 2; rounding mode) | Money in floats = wrong pennies; round-half-up vs banker's is a SHA difference; negative-zero spelling | amount scale + rounding |
| C05 | **Group aggregation, SORTED output** (events page: "emit sorted aggregates") | The aggregate shape + its sort order are the output identity | aggregate key + sort order (sorted = pinned; the key/sort bytes = frozen spec) |
| C06 | **Lookup join** (records → lookup by key; missing-key spelling) | Join makes it Track C, not a pure group-by; the lookup's own quoting rules apply if it's CSV too | lookup filename/schema + missing-key output |
| C07 | **Deterministic number formatting** (fixed 2 decimals, no exponent, padding) | `12.3` vs `12.30` vs `12.30\n` — all SHA | output number format |
| C08 | **Trailing-newline / separator policy on output** | bytes | trailing newline |

### P1 — the Commit row

| # | Feature | Why | Notes |
|---|---|---|---|
| C11 | **SIMD quote/locator pass is ONLY a candidate locator**: scan 64 B lanes for `"`/`,`/`\n` hints, but the scalar oracle decides every record boundary and field value | "Never let a fast path near the quoting rules" — the locator suggests split points, the oracle confirms | the doctrine from `docs/specs/track-c-join.md` |
| C12 | **Shard the CSV only at oracle-certified record boundaries** (find the previous record end per 1 MiB block) | 12-thread parse without splitting a record | — |
| C13 | **Key interning**: join keys → dense u32 via one hash pass, then array lookups | The Zipf-keyed group table stays small | — |
| C14 | **Chunked group accumulation: (count, cents-sum) per group id, sum in i64 cents** | i64 cents is exact for any realistic total; no float anywhere | — |
| C15 | **Single-pass design: parse → join → aggregate, one streaming pipeline** | 204 MB read once; no intermediate files (sandbox forbids caching, memory hates staging) | — |
| C16 | **Lookup table loaded and hashed FIRST** (build the join side before the records side) | One controlled pass over the small lookup, then the big pass over records is pure streaming | lookup size is small |

### P2 — the insane toppings

| # | Feature | Why | Risk |
|---|---|---|---|
| C21 | **SIMD quote-mask precompute** (portable 16-lane ASIMD-style masks for `"` `,` `\n`) that FEEDS the oracle's per-field decisions | Cuts the oracle's byte visits ~4× | only after C01 is fixture-proven on every hostile case |
| C22 | **Fast decimal parse for the amount column** (digits → i64 cents with scale, overflow-checked) | Amount parse is the hot field; generic `parse::<f64>` is slow AND wrong | fixed scale assumption |
| C23 | **Output built in a reusable buffer with precomputed decimal tables** | Formatting 100 k+ lines naively costs; table-driven fixed-width 2-decimal emit | byte-match C07 |
| C24 | **Per-shard group tables merged deterministically by group id** | The parallel story | never hash-order |
| C25 | **Lookup resident in a perfect-ish hash** (open addressing, key → id) | Join lookups become ~1 cycle | key count small |

## 2. The ladder

```
L0  reference-equivalent (python, ~14.4 s)      H0–H1   — the SHA oracle
L1  correct scalar Rust: oracle + streaming     H1–H4   — target ~1.5 s, SHA-green
L2  parallel records + join + aggregate         H4–H8   — target ~0.5 s
L3  insane (C21–C25)                            H8+     — target ~0.3 s; SHA-green per rung
```

## 3. Determinism traps (specific to C)

- **Group emission order** — the #1 trap. Aggregate map iteration is forbidden:
  group ids sorted, emitted in id order (or the pinned sort key).
- **Integer cents, never floats** — a `f64` accumulator gives a DIFFERENT last
  penny on different thread counts. i64 cents is exact and parallel-safe.
- **Rounding order with parallelism** — if the spec rounds per record vs per
  group sum, the answers differ; pin it; if per-group, round ONCE on the merged sum.
- **The oracle must be byte-exact** — every quoting rule gets a fixture row;
  a "close enough" oracle is a zero.
- **CRLF normalization** — inside quotes CRLF may be data; decide and never
  normalize blindly.
- **BOM** — mid-file BOM starts a field with U+FEFF per the pinned rule, or is
  stripped; write the fixture exactly as the spec says.

## 4. Memory budget (M target « 1 GB; data is 204 MB — stream it)

| Structure | Size |
|---|---|
| Streaming read buffers (12 × 1 MiB shard) | 12 MB transient (reused) |
| Group table (keys → [count, i64 cents]) | small — thousands of keys |
| Lookup table | lookup-size, resident |
| **The CSV itself** | **never retained** |

Peak is dominated by the read shards; keep them small and reused so
`memory.peak` stays far under the 8 GB cap and M ratio ≈ reference-friendly.

## 5. Parallelism plan

- 12 shard workers: each takes a 1 MiB block, walks back to the previous
  oracle-decided record boundary, parses its records through the oracle, joins
  and accumulates into its own group table.
- Merge: group ids sorted, sums summed (i64), rounding applied once, formatted
  once, emitted. Output order = pinned sort; thread count never affects bytes.
- The lookup table is immutable after load → lock-free.

## 6. Edge-case matrix (every one becomes a fixture)

| Hostile input | Decision | Fallback |
|---|---|---|
| Quoted field with an embedded LF | kept in the value (escaped in output?) | per C01 |
| `""` inside a quoted field | becomes one `"` | per C01 |
| Trailing spaces before the closing quote | kept | sample says yes |
| A quote in an unquoted field | illegal → how does the oracle react? | spec |
| BOM at byte 0 vs mid-stream | strip vs data | per C02 |
| CRLF record endings mixed with LF | both accepted; output normalized to LF | per C03 |
| Missing lookup key | spelled output (empty? literal?) | per C06 |
| Amount like `-0.05` | negative values legal; negative zero `-0.00`? | per C04 |
| Empty quoted field `""` | zero-length value vs null | spec |
| Column count short/excess on a line | oracle policy: skip, error, fill? | spec |
| 1 MB single quoted field | validation fixture; oracle must not blow the stack | bounded state machine |

## 7. Validation plan

- **Fixture-first:** port every hostile case below into `gen-fixtures` hostile
  CSV cases BEFORE writing the implementation; the oracle is built against the
  fixtures, not against hope. The events page's five sample records are the
  oracle's ACCEPTANCE TEST — copy them byte-for-byte (BOM at file start AND a
  mid-file BOM, CRLF mixed with bare LF, `""` escapes, embedded comma+LF,
  trailing space in quotes, `-5.25`, `0.00`, empty quoted fields):
  ```
  ⟨BOM⟩id,name,note,region_id,amount␍␊
  1,"Kaphar, Titus","said ""no"" twice",7,1420.00␍␊
  2,"Betts","a note that␊spans two lines",7,88.50␊
  3,,"",12,0.00␍␊
  4,"trailing space ","comma, quote "" and newline␊together",3,-5.25␊
  ```
- `difftest` vs L0 on sample + every fixture; byte-identical across 1/6/12 threads.
- The SHA gate pins `join.txt`; re-pin per rung.

## 8. H0 window runbook (first 6 hours)

1. **H0–H1:** read the frozen quoting/amount/group pins; convert them into the
   fixture matrix (C01–C08) the same hour.
2. **H1–H4:** L1 — scalar oracle + streaming pipeline, SHA-green. This is the
   hardest correctness of all four tracks; do NOT parallelize before it is green.
3. **H4–H8:** L2 parallel records. Bench each rung vs same-session ref.
4. **H8+:** L3 (locator SIMD) only after the oracle is fixture-clean.
5. **Freeze-day:** determinism ×3, final SHA, PERF.md ladder (the organizers
   reward the "experiments that failed" table on this track especially).

## 9. Open questions → Oct 28

- The full quoting rule set (C01), BOM policy (C02), record-ending policy (C03).
- Amount scale/rounding incl. negative zero (C04), aggregate + sort (C05),
  lookup filename/schema + missing-key spelling (C06), output formatting (C07).