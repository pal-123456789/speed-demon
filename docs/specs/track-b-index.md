# TRACK B · INDEX — Design Specification

Status: pre-kickoff design document. No solution code, no function bodies, no implementation language.
Track window: baseline H0–H17 (2h charged to Track B), optimization block H56–H61 (5h charged).
Reference: T 56.35s ONE total wall time (build + queries — the 40/60 blend is gone) · Mref/Sref DEFERRED to Oct 28 frozen spec · timeout 564s.
Corpus: 250MB text. Queries: 50,000 (62% BOOL, 38% PHRASE). Banned: any ready-made search engine.
Scoring reminder: K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15; speed_score = 50·log10(K), clamped [0,100]. The X axis is gone; only M is capped at 50×.

---

## 1. Reference Teardown

**What the reference does (from the frozen brief).** Builds an inverted index over a 250MB corpus, then answers 50,000 boolean and phrase queries, with build and query times measured separately and combined into one T at 40/60 weighting.

**Measured reference axes.** T = 58.7s. Bref, Qref, Mref, Sref, and the exact build/query split are **DEFERRED to Oct 28 frozen spec**. We know Sref = 3180B is Track A's reference source size; Track B's Sref is not published in the brief we hold. We therefore work with the S *ratio* targets directly and treat the absolute source budget as DEFERRED.

**Task semantics that the spec pins (from the brief).**
- BOOL (62%): operators AND/OR/NOT, **left-associative, no parentheses**. Example: `indemnity AND (nothing) NOT surety` — note the literal parentheses in that sample are in a *quoted* phrase position or a malformed sample; the brief states no parentheses are supported, so the tokenizer must decide how to treat a literal `(` in a query. **DEFERRED to Oct 28**: how parentheses are handled (rejected, treated as a token, or a parse error). Design carries a single parse-mode switch.
- PHRASE (38%): quoted `"..."`, adjacent tokens, **exact order**, and **stopwords RETAINED** (so no stopword removal at index or query time — a stopword list would be an error, not an optimization).
- Boolean semantics for NOT (set difference over the whole corpus) are DEFERRED; the standard reading is corpus-minus, and we implement that reading plus a corpus-frequency guard.

**Why the reference takes 58.7s — structural reading (hypothesis, to be measured in the H0–H2 baseline window).**
- Tokenization of 250MB: a general tokenizer (word splitting, case folding, punctuation handling) is typically 15–40MB/s single-threaded → 6–16s.
- Posting-list construction with doc ids and positions, hash-map-per-term: the dominant memory consumer and a large allocator cost.
- 50,000 queries where the *first* operand's posting list is scanned in full, then intersected with subsequent operands in the given left-associative order: with a 250MB corpus the leading operands of common words are enormous, and a naive left-to-right evaluation re-scans those lists 50,000 times. The brief's hint — "intersection order is most of the win on boolean queries" — is exactly this.
- Phrase queries need positions; positions are where the memory goes (the brief says so).

