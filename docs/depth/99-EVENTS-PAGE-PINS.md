# EVENTS-PAGE PRE-PINS — verified against the live official page

> Fetch date: **Oct 27** (this document). Source: https://speeddemon.dev/ (official,
> pre-freeze) and https://speeddemon.dev/allowlist.json. Everything below is quoted
> or paraphrased from the live page and pre-pins values that would otherwise wait
> for the Oct 28 freeze. Verify each line against the frozen spec anyway — the
> freeze replaces any wording, but nothing on this page is expected to change.
>
> **Rule 04/05 status:** research and design only — no solution code.

## 1. Digest allowlist (cryptographically verified)

`tooling/base-image.txt` is compared against the live
[allowlist.json](https://speeddemon.dev/allowlist.json) — both digests match:

| Image (in our Dockerfiles) | Digest | Allowlist entry |
|---|---|---|
| `debian:13-slim` | `sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad` | "runtime stage" ✓ |
| `rust:1-slim-trixie` | `sha256:1a939748a7cbab172de91a014e3804a97af1394a8c844faa67fce9401c4e40fb` | "Rust" ✓ |

Allowlist note (verbatim): *"Every FROM must be one of these images pinned by its
multi-arch index digest, or an earlier stage of the same Dockerfile."* — our
two-stage build (rust build → debian runtime) is exactly the permitted shape.
`check_base_pinned.py` enforces FROM ∈ allowlist + runtime stage = debian.

## 2. Track A — Ingest (mostly PRE-PINNED by the page)

> "Aggregate 3.1M structured log lines into **per-endpoint request counts,
> p50/p95/p99 latency, and error rates bucketed by hour**."

| Item | Pinned by the page | Still DEFERRED to the frozen spec |
|---|---|---|
| Grouping key | **endpoint** | — |
| Hour buckets | **bucketed by hour** | hour-bucket spelling (RFC3339 hour vs integer) |
| Percentile list | **p50, p95, p99** | nearest-rank vs interpolated; index semantics |
| Error class | **status 5xx** ("5xx counts toward the error rate") | error-rate format (fraction/%/decimals) |
| Schema | `ts` RFC3339 µs · `method` 7-enum · `endpoint` string · `status` 100–599 (3 digits) · `latency_us` uint32 **microseconds** | the delimiter rule |
| Structure | 3.1M records / 239 MB; ts monotonic **per shard**, interleaved across shards | exact output ordering (→ explicit sort) |

The three sample rows (verbatim, unmodified) are already in the page — use them
as the first fixture: `2026-10-30T18:07:18.569411Z GET /api/v2/products 200 8177`
etc.

## 3. Track B — Index

> "Answer 50,000 boolean and phrase queries over a 250 MB text corpus ... the
> queries are readable from the start, and the task is to answer these 50,000."

| Item | Pinned | Note |
|---|---|---|
| Query mix | **62% BOOL, 38% PHRASE** | BOOL: `AND`/`OR`/`NOT`, left-associative, no parentheses; PHRASE `"..."`: adjacent tokens, exact order, **stopwords retained** |
| Query-aware planning | **allowed and expected** | "the queries are readable up front, so you can plan the index around them" — rehearse against the Oct-30 sample queries |
| Corpus | ~256 MB / 50,000 queries | — |
| Organizer hints | "Posting-list compression actually pays"; "Intersection order is most of the win on boolean queries"; "Phrase queries need positions, and positions are where your memory goes" | direct L2/L3 roadmap |

## 4. Track C — Join

> "Parse a hostile multi-megabyte CSV ... join it against a lookup table, **emit
> sorted aggregates**."

| Item | Pinned | Note |
|---|---|---|
| Columns | **`id,name,note,region_id,amount`** (5, fixed, WITH a header row) | visible in the sample bytes |
| Records | 3.32M records / 204 MB | — |
| Output | **sorted aggregates** | sort order = frozen spec |
| Hostile features (all in the sample) | CRLF and bare LF mixed; `""` escaped quotes; commas and newlines inside quotes; trailing space inside quotes; negative amounts (`-5.25`); empty quoted fields; `0.00`; **a U+FEFF BOM at file start AND mid-file at chunk boundaries** | every one → a fixture row |

The five sample records are the oracle's acceptance test — copy them verbatim
as `gen-fixtures` hostile cases before writing the parser.

## 5. Track D — Traverse

> "Queries are drawn at a bounded search distance, so a heavy index has little
> time to pay for itself: **fast parsing, a compact layout and a lean search
> loop decide this track**."

| Item | Pinned | Note |
|---|---|---|
| Graph | 92 MB / 5M edges; header `n m`; edges `u v w` u32, **not sorted by u**; ids dense 0-indexed; directed; **reverse edges listed separately** | parse speed is the L3 story |
| Queries | ~2,000; **local, bounded distance**; "a long tail" incl. unreachable | bidirectional search + early-exit |
| Preprocessing | **CH is legal but "this query mix rarely repays them"** | deprioritize CH/ALT; spend on parsing + layout |

## 6. Scoring & machine (re-confirmed verbatim)

- K = (T_ref/T)^0.50 × (M_ref/M)^0.20 × (S_ref/S)^0.15 × (D_ref/D)^0.15; exponents sum to 1.0.
- T uncapped; **M capped at 50×**; S and D capped at 1.0 (can only cost).
- speed = 50·log10(K) clamped [0,100]; curve table: K 1/2/5/10/20/35/60/100 → 0.0/15.1/34.9/50.0/65.1/77.2/88.9/100.0.
- **S = code lines by `scc` after the formatter, across each track's folder, at the recorded commit** ("which is all the build can see") — per-folder scope confirmed.
- D = 1 + direct non-stdlib runtime deps (one dep ⇒ 2^-0.15 = 0.90 on K).
- Track total = 0.65×speed + 0.35×judge; overall = geometric mean of best 3 of ≥3 tracks.
- 5 runs, discard high+low, mean of middle 3; M from the kernel's peak counter; T from exec to exit.
- Machine: fsn1 (Falkenstein), i5-13500, `--cpuset-cpus=0-11`, 8 GB cap, turbo off, page cache pre-warmed.
- Prizes: $1,800 total ($800/$400/$200 overall, $200 ×2 write-ups, $50 highest-K per track).
- Repos may stay **private until code freeze**; live board via GitHub App; board runs on the full dataset at kickoff and closes at freeze (final scoring on a holdout seed).
- Reference times (measured, 5 runs each): A 32.05 s / B 56.35 s / C 14.44 s / D 72.69 s; timeouts 10×.

## 7. What this de-risks

- The Track A output contract is ~70% pinned before the freeze (grouping, hour
  buckets, percentile list, error class) → spec_intake's A blocking list shrinks
  from 5 to 2 (`sort_order`, `percentile_rule` + formatting details).
- Track C's column set + sorted output + hostile sample are pinned → the
  fixture corpus can be extended NOW with the verbatim sample.
- Track D's organizer hint settles the L3 question (parsing/layout, not CH).
- Every target in this repo's K budgets stays valid (all re-derived from the
  same formula and refs the page states).