# Track C — Join (DEEP, the DQ track, target K 8–9)

**Task:** parse a **hostile ~204 MB CSV** (quoted commas, escaped quotes `""`, embedded LF/CRLF
inside quoted fields, mixed line endings, a mid-file BOM), columns `id,name,note,region_id,amount`;
**join + sorted aggregates**. Reference ≈ 14.44 s. Timeout 144 s. **Most disqualifications happen
here — correctness first, always.** *(These figures are the published reference numbers.)*

> Join key = `region_id` and amount scale are **INFERENCE, not fact** — defer the lookup schema,
> aggregate key, output format, comparator, and amount scale to the frozen spec. All K are
> **TARGET/EXPECTED**.

## Corrected K budget (recompute `∏(ref/ours)^w` on the box)

`K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15`; speed score = `50·log10(K)`,
clamped to 0–100. Wall time is uncapped; memory is capped at 50×; S and D can cost but never pay.

| Row | T | M | S | D | K | Speed |
|---|---:|---:|---:|---:|---:|---:|
| Safe | 20× | 10× | 0.60× | 1 | **6.57** | 40.9 |
| Commit | 30× | 12× | 0.62× | 1 | **8.38** | 46.2 |
| Stretch | 35× | 12× | 0.62× | 1 | **9.05** | 47.8 |

Astra's §2.2 bands carried ~1% rounding noise (mixed direction); the Exceptional row was
*conservative*. The old "K 9 is the optimistic edge but inside the ~10 physical cap" reading is void
— **T is uncapped now**, so the ceiling is set by effort, not by a cap. Realistic band **K≈8–9**
commit/stretch. Track C memory can genuinely go high (it streams, never materializes all rows) —
unlike Track B.

## Algorithm & data layout
- **Scalar 4-state CSV machine** (`FieldStart / Unquoted / Quoted / AfterQuote`) is the **permanent
  oracle**. SIMD is **only a candidate-byte locator** feeding the scalar transitions — never a
  semantic replacement.
- **Serial structural pre-scan** for safe row boundaries. **PROHIBIT nearest-newline chunking** — a
  newline can live inside a quoted field (the #1 Track C landmine).
- **BOM:** context-gated recognition only; **refuse global `EF BB BF` stripping** (it would corrupt
  quoted content). Emit a BOM only if the reference does.
- **Amounts:** fixed-point integer **cents** (`i64` per record, `i128` aggregate) for bit-exact,
  order-independent sums. **`f64` prohibited** unless the ref demands it (parallel float reduction is
  non-associative → nondeterministic → SHA fail).
- Escaped quote `""` → `"` via a quote-run rule; embedded LF/CRLF preserved as data; trailing spaces
  preserved (no trim). **Explicit frozen comparator**; no hash-map iteration into output; **no
  invented tie-break.**
- Parallel: 6 P-cores default → sweep 12; thread-local aggregates; **deterministic worker-index
  merge.**

## Optimization ladder (PERF.md spine; keep ≥3 failed experiments)
1. **Correct scalar baseline** (no SIMD, no cleverness) → SHA-green. *Protect this — it's the H17
   milestone's #1 risk; defer all speed work to the deep block.*
2. Fixed-point cents; dense region IDs.
3. Serial structural pre-scan → safe parallel row ranges.
4. SIMD candidate-byte locator feeding the scalar machine.
5. Thread-local aggregation + deterministic merge.
Likely failed/reverted: nearest-newline chunking (splits quoted fields → DQ), global BOM strip
(corrupts data), perfect-hash-from-sample lookup (overfit → DQ), in-place mutation of the RO input.

## Determinism checklist
- [ ] Row boundaries from the structural pre-scan, never from naive newline search.
- [ ] Sums are integer cents; aggregate order fixed; no float reduction.
- [ ] Output rows in the frozen sort order; comparator pinned to the reference.
- [ ] 1-worker vs 12-worker byte-identical ×10; differential tester green on the hostile corpus.

## Memory & risk
- Streaming keeps peak modest; mmap/cgroup caution applies (measure `memory.peak` both ways).
- **Differential tester** is mandatory: candidate vs reference byte-diff with a first-diff reporter,
  run over a *generated* grammar-preserving hostile corpus (retain failing seeds). Never tune to the
  sample bytes.
- Risk: this is the DQ track — a single mis-handled quote/BOM/newline zeroes it. Correctness and the
  SHA gate dominate every speed decision.
