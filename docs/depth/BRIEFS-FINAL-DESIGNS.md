# FINAL ROUTINE DESIGNS — the P-A answer, verified and adopted

> Hand-verified against the rank formula, the spec worked examples and the
> oracle before adoption. This supersedes the prose in
> `docs/depth/BRIEFS-CRITICAL-MODULES.md` for these four routines; the
> corrections section there still applies.
>
> **Verification done by hand on the returned vectors** (all matched):
> * A: rank indices for n = 17, 18, 19 (i95 == i99 == n-1, so the `else` branch
>   is taken and nothing is out of range); n = 33 → 17/32/33, n = 64 → 32/61/64,
>   n = 100 → 50/95/99, n = 200 → 100/190/198; `v[i] = i/2` at n = 17 → 4/8/8;
>   the outlier series `[1..18, MAX]`, `[1..19, MAX]`, `[1..98, MAX]`,
>   `[1..99, MAX]` all land on the stated ranks.
> * C: the worked example's totals are internally consistent with the spec's own
>   rules (AA: id_sum 3, 12.34 + (−0.05) = 12.29, 2 distinct names of 11 and 12
>   decoded bytes = 23, notes 5 + 5 = 10; BB: ids 3+4+5+6 = 18, 100.00 + 0 + 0 +
>   1.50 = 101.50, three distinct names including the empty one counted once,
>   `-0.00` → 0.00; `_UNMATCHED` for region 99).
> * D: 2 × (2^32−1) = 8589934590; 2047 × (2^32−1) = 8791798052865;
>   2048 × (2^32−1) = 8796093020160; and the packing boundary is exactly right —
>   n = 2048 × (2^32−1) < 2^43 (packed heap) while n = 2049 crosses it (wide
>   heap). A u32 wrap would have produced 4294967294.

## The four designs, adopted

1. **A · `pct`** — ranks `i_p = (p*n + 99)/100 − 1`; `n <= CUT` full
   `sort_unstable`; above it `select_nth_unstable(i99)` then narrow to `v[..i99]`
   **inclusively**, taking `p95 = p99` when `i95 == i99` and `p50 = p95` when
   `i50 == i95`. Special-case `n = 1` (`(v0,v0,v0)`) and `n = 2`
   (`(min,max,max)`). **CUT starts at 32 and is tuned on the real group-size
   histogram** — it cannot change output, because both paths return
   `sorted[k−1]`. Property test: n = 1..200 × {random×5 seeds, ascending,
   descending, all-equal, `i/2`, one outlier at first/middle/last} against
   `sorted[(p*n+99)/100 − 1]` computed by the Python reference.

2. **B · `phrase`** — doc-level driver walks the **rarest term's** doc list with a
   monotone `gallop_ge` cursor per term and exits early when any cursor runs off
   the end; inside a doc, positions are **document-relative** and matched with
   **one cursor per phrase slot** (`cp[s]`), anchored on the shortest position
   list, testing `target = start + slot`. First match in a doc emits once and
   returns. `k == 1` emits the doc list; a term occurring nowhere, or `k == 0`,
   emits nothing. Query text goes through the **same byte tokeniser**, so
   `"visit Zürich"` is 3 tokens and `"Zürich"` is 2. Documented traps:
   `go go` on `go go go` counts once; `x x x` needs three consecutive; `quick
   brown` must NOT match across docs 3→4 but `brown fox` MUST match across the
   period in doc 5; `x y x` anchors on `y`.
   Property test: random posting lists vs a brute-force `(doc, start)` matcher,
   forcing both the gallop and the linear-merge paths.

3. **C · record walker** — parity gives the quote state; **record starts** are
   `s == 0`, or `buf[s−1]` is an unquoted terminator (`\n`, or `\r` not followed
   by `\n`). Each chunk is parsed in parallel from its `sync(c)` start; the driver
   consumes the parallel result **only when `cur == R[c]`**, otherwise it walks
   that chunk serially — so a parity assumption that ever breaks degrades to a
   correct serial parse, and the output never depends on the chunk size or the
   thread count. Fields are `(start, len, quoted, escaped)` with the outer quotes
   excluded; BOM is stripped only at a record start; a CR inside quotes is data.
   **Distinctness and byte counts use the decoded content** (each `""` → `"`).
   Sweep test: the worked example plus every vector at **CH ∈ 1..80** must equal
   CH = ∞ — that puts a chunk edge on every BOM, CRLF and `""` byte.