**Reference characteristics we can exploit.**
- Only 50,000 distinct queries exist and they are *published in the input* (queries.txt is part of the run's input). The build phase therefore has complete knowledge of the query set. This is the dominant structural asymmetry of the track and it is legal: it is a runtime decision made from the mounted input, not a precomputed answer baked into the image.
- The corpus is 250MB, so the *entire* index can be built in memory in one pass; there is no need for an on-disk index, and no need to trade build time for query time via a lazy index.
- The brief says posting-list compression "actually pays at this corpus size" — i.e. the corpus has enough repeated terms that gap-encoding beats raw 4-byte doc ids.

**Gate implications for this track.**
- Gate 1 (hash): the output must be exactly right — the set of matching document ids per query, in the pinned order and format. The single highest-leverage unknown is **what the answer actually contains**: full result lists (document ids, possibly in a specific order) or only per-query match counts. That determines whether we must retain doc ids at all. DEFERRED to Oct 28. Our design is parameterized on it (§3).
- Gate 2 (speed floor): a 10× floor against the fastest correct submission; our anchor is 30×.
- Gate 3: 587s timeout. Reference is 58.7s, so even a mediocre result has 10× headroom; a hang is the only real risk.
- Gate 4 (sandbox): no network, read-only dataset, read-only rootfs, /tmp is a 1GB tmpfs. An on-disk index staged in /tmp is doubly bad: tmpfs pages are charged to the 8GB cgroup limit and inflate `memory.peak`, and 250MB–1GB of tmpfs use will wreck the M axis. **In-memory index only, scratchless.**
- Reference drift: because build and query are measured separately, both numbers must be re-derived from the batch-local reference; the 40/60 blend means a 2% drift in the query phase moves T by 1.2% and can flip a borderline comparison.

---

## 2. Axis Budget

**Anchor rows.**

| Row | T ratio | M ratio | S ratio | D ratio | X ratio |
|---|---|---|---|---|---|
| Solid | 30× | 12× | 0.62× | 1.00× | 1.12× |
| Stretch | 50× | 20× | 0.60× | 1.00× | 1.20× |

**Full arithmetic.**

- Solid: T 30× ^0.40 = 3.8981; M 12× ^0.20 = 1.6438; S 0.62× ^0.15 = 0.9308; D 1.00× ^0.15 = 1.0000; X 1.12× ^0.10 = 1.0114.
  K = 3.8981 × 1.6438 × 0.9308 × 1.0000 × 1.0114 = **6.0321**.
  speed = 100·ln(6.0321)/ln(25) = 100 × 1.79742/3.21888 = **55.83**.
- Stretch: T 50× ^0.40 = 4.7818; M 20× ^0.20 = 1.8206; S 0.60× ^0.15 = 0.9262; D 1.00×; X 1.20× ^0.10 = 1.0184.
  K = 4.7818 × 1.8206 × 0.9262 × 1.0000 × 1.0184 = **8.2117**.
  speed = 100·ln(8.2117)/ln(25) = 100 × 2.10597/3.21888 = **65.41**.

**The build/query blend: additive vs harmonic vs geometric.** The brief says the two phases are "measured SEPARATELY and combined into one T, weighted 40% build / 60% query", and that "the build/query split is published in the spec so you can plan the trade". The exact combination formula is **DEFERRED to Oct 28**. We analyze all three plausible readings with a single variable set:
- α = Bref / Tref (the build share of the reference's total time) — DEFERRED; we evaluate α ∈ {0.15, 0.25, 0.35}.
- β = Bref / B (our build speedup), q = Qref / Q (our query speedup).
- **Blend (1) — weighted sum of times (the most likely reading):** T = 0.4·B + 0.6·Q, so the speedup R = Tref/T = 1 / (0.4α/β + 0.6(1−α)/q).
- **Blend (2) — harmonic blend on the normalized times:** T = Tref · (0.4/β + 0.6/q), so R = 1 / (0.4/β + 0.6/q). (This is the reading a scorer gets if it blends the two *relative* times.)
- **Blend (3) — geometric blend:** T = Tref · β^(−0.4) · q^(−0.6), so R = β^0.4 · q^0.6.

**Case analysis with equal build and query speedups (β = q = s).**

| α (build share) | Blend (1) additive | Blend (2) harmonic | Blend (3) geometric |
|---|---|---|---|
| 0.15 | R = s/0.57 = 1.754s | R = s | R = s |
| 0.25 | R = s/0.55 = 1.818s | R = s | R = s |
| 0.35 | R = s/0.53 = 1.887s | R = s | R = s |

**Reading.** Under the additive reading the 40/60 weighting acts as a *leverage multiplier of about 1.8× on the balanced case*: hitting s on both phases yields a blended R of roughly 1.8s. Under the harmonic or geometric readings the balanced case yields R = s exactly. Therefore:

- **The conservative plan targets R = 30× under the most pessimistic reading (blends 2/3), i.e. β = q = 30×.** Under the additive reading the same work buys R ≈ 54.5× (with α = 0.25: 0.4·0.25/30 + 0.6·0.75/30 = 0.01833 → R = 54.5).
- **The blend only matters if we make an unbalanced bet.** Example with α = 0.25: build-heavy (β = 60, q = 20) gives blend (1) R = 39.5, blend (2) R = 36, blend (3) R = 60^0.4·20^0.6 = 5.33 × 6.03 = 32.1. Query-heavy (β = 20, q = 60) gives blend (1) R = 83.3, blend (2) R = 44, blend (3) R = 20^0.4·60^0.6 = 3.31 × 13.0 = 43.0. The spread between readings is up to 2.1× — a 17-point swing in speed_score. **Conclusion: do not bet unbalanced.**
- **Leverage analysis (marginal value of 1% more speed in each phase), α = 0.25, s = 16.5 (the balanced speedup needed for R = 30 under blend 1):** ∂(1/T)/∂β ∝ 0.4α/β² = 0.1/272 = 3.68e-4; ∂(1/T)/∂q ∝ 0.6(1−α)/q² = 0.45/272 = 1.65e-3. **Query speedup carries 4.5× the marginal leverage of build speedup.** Under the geometric blend the leverage ratio is only 0.6/0.4 = 1.5×. Under the harmonic blend, 0.6/0.4 = 1.5× as well (both are symmetric in β and q). So the planning conclusion is blend-robust only if we stay balanced: **spend effort on queries first (up to parity), then on the build.**
- The practical parity target: s = 16.5× on both phases yields R = 30× under blend (1) with α = 0.25 (and R = 16.5 under blends 2/3, i.e. a 30-point shortfall). Since the blend is unknown, we plan for **s = 30 on both phases**, and the Oct 28 spec's published split then tells us whether the anchor rows (30× / 50×) are met under the true blend with slack or need to be re-anchored.

**S axis.** A hand-written inverted index with compression, a query planner, and a phrase engine is *much* more code than a 3,180B reference. Our S targets (0.62 / 0.60) mean our normalized source is 1.61× / 1.67× the reference's. Sref for Track B is not published (DEFERRED). The exchange rate from the K formula: doubling S costs 0.15·ln2 = 0.1039 → K ×0.9013 → −2.1 points; a 1.5× T gain is worth +3.3 points. **Decision: code size is not a constraint on this track; it is the cost of the technique, and it is priced in.**

**D axis.** D = 1 requires std-only, no vendored source. No compression library, no search library (explicitly banned), no `memchr`, no `rayon`. Hand-rolled everything: varint, hashing, thread pool, bitmap ops.

---

## 3. Algorithm & Data Layout

**Stage 1 — read the query set first.** queries.txt is mounted input, available before the corpus is indexed. Parse all 50,000 queries into a compact structure: kind (BOOL/PHRASE), the term/phrase operand list, and the operator sequence. From this we derive:
- the set of distinct terms appearing in any query (call it Q-terms),
- the set of distinct terms appearing in any *phrase* query (P-terms),
- the per-term query occurrence counts, which give us the expected selectivity.
**This is the track's central decision and it must be written into PERF.md with a measured before/after:** postings are built only for Q-terms, and positions are stored only for P-terms. Everything else in the corpus is tokenized (we must still scan every byte) but contributes no postings.

**Stage 2 — tokenize and index the corpus.**
- Tokenization: read the 250MB corpus in large chunks (per-thread buffers, no whole-file residency), split on a byte-class rule, lowercase (ASCII-only fold; case handling is DEFERRED to the Oct 28 spec — the sample queries are lowercase). The token rule must match the reference's exactly (DEFERRED: how punctuation, digits, hyphens, and case are treated). This is a correctness exposure, not a speed one.
- Because the corpus is fixed at 250MB and the query set is known, a **two-pass design** is possible: pass 1 tokenizes and counts term frequencies (no postings); pass 2 emits postings for Q-terms only, and positions for P-terms only. Two passes double the tokenization cost (the dominant build cost). Alternative: one pass that computes per-term doc frequencies and then, at the end, keeps only what we need — but by then the postings are already written, so the memory saving is lost. **Decision: measure both.** If the tokenization cost dominates, prefer one pass with a cheap "is this term interesting?" test (a hash-set probe on the token hash) and write postings only for interesting terms — that gets the memory saving in a single pass, at the cost of a hash probe per token. The memory probe is the right default.
- Postings representation: per term, a list of (doc id, [positions]) with doc ids gap-encoded (varint deltas) and positions gap-encoded per document. The brief explicitly says posting-list compression pays at this size, and it also pays in the M axis.
- **Doc id space:** use the document ordinal (0-based) as the doc id, not a term-frequency-based value; the output format for result lists is DEFERRED, and if the reference emits document *ordinals* we match it; if it emits external document ids, we need a mapping from ordinal to external id, which we keep as a compact array.
- **Positions:** for P-terms, per posting, the list of token positions within the document, delta-encoded. Phrase matching then needs, for a phrase of k terms, the conjunction of k posting lists plus an adjacency test on positions (position in term i+1 equals position in term i plus the gap, where the gap is determined by the token positions of the intervening stopword-free terms — note stopwords are RETAINED, so the phrase adjacency is over the *token stream positions*, not over "content word" positions).

**Stage 3 — query evaluation.**
- BOOL with no parentheses and left-associativity: parse into a flat operator/operand sequence and evaluate with a single accumulator (plus a stack depth of 1 for the deferred NOT). Operators: AND (intersect), OR (union), NOT (complement). The result sets are represented as **sorted integer lists** (doc ids) or as **bitsets over the doc space**; the choice is measured (see the ladder). A bitset over ~50K–500K documents is 6KB–60KB per set — cheap, and AND/OR/NOT are trivially vectorizable. A sorted list is cheaper when the sets are tiny, which is the common case for selective terms.
- **Intersection order is the win.** The brief says so explicitly. For a conjunction, intersect the operand lists in ascending order of their estimated size; for a disjunction, short-circuit as soon as the accumulated union exceeds a threshold, and order the OR operands so that the common (high-frequency) terms are handled once. For a mixed left-associative sequence, we can reorder *within* an AND group without changing the result (intersection is commutative and associative), but a NOT must be applied at its left-associative position — reordering across a NOT changes semantics, so the reorder is only applied inside maximal AND runs and OR runs. This is a subtle, judge-visible design point: it respects the left-associative semantics while still getting the ordering win.
- PHRASE: intersect the posting lists of the phrase's terms (rarest first), then walk the surviving documents' position lists to test adjacency. The classic alternative — intersect the *phrase* postings directly by building phrase-level lists at build time — is too expensive to build for all possible phrases; but since the phrase set is known at build time (38% of 50,000 queries), **we can build phrase-atom postings at build time for the multi-term phrases that actually occur**. This is the second big structural decision: phrase-level posting lists for the known phrases, built during the same corpus pass, turn a query-time multi-list intersection + position walk into a single list scan. Cost: memory for one list per distinct phrase, and the build-time work of detecting the phrase. Measure it; it is likely a large win on the 38% phrase population.

**Data layout summary (per-term):**
- Term dictionary: hash map (our own, open-addressed, deterministic) from term bytes → term id.
- Per term: a posting-list header (offset, count, doc-freq) into a global postings arena; a separate positions arena for P-terms.
- Postings arena: one contiguous `Vec<u8>` per... no — one global arena with per-term regions, so that a query's list scan is a single contiguous read (cache-friendly). Build order: postings are appended per term as documents are processed, so each term's list is scattered across the arena; to make each list contiguous, do a final "gather" pass: compute per-term total sizes, prefix-sum, and copy. That copy is O(postings) and is the classic two-pass index build. Alternative: per-term `Vec`s (fragmented memory, allocator churn) — rejected. Alternative: linked chunk lists (chunks of e.g. 64KB) that keep lists contiguous in chunks and avoid the gather pass — attractive when the gather pass is expensive. Decide by measurement.
- **Concurrency:** the corpus scan is embarrassingly parallel by document ranges; each worker produces partial postings per term. Merging partial postings is the classic expensive step. **Design choice: shard the postings arena by term id, not by worker** — i.e. each term's postings are written by a single designated worker, which requires knowing which worker owns which term. Practical approach: partition the corpus into 12 contiguous ranges; worker w handles range w; postings for a term may be produced by many workers → they must be merged. Merging = concatenate per-worker segments per term and re-encode the gaps. Alternative that avoids merging entirely: **two-phase build** — phase 1 (parallel) tokenize and produce per-worker (term, doc, positions) triples; phase 2 (parallel by term) group by term and encode. Phase 2's group-by requires a radix partition over (term, worker) which is exactly the counting-sort trick: compute per-worker term counts, prefix-sum per (term, worker) pair, scatter. That is a single O(n) scatter with a large destination footprint (postings arena) — memory-heavy but the arena is exactly the M axis.
  **The M axis forces the choice.** A fully parallel build that holds all partial postings plus the final arena peaks at ~2× the final index size. Since M has weight 0.20 and the target is a 12× reduction vs the reference, we need the *peak* small, not just the final size. **Decision: build the index single-writer per term with the scatter pass, and free the partials incrementally.** Concretely: parallel tokenize into per-worker partial arenas (each worker owns its own memory, no sharing), then a parallel-by-term phase reads the partials and writes the final arena, releasing each partial as it is consumed. Peak ≈ final arena + largest partial. If the final arena is 60MB and partials are 60MB, peak is ~120MB — acceptable against an 8GB cap and against an Mref of DEFERRED size.
  **The alternative we will also try: restrict postings to Q-terms so hard that the whole index fits comfortably, and accept a slower build in exchange for a much smaller M.** With Q-term filtering the postings arena may be only 20–30% of the full-corpus index.

**Memory budget (target shape; absolute Mref DEFERRED).** With ~50M tokens in a 250MB corpus (5 bytes/token average) and Q-term postings at an assumed 30% coverage:
- postings (doc ids, varint-gap): ~15M postings × ~2B = 30MB
- positions for P-terms (assume 20% of tokens): ~10M positions × ~1.5B = 15MB
- phrase-atom postings: assume +5MB
- doc-id mapping array and term dictionary: ~5MB
- corpus read buffers: 12 × 4MB = 48MB
- **Total ≈ 100MB.** If Mref is in the 1.2GB range, 12× is 100MB — this budget lands exactly on the Solid row. If Mref is smaller (e.g. 600MB), 12× is 50MB and this budget misses it; the lever is then the corpus buffer (48MB of the 100MB) and the Q-term filter aggressiveness. **DEFERRED: Mref.** We will measure our absolute peak and convert to a ratio on Oct 28.

---

## 4. Hash-Gate Correctness & Determinism

**Determinism rule.** The answer for query i must be identical across runs, worker counts, and phases. The index build is a deterministic function of (corpus bytes, query set bytes).

| Hazard | Elimination |
|---|---|
| Hash-map iteration order leaking into output | All output is driven by the query list order and by explicit sorts on doc ids; no index structure is iterated to produce output |
| Parallel build producing different posting orders | Postings are ordered by doc id by construction (doc ids ascend as the corpus is scanned in range order); the gather pass preserves per-term doc-id order |
| Word/tokenization ambiguity | The token rule is a single, pinned function; every token it emits is a unit test with expected output; the 10% sample hash is the arbiter |
| Case folding and Unicode | ASCII-only case folding; no Unicode normalization unless the Oct 28 spec demands it. DEFERRED |
| Boolean evaluation order affecting NOT | NOT is applied at its left-associative position; reordering is only inside maximal AND/OR runs, which is semantics-preserving |
| Phrase adjacency off-by-one (stopwords retained) | Positions are token-stream positions, not content-word positions; unit tests with a stopword inside the phrase |
| Set representation differences (bitset vs list) | Both must yield the same doc-id set; the output always sorts doc ids ascending; a cross-check compares the two representations on a sample of queries |
| Result-list output order (DEFERRED) | One module renders results; candidate orderings (ascending doc id, corpus order, relevance order) are selectable, and the sample hash selects one |
| Query parse of `(` and of empty queries | A single documented policy switch; a malformed query produces a pinned empty result rather than a crash |
| 32-bit vs 64-bit doc ids | Doc ordinals are 32-bit (corpus 250MB ⇒ ordinals fit in 32 bits with room); distances/ids are widened deterministically where the output format requires |

**Verification ladder.**
- **V1 (baseline, H0–H2):** reproduce the published 10% sample hash with the baseline implementation. Hard gate before any optimization.
- **V2 (two-implementation oracle):** a deliberately simple "reference-style" implementation — tokenize, build plain postings with doc ids and positions, evaluate queries with straightforward set operations in the given left-associative order — is written during the baseline window and kept as the oracle. Any optimized path must agree with it on *every* query's result set, query by query, not merely on the aggregate hash. This is the only way to catch a systematic error that still happens to hash-match on the sample.
- **V3 (index-representation invariance):** the same query answers must be produced by the bitset path and by the sorted-list path, on a sample of at least 1,000 queries including the 10 largest results and the 10 smallest. Prevents a representation-specific bug from reaching the gate.
- **V4 (build-order invariance):** the index built with 1, 6, and 12 workers must produce byte-identical output. Any mismatch is a stop-the-line event.
- **V5 (repeat-run invariance):** 5 runs, identical hashes, spread under the 5% re-run threshold.
- **V6 (query-set sensitivity):** the parser must be tested against adversarial query strings: empty query, only an operator, trailing operator, repeated operators, a phrase with one term, a phrase with a stopword at each end, a term absent from the corpus, a term with corpus-frequency 1, and a query whose result is the entire corpus. Each must produce a pinned, non-crashing result.

**Gate 4 compliance.** No network. The corpus mount is read-only. The index lives in process memory; no files are written to the image, and **no index is staged in /tmp** (tmpfs is charged against the 8GB cgroup limit and against `memory.peak`, which would destroy the M axis). The only file written is the output to /out. Nothing is precomputed into the image: the index is built at run time from the mounted corpus, which is exactly what the task asks for, and which is why build time is inside the timed run.

**Gate 3 compliance.** The reference is 58.7s against a 587s kill. A hang would come from a cyclic loop in the query evaluator or from an unbounded scan. Every evaluator loop advances a cursor monotonically; every list intersection terminates because both cursors advance.

---

## 5. Optimization Ladder

| # | Rung | Hypothesis / expected effect | Risk | Acceptance |
|---|---|---|---|---|
| L0 | Baseline: simple tokenizer, plain postings (doc id + positions), set operations in the given order | Oracle + V1 hash; T ≈ reference-class | Low | Sample hash matches |
| L1 | Q-term filtering: build postings only for terms that appear in the 50,000 queries; positions only for phrase terms | The single largest M win (potentially 12× on the M axis) and a large build-time win; expected M ratio 3–6× and T +15–25% | Medium (must not drop a term the reference keeps) | Oracle agreement on every query; M measured |
| L2 | Postings compression: varint gap-encoded doc ids and positions | The brief says compression pays at this corpus size; expected M +20–40% and better cache behavior in query scanning | Medium (encoding bugs) | Oracle agreement; M and T both measured |
| L3 | Phrase-atom postings built at index time for the known phrases | Turns each phrase query from a multi-list intersection + position walk into one list scan; expected 2–4× on the 38% phrase population | Medium (build cost) | Oracle agreement; query-phase time measured separately |
| L4 | Intersection reordering: sort AND operands by posting-list length ascending, inside maximal AND runs only | The brief's stated "most of the win"; expected 2–10× on boolean queries | Medium (semantics risk near NOT) | Oracle agreement on all 50,000 queries; query time measured |
| L5 | Bitset set representation for large results, sorted lists for small ones (hybrid) | Removes per-element allocation in AND/OR; expected 1.3–2× on the query phase | Medium | V3 representation cross-check |
| L6 | Short-circuit evaluation: stop an OR once the result is provably "large enough" under the pinned output semantics; defer the complement for NOT until last | Cheapest remaining win in the query phase | Medium (semantics risk) | Oracle agreement; must be provably equivalent, never approximate |
| L7 | Parallel corpus scan: 12 threads, per-worker partial postings, gather phase | Expected 2.5–4× on the build phase, which is 40% of T | Medium (merge order) | V4 build-order invariance; build time measured separately |
| L8 | Parallel query evaluation: 50,000 queries are independent; distribute across 12 workers, write results to a per-query slot | Expected 2–3× on the query phase (60% of T) | Low (results are per-query slots) | V4; per-query results identical |
| L9 | Tokenizer hardening: byte-class table, in-place lowercase, no allocation per token | Build phase; expected 1.2–1.6× on tokenization | Low | Oracle agreement |
| L10 | Hot-loop work reduction: avoid materializing the term string per token (hash the bytes directly and compare hashes) | Removes per-token allocation and string construction | Medium (hash collisions must be resolved by content compare, never ignored) | Oracle agreement; collision test on a synthetic corpus |
| L11 | Postings arena gather (two-pass grouping) vs per-term chunked lists | Decides the peak-memory shape | Low | Measure both; keep the lower peak |

**Explicitly rejected rungs (for PERF.md).**
- A general-purpose search library: banned by the brief, and it would also set D far above 1.
- Building a full positional index for every term: enormous memory for zero query benefit (we only ever query 50,000 known queries).
- A query-time phrase engine with a generic position intersection, once L3 shows the prebuilt phrase atoms are faster.
- "Optimizing" the intersection order *across* a NOT: it changes semantics. Rejected on correctness grounds, not on speed grounds.
- Storing the corpus text to answer phrase queries: memory blowup with no benefit once L3 exists.

**PERF.md content plan.** A table with one line per rung: build time, query time, blended T, memory.peak, index size, and kept/reverted. Build and query are reported separately because the scoring blends them and because a reader can see the trade. The failed experiments above are logged with numbers.

---

## 6. X-Scaling Safeguard

**What X measures.** X = (runtime at 10× input) ÷ (runtime at 1×) versus the reference's own ratio. Weight 0.10; the intent is to not lose, not to win. Targets: 1.12× (Solid) and 1.20× (Stretch) — deliberately looser than Track A because an index is inherently a structure that grows with the corpus, and because the reference's own X ratio is likely favorable to us (a naive reference that stores positions for every term will scale worse than 10× in memory, and if it OOMs at 10× its ratio is much worse than ours).

**Where superlinearity enters this design.**
1. *Index size.* The index is O(corpus). At 10× input the postings arena is 10× (≈1GB at our budget) plus positions 10× (≈150MB) plus buffers. Peak ≈ 1.2GB — inside the 8GB cap but no longer trivially so. The M axis at 1× is unaffected; X measures time, but an OOM at 10× would be a failed probe.
2. *Query cost at fixed query set.* The 50,000 queries are fixed, but the corpus behind them is 10× larger: posting lists are 10× longer, so each query's intersection cost grows by more than 10× if the lists are not proportionally *sparser* — and they are not. **This is the dominant X hazard in this track**: query time can grow 10–20× while the corpus grows 10×. Mitigations: (a) the Q-term filter does not change with corpus size, so the *number* of terms queried is constant while the *lengths* grow; (b) bitset operations cost O(doc count / 64) — also 10×; (c) the rarest-operand-first ordering means the intersection cost is driven by the rarest list, which grows 10× → query time grows ≈10×. Expect X_local ≈ 1.05–1.20.
3. *Doc-space bitsets.* If the doc space grows 10×, a bitset per term grows 10×. If we keep bitsets for many terms simultaneously, memory explodes. Mitigation: bitsets only for the small working set of a single query (allocated and released per query, or a per-thread pool of a bounded number of bitsets).
4. *Corpus buffers.* Fixed-size, so linear.

**Safeguards.**
- **S1.** The index is rebuilt from scratch at 10×; we never rely on a structure whose size is fixed at 1×.
- **S2.** Bound the per-query working set: a small fixed pool of scratch buffers per worker (e.g. 4 bitsets of 64KB each), reused, so query evaluation's memory is constant, not proportional to the corpus.
- **S3.** Keep the doc-space representation scale-free: 32-bit doc ordinals; bitset words 64-bit; no per-doc arrays that are sized from a compile-time constant (a fixed-size array is exactly the thing that makes a 10× probe behave superlinearly).
- **S4.** Prefer sorted lists over bitsets when a result is small (a bitset costs O(doc count) regardless of how few docs matched; a list costs O(matches)). Choose per query by an estimated-size threshold. This also keeps the *time* growth closer to linear in the number of matches rather than the number of documents.
- **S5.** Probe construction is DEFERRED (the organizers' 10× input is unpublished). We build our own 10× probe by concatenating the corpus ten times into a dev scratch file on host scratch (never /tmp — a 2.5GB tmpfs staging would blow the tmpfs limit and charge the cgroup). The probe must exercise the *same* code path with no size special-casing. Note the semantic effect of concatenation: the same doc content repeats, so posting lists are 10× longer and doc ordinals 10× larger — a faithful stress of the time-scaling, though not of vocabulary growth.
- **S6.** Acceptance: X_local ≤ 1.20 and 10× peak ≤ 4GB. If X_local > 1.25, the ablation is: (i) build phase only, (ii) query phase only, (iii) postings-scan only, (iv) bitset-vs-list — identified within one hour.
- **S7.** Never trade a verified T win for an X fix: 0.10 weight means a 0.10·ln(1.2) = 0.018 penalty on ln K (K ×0.982, ≈ −0.4 points) versus the 3+ points a 20% T gain returns. This track's X risk is real but small; the design's job is to keep X_local ≤ 1.20 and move on.

---

## 7. Hour Budget

Total charged to Track B: **7h = 2h baseline + 5h optimization**. The baseline deliverable is written inside the H0–H17 window; the optimization block is H56–H61, late in the campaign — by then the corpus and query set are frozen and the team can measure against a stable reference.

**Baseline block (2h, charged), H0–H2 nominal.**

| Hour | Activity | Exit criterion |
|---|---|---|
| H0–H0.5 | Read the Oct 28 frozen spec: tokenization rules, stopword policy, query grammar, result format (doc ids vs counts), the published build/query split | Format + grammar sheet; unknowns flagged DEFERRED |
| H0.5–H1.5 | Baseline implementation: tokenizer, postings with doc ids and positions, left-associative set evaluation, phrase matching by position adjacency | Runs end to end on the 10% sample |
| H1.5–H2 | V1: sample hash matches; measure build and query separately, 5 runs, discard extremes; measure memory.peak; confirm the oracle is usable | Sample hash reproduced; baseline build/query split recorded |

**Optimization block (5h, charged, H56–H61).**

| Hour | Activity | Exit criterion |
|---|---|---|
| H56 | L1 (Q-term filtering) + L2 (postings compression) | Oracle agreement on all queries; M measured; build time measured separately |
| H57 | L4 (intersection reordering) + L3 (phrase-atom postings) | Query-phase time measured separately before/after; oracle agreement |
| H58 | L7 (parallel build) + L8 (parallel queries) | Build-order invariance (V4) passes; blended T re-measured |
| H59 | L5 (hybrid set representation) + L6 (short-circuit) + L9/L10 (tokenizer and hashing hardening) | Each rung measured independently; keep only ≥3% gains |
| H60 | **Verification block** (V2 oracle diff over all 50,000 queries, V3 representation cross-check, V5 repeat runs) + L11 (peak-memory shape) | All green; this hour is not traded for speed |
| H61 | Freeze: re-anchor the axis table to the batch-local reference under the *published* blend formula; recompute K; write PERF.md; measure normalized source bytes against Sref | Submission frozen; axis table matches the true blend |

**Budget hygiene.**
- The late placement (H56) is an advantage: the team has 3 days of other-track learning to apply, and the corpus/query files are stable. It is also a risk: a discovery late in the campaign has no slack. Therefore the *baseline* must be complete and correct by H2 — the 5h of optimization is a bonus on top of a working, verified submission, never a rescue of an unfinished one.
- Verification gets a full hour (H60). Track C's warning that most disqualifications happen in the correctness track applies here: a fast wrong answer scores zero, and a correct slow one scores 45+.
- If the blend turns out to be the pessimistic harmonic one (blend 2) and the build share α is large (0.35), the stretch row is unreachable; we then re-anchor to the Solid row and spend the remaining hour on PERF.md and on the M axis (the 8% judges' share and the 0.20-weight M axis are both still available).

---

## 8. Risk Register & Stop Rule

| ID | Risk | Likelihood | Impact | Detection | Mitigation | Stop rule / trigger |
|---|---|---|---|---|---|
| R-B1 | Result format unknown (doc id lists vs counts; ordering; external doc ids) — DEFERRED to Oct 28 | High | **Total** (Gate 1) | V1 sample hash | One render module with selectable orderings; the sample hash selects the right one | If unresolved at H56, emit several candidate renderings and keep only the hash-matching one |
| R-B2 | Tokenization rule mismatch (case, punctuation, digits, hyphens) | High | **Total** | V2 oracle diff on the sample; token unit tests | Pin the rule from the Oct 28 spec; ASCII-only folding; a table of tricky tokens | Any disagreement → freeze optimization, fix the tokenizer, re-verify |
| R-B3 | Grammar mismatch (parentheses in the sample, empty queries, dangling operators, NOT semantics) | Medium | **Total** | V6 adversarial query set | One documented policy per case, all in the spec sheet | If the grammar is still ambiguous at H56, choose the conservative reading and document the risk in PERF.md |
| R-B4 | The published build/query split α and the blend formula differ from our planning assumption, invalidating the anchor rows | Medium | Medium (−5 to −15 points) | Read the Oct 28 spec | We plan a blend-robust strategy (β = q = 30×); we never bet unbalanced | At H56, re-anchor the table to the published formula; if the Solid row is unreachable, shift hours to M and PERF.md |
| R-B5 | Memory blowup: the M axis collapses to ~1× | Medium | Medium (−2 to −10 points) | memory.peak measured at H1 and H56 | Q-term filtering, positions only for phrase terms, phrase atoms, no whole-corpus residency, no /tmp staging | If memory.peak exceeds 25% of the 8GB cap, cut the index scope (drop phrase atoms, then drop positions for single-term phrases) before any speed work |
| R-B6 | Build-time explosion from the gather/merge phase, making the 40%-weighted build the bottleneck | Medium | Medium | Build time measured separately, every rung | Per-term chunked lists to avoid a gather pass; scope reduction; parallel scatter | If the build phase exceeds 50% of the blended T, stop query-side optimization and fix the build |
| R-B7 | Phrase-atom build cost exceeds the query savings | Medium | Low–Medium | Separate build/query measurements | Measure L3 in isolation; keep it only if the blended T improves ≥3% | Revert L3 if it is neutral or negative |
| R-B8 | Reordering intersection changes boolean semantics (crossing a NOT) | Low | **Total** | Oracle agreement over all 50,000 queries | Reorder only within maximal AND/OR runs; keep a "no-reorder" mode and diff the two modes over all queries | Any difference → disable reordering permanently |
| R-B9 | Nondeterminism from parallel build or parallel query evaluation | Low | **Total** | V4 | Per-query result slots; deterministic ordering; integer-only set ops | Any mismatch → drop to single-threaded query evaluation and re-verify |
| R-B10 | The 10× probe OOMs or is 2.5GB of dev scratch we cannot place | Medium | Low | Probe construction at H59 | Host scratch, never /tmp; staged partial probes (2×, 4×) if 10× is impractical | If a 10× probe is impossible, reason from the size invariants and document it in PERF.md |
| R-B11 | Source size overshoot (S ratio below 0.60) | Medium | Low (−2 points) | Normalized byte count per rung | Budget: ≤1.7× the reference's normalized size; cut L3/L10 features first | If over budget, cut features until within 1.7× |
| R-B12 | Reference drift voids the batch and our ratios are fiction | Medium | Medium | Batch-local reference timings | Re-derive from the batch-local reference; re-measure after drift | Discard and re-measure; do not re-tune to a drifted reference |

**Global stop rule for Track B (in priority order).**
1. **Correctness stop:** at H60, if the oracle diff is non-empty for any query, the optimization ends and the last green state ships. Zero is worse than 45.
2. **Representation stop:** if the bitset and list paths disagree on any query, the bitset path is disabled.
3. **Unbalanced-bet prohibition:** we do not ship a build-heavy or query-heavy strategy without knowing the published blend and α. Until then, β = q parity is the target.
4. **Diminishing-returns stop:** two consecutive rungs below 3% ⇒ stop the ladder, finish verification and PERF.md.
5. **Memory stop:** if memory.peak exceeds 2GB at 1×, the index scope is cut before any further speed work.
6. **Freeze:** at H61 the last green, verified, deterministic build ships with the re-anchored axis table and the PERF.md log.

**Definition of done for Track B.** The published 10% sample hash is reproduced; the full-dataset output hash matches on 5 of 5 runs; the optimized results agree with the baseline oracle on all 50,000 queries; build and query times are measured separately and blended under the published formula; memory.peak is recorded; the 10× probe reports X_local ≤ 1.20; PERF.md contains the per-rung build/query/memory table and the failed experiments.
