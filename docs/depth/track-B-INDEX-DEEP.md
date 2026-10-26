# TRACK B · INDEX — INSANE-LEVEL DEEP DIVE

> **MODEL-DERIVED UPGRADES (external model, see `97-MODEL-OUTPUT-DIGEST.md` §4):**
> (1) **PHRASE QUERIES NEED NO POSITIONS** — an Aho–Corasick automaton over
> term ids runs in the tokenize pass; each phrase becomes a pseudo-term with its
> own posting list (PHRASE query = single list lookup; position storage drops
> to zero). (2) **Tiered postings** — tiny (<128, u32 array), mid (128-posting
> blocks + block_max + u16 offsets), dense (D-bit bitmap); blocks beat
> delta-varint (varint's skip table is just a worse block layout). (3) **The
> left-associative correctness pitfall:** `A AND B OR C AND D` =
> `(((A∩B)∪C)∩D)` — AND-over-OR precedence is a SHA-zero; evaluate as a left
> fold with distribute (A∪B)∩C = (A∩C)∪(B∩C). (4) Only QUERY terms get
> postings (term ids by first appearance in the query file). (5) Memory budget
> ≈ 100–150 MB; wave-of-24-slab build; chunks of 64 queries, heavy-first,
> slot-ordered emission. Model's T estimate ≈ 0.3–0.5 s vs the 56.35 s ref.

> Pre-kickoff design. No solution code. Companion to `docs/specs/track-b-index.md`.
> **Engine:** inverted-index full-text search (boolean + phrase) → Elasticsearch-lite.
> **Ladder goal:** L3. **Contract anchors:** ref **56.35 s ONE total wall time**
> (build + all queries; the 40/60 blend is gone), timeout 564 s; corpus ~256 MB
> text, **50,000 queries (62% BOOL, 38% PHRASE "…")** — **the queries are
> READABLE UP FRONT**, so plan the index around them (events page);
> stopwords **retained**; queries keep left-associative, no parentheses;
> banned: any ready-made search engine. **Absolutes:** Commit T ≈ **1.88 s**
> (30×), Stretch ≈ 1.25 s (45×). Organizer hints = the L2/L3 roadmap:
> "posting-list compression actually pays", "intersection order is most of
> the win on boolean queries", "phrase queries need positions, and positions
> are where your memory goes".

---

## 1. The complete feature inventory

### P0 — correctness-critical

| # | Feature | Why it exists / the trap | Oct-28 pin needed |
|---|---|---|---|
| B01 | **Token definition** (byte-level split set: which bytes separate, are tokens lowercased, is anything collapsed) | Tokenization defines both the index and every query — a mismatch between them is a silent wrong-answer bomb | tokenizer rule |
| B02 | **Stopwords RETAINED — must be indexed** | The page says stopwords are kept; dropping them makes phrase queries "a the an" fail | — |
| B03 | **Boolean semantics: left-associative `AND`/`OR`/`NOT`, no parentheses** | The parser must NOT implement precedence beyond the pinned binding; NOT with no following term = defined spelling? | NOT precedence + NOT-alone spelling |
| B04 | **Phrase matching on the SAME token stream as the index** | Phrase = consecutive token positions of the query terms, in order, adjacent | — |
| B05 | **Result format per query** (one line per query? ids space-separated? cap on count? ordering of ids) | The output shape is the SHA contract | result format + id cap |
| B06 | **Deterministic posting order and merge order** | Doc ids must appear in a fixed order (index insertion order), never hash order | — |
| B07 | **Term absent from the corpus** (query mentions a token with no postings) | Boolean AND → empty; OR → other terms still answer; PHRASE → empty; the spelled output for "no results" | the empty-result spelling |
| B08 | **Queries answered in input order** | The 50 k lines of output map 1:1 to the 50 k query lines, in file order | — |

### P1 — the Commit row

| # | Feature | Why | Notes |
|---|---|---|---|
| B11 | **Single-pass corpus scan**: tokenize → per-term postings (doc id) + per-term positions | Re-reading 250 MB costs seconds; one pass builds both | streaming tokenizer, no corpus retention |
| B12 | **Doc-id encoding: u32 delta lists, chunked** | Postings shrink ~4×; decode is a running sum — cache friendly | varint or fixed-u32 chunks |
| B13 | **Term dictionary: sorted unique terms → (posting offset, count)** | Binary search per query term; the dictionary is small (~100 k terms?) | — |
| B14 | **Posting intersection: pick the shortest list as the lead, galloping advance on the rest** | AND queries dominate BOOL; list-length-aware evaluation is the biggest query win | — |
| B15 | **Phrase evaluation via positional intersection** (for each candidate doc from the lead list, verify positions with a small sliding window) | 38% of queries are phrase; naive cross-product is catastrophic on "the a an" | positions stored per term |
| B16 | **Parallel query evaluation across the 50 k queries (12 threads)** | Queries are independent; output is emitted in-order with a per-thread queue → deterministic | — |
| B17 | **Position lists only for terms that matter** (store u32 deltas; drop positions of single-token vocabulary?) | Memory discipline; check spec's phrase rule first | — |
| B18 | **No corpus retention**: after the index build, the text is released | 250 MB in RSS is 3× our M target | — |
| B19 | **Query-aware index planning** — the 50 k queries are readable from the start (events page); rehearse the index shape (dictionary choice, which terms get positions) against the Oct-30 sample query file | Turns a guess into a design decision before H0 work is wasted | sample queries at Oct 30 06:00 |

### P2 — the insane toppings

| # | Feature | Why | Risk |
|---|---|---|---|
| B21 | **SIMD tokenizer hot loop** (lane-wise delimiter testing on 16–32 B, then scalar word-buffer handling) | Tokenize 250 MB in < 0.3 s | keep the scalar oracle for the first SHA |
| B22 | **Skip-list / tiered posting blocks** (per 64-doc block: first id + block bitmask for AND) | Intersection skips whole blocks | complexity vs L14 galloping |
| B23 | **Term-id caching in the query parser** (interning the 50 k queries' tokens once) | Removes repeated dictionary hits for hot terms | hot-term cache, bounded |
| B24 | **Reorder posting blocks by term at BUILD time** (dual: build in streaming order, then a final compact pass that also computes block maxima) | Turns AND into a parallel block merge | one extra pass over postings (~cheap) |
| B25 | **Phrase prefilter**: if all phrase terms are single-token (positions == 1), phrase = AND | Cheap common case | correctness tied to B04 |

## 2. The ladder

```
L0  reference-equivalent (python, ~56 s)        H0–H1   — the SHA oracle
L1  correct single-thread Rust (B01–B08)        H1–H4   — target ~6–8 s, SHA-green
L2  encode + shortest-lead eval + parallel      H4–H12  — target ~1.9 s
L3  insane (B21–B25)                            H12+    — target ~1.2 s; stop when <0.3 pts/hour
```

## 3. Determinism traps (specific to B)

- **Hash-ordered postings** — term → postings must be collected in doc order and emitted in doc order; a `HashMap<Term, Vec<u32>>` merged by arbitrary iteration breaks the SHA. Use a term-ordered build (schema: stream → sort-by-term buckets → stable) or an id-ordered dictionary.
- **Parallel query buffers** — each thread's answers must be written to the query's own slot (query index), never reordered.
- **Interning order** — if two terms first appear in different shards, the shared id table must be built deterministically (by term bytes), not by first-seen.
- **Stopword case** — "The" vs "the": everything hinges on B01; decide once, apply to both index and query, test the identical tokenizer on both sides.
- **Tie in "no results"** — the SAME spelling for a term absent vs a phrase with no matches (B07).

## 4. Memory budget (M target well under 1 GB; cap 8 GB)

| Structure | Estimate (250 MB corpus) |
|---|---|
| Postings (u32 doc ids, delta-chunked) | ~40–80 MB |
| Positions (u32 deltas, only where phrases need them) | ~60–140 MB (or less with B17) |
| Term dictionary | ~5–15 MB |
| Query buffers (12 × 4 k lines × 2) | ~50 MB transient |
| Corpus text | **released after build** — never retained |

The 8 GB cap is not a constraint; the discipline is: never retain the corpus,
never copy a posting list for a merge (operate on slices with cursors).

## 5. Parallelism plan

- **Build:** 12 threads each tokenize a 1 MiB shard → per-shard term buckets →
  deterministic merge by term bytes → final posting/position pass.
- **Queries:** 50 k queries → static partition (balanced by token count); each
  thread evaluates independently against the read-only index; results staged per
  query id and emitted in input order. The index is immutable during querying →
  zero locks, zero atomics in the hot path.
- Phrase evaluation uses per-thread position windows; no shared scratch.

## 6. Edge-case matrix

| Input | Decision | Fallback |
|---|---|---|
| Empty corpus / zero tokens | all queries empty → spelled empty result | B07 spelling |
| Query line empty | skipped? empty line? | follow spec; test both |
| Token at EOF without separator | counts as a token | — |
| 1-byte tokens ("a", "I") | indexed like any token (stopwords retained) | — |
| Duplicate terms in one query ("cat AND cat") | idempotent on postings | — |
| NOT with no postings anywhere | NOT x = all docs minus none = all docs — cap explosion | follow B03 pin |
| Ultra-common term in PHRASE ("the big the") | positional window must handle repeats | — |
| CRLF corpus lines | tokenizer must not leak `\r` into tokens | — |

## 7. Validation plan

- **Tokenizer parity test:** every token in the corpus's dictionary is produced
  identically by the query-time tokenizer (loop: tokenize query, tokenize a
  synthetic doc with the same bytes, assert same set).
- `difftest` vs L0 on: the sample corpus + sample queries, hostile fixtures
  (1-byte tokens, CRLF-only, giant single line, all-stopword queries).
- Query-order determinism test: run with 1, 6, 12 threads → byte-identical output.
- The SHA gate pins `index.txt`; re-pin per rung.

## 8. H0 window runbook (first 6 hours)

1. **H0–H1:** freeze B01 (tokenizer) and B03/B05 pins from the spec; write the
   tokenizer parity test first — it's the whole track's correctness spine.
2. **H1–H4:** L1 SHA-green single-thread. Re-pin `index.txt.sha256`.
3. **H4–H12:** L2 — encoded postings, shortest-lead AND, phrase windows,
   parallel queries. Bench every rung vs same-session ref.
4. **H12+:** L3 (SIMD tokenize, block skips) while SHA stays green and the hour
   buys ≥ 0.3 points.
5. **Freeze-day:** determinism ×3 (1/6/12 threads), final SHA, PERF.md ladder.

## 9. Open questions → Oct 28

- Tokenizer rule (B01) — lowercasing? which separators? length floor?
- NOT binding and NOT-alone spelling (B03), result line format + id cap (B05),
  empty-result spelling (B07).