4. **D · bidirectional Dijkstra** — parallel two-pass CSR (atomic degrees, then
   `fetch_sub` scatter from each row's end) storing `(w<<32)|v`, plus the
   transpose; `INF = u64::MAX/4`; distances stored as `dist+1` with **0 = unset**
   so `alloc_zeroed` pages stay lazy; `touched` is pushed on the **first** label
   write and both tables must be all-zero between queries; keys pack a **21-bit
   node** (`n < 2^21`) allowing keys to 2^43, with a wide-heap variant when
   `n·wmax ≥ 2^43`; alternate sides, break when either heap is empty, and update
   `mu` **unconditionally** on every relaxation with saturating adds.
   Test vectors include the four parallel-edge orders, a zero-weight cycle, a
   self-loop, `s == t` on an isolated node, out-degree-0 source / in-degree-0
   target, the two overflow chains (packed vs wide heap must agree), a graph
   where the first meeting is not the answer, and **the stale-label pair**
   `(0,2)→2` then `(5,4)→51` on one thread's state.

## The three things to settle empirically at H1 (not by assumption)

The answer marks these `[S]` and they are exactly right to defer — the reference
defines them, and we can ask it directly:

1. **An unterminated quote** at EOF, and **bytes after a closing quote** other
   than `,`/CR/LF (`"ab"cd`). Proposed default: skip to the next delimiter.
   *Settle with:* `python tooling/conformance.py --track c --oracle` on a
   two-line fixture (`spec-cases`-style) so the reference's own parse decides.
2. **An empty phrase query** (`k == 0`) — proposed: no matches (`0 0`).
   *Settle with:* the same fixture route on track B.
3. **`-0.00` as a minimum** — proposed: normalises to `0`, printed `0.00`
   (the worked example's BB line agrees). *Settle with:* the C worked example,
   which is already in `verification/cases/track-c`.

## Why this is safe now

Every claim above is either (a) arithmetic verified by hand against the rank
formula and the spec's worked example, (b) a property test that compares against
the Python reference, or (c) one of the three items explicitly deferred to the
oracle. Nothing is taken on faith, and the property tests are cheap enough to run
as the first thing after each module lands.

---

# SELF-REVIEW (the P-E pass, done in-house — no further model credits spent)

I attacked the two items that carried residual proof obligations, plus the
termination rule. Result: **no new defects**, two proofs closed, one nuance
recorded. What follows is the reasoning, so it can be re-checked at H1.

## 1. Phrase matcher — cursor monotonicity holds, and the anchor is arbitrary

*Claim under test:* `cp[s] = gallop_ge(P[s], cp[s], tgt)` is sound "because
targets only grow".

- The outer loop walks the anchor slot's positions ascending, so
  `st = p - b` is non-decreasing, and `tgt = st + s` is non-decreasing for each
  slot. Therefore every `cp[s]` is only ever searched forward from its previous
  value — monotone, valid, no rescan. ✓
- **Completeness does not depend on which slot is the anchor.** A valid match
  occupies `st .. st+k-1`, so slot `b`'s term sits at `st + b`, which is in
  `P[b]`; the anchor loop reaches `p = st + b` and reconstructs exactly that
  start. Choosing the *shortest position list* is therefore purely a speed
  choice, not a correctness one — which is why `argmin` cannot introduce a wrong
  answer. ✓
- `if cp[s] == P[s].len() { return false }` is correct rather than premature:
  later anchors have strictly larger `tgt`, so once a slot has no position at or
  after `tgt`, it has none for any later anchor either. ✓
- Repeated terms are safe because cursors are indexed **per slot**, not per term
  (`"x x x"` holds three independent cursors over one shared slice). ✓
- `if p < b { continue }` correctly rejects starts that would begin before
  position 0.

## 2. Bidirectional termination — the empty-heap break is sound, and `top()` is a valid lower bound

*Claim under test:* breaking when either heap empties cannot miss a shorter path.

- If the forward heap empties, forward has **settled every node reachable from
  `s`** (a node is settled exactly when popped; the heap empties only after all
  of them are popped). On any shortest `s→t` path, the tail edge `(u,v)` has `u`
  reachable, so `u` was settled, and at that moment forward relaxed `(u,v)` and
  set `mu <= df(u) + w + db(v) = dist(s,t)`. Since `mu` is always the length of a
  real walk, `mu >= dist(s,t)`. Hence `mu == dist(s,t)`. ✓ The symmetric
  argument covers an emptied backward heap.
- `F.d[s] = 0` and `B.d[t] = 0` at init, so `lab(other, t)` / `lab(other, s)` are
  real values (never INF-from-unset) on those relaxations — which is what makes
  the argument above land. ✓
- **`top(S) = heap.peek() >> NB` is a lower bound on the minimum *unsettled*
  distance, so the `top(F) + top(B) >= mu` break is conservative.** Keys in the
  heap are >= their node's current label (labels only decrease), and the minimum
  unsettled label is itself present in the heap, so `top() <= min unsettled`.
  Settled nodes can leave **stale, smaller** keys behind, which makes `top()`
  *weaker* (smaller) — that only extends the search, it can never break early and
  return a too-large answer. This nuance was not in the original answer; it is
  the part of the proof that makes the rule safe rather than merely plausible. ✓

## 3. Adopted as-is, with the one nuance above noted

`mu` is updated **unconditionally** on every relaxation (including when `nd` is
not an improvement) — this is required, not optional: the value that completes
the shortest path may come from a relaxation of an edge whose target already had
a smaller label.

## 4. Decision: stop asking models about the design

The remaining risk is no longer in the algorithms — the arithmetic is verified
and the two proof obligations are closed. It sits in three places a model cannot
help with:

1. **Three empirical behaviours** deferred to the oracle (unterminated quote /
   bytes after a closing quote; empty phrase query; `-0.00` as a minimum). One
   `conformance --oracle` run each settles them, at H1.
2. **Implementation discipline** — the property tests (n = 1..200 vs the
   reference; CH = 1..80 sweeps; edge-order and thread sweeps; the all-zero
   assertion between queries) are what actually catch coding mistakes.
3. **Measurement**, which decides every later optimisation.

So: no further model passes on the design. Spend the remaining budget on the
oracle runs and the property tests at H1.
