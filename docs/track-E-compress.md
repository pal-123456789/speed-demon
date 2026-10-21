# Track E — Compress (BASELINE+, lowest ceiling, target K 3–3.5)

**Task:** compress a **100 MiB (104,857,600-byte) 4-segment corpus** (prose / logs / json / blob)
with your **own codec**; the **M axis = compressed size** (not peak RAM); **decompression time is
weighted ~4× compression time.** Reference ≈ 29.8 s / **2.41× ratio**. Timeout 298 s. *(Figures
verbatim from the frozen context block.)*

> **Lowest ceiling of all five tracks** — don't over-invest. M carries only 0.20 weight, the ref is
> already a strong 2.41×, and T is self-limited by needing a fast decoder. All K are **TARGET/EXPECTED**.

## Corrected K budget — the most uncertain of the five (pin Mref first)

| Row | Lever | M axis = our_ratio / 2.41 | K (recompute on box) | Speed |
|---|---|---:|---:|---:|
| Realistic competitive | fast rANS decode + good ratio | ~1.27× (from §5 budget) | **≈2.66** | ~30 |
| Realistic stretch | + per-segment transforms | higher, less supported | **≈3.0–3.5** | ~33–38 |

**Headline correction (§2-vs-§5 M inconsistency).** The M axis is a **ratio of sizes**, i.e.
`(our compression ratio) / (ref 2.41×)` — *not* the raw compression ratio. Astra's §2 plugged the
raw "2.8×" straight into K (→ inflated 3.12). §5's own bottom-up segment budget (prose 3.8 / logs
5.0 / json 4.0 / blob 1.7) blends to only **~3.05× overall → M axis ≈ 3.05/2.41 ≈ 1.27×**, which
drops competitive K from 3.12 to **≈2.66**. §2's stretch was also understated (stated 4.2, ratios
give ~4.43). **Realistic Track E band is K ≈ 2.7–3.5; hard ceiling ~5–6 — the lowest of all five.**
The real levers are **T (fast parallel decode)** and **X**, not M.

## Algorithm & data layout — decoder-speed-first
- **One shared canonical rANS decoder** across all 4 segments; put the complexity in the **cheap
  encoder** (per-segment reversible transforms), keep the **expensive-weighted decoder simple + fast
  + parallel.** rANS beats range/arithmetic coding for decode speed. PPM/tANS stay encoder-only
  experiments.
- **Per-segment transforms (all with a mandatory raw-block fallback):**
  - **prose** → bounded-LZ + order-1/2 context (not full PPM).
  - **logs** → **reuse the Track A parser**: field-split + timestamp/latency deltas + method/endpoint
    dictionaries (**likely the biggest single ratio win**).
  - **json** → lexical tokenizer + key dictionary, **no AST** (preserve escapes, whitespace, number
    spelling byte-for-byte).
  - **blob (float32)** → column transpose + XOR/delta on the **raw u32 bit pattern, never float
    arithmetic** (preserves NaN payloads, ±0, inf, subnormals).
- **D=1:** hand-written coder; general-purpose compression libraries are banned.

## Optimization ladder (PERF.md spine; keep ≥3 failed experiments)
1. Raw-block container + minimal rANS, **exact round-trip** → SHA-green.
2. Shared canonical decoder; per-segment dispatch.
3. Logs transform (Track A tokenizer reuse) — the biggest ratio lever.
4. prose / json / blob transforms.
5. Fast **parallel** rANS decoder.
Likely failed/reverted: full PPM encoder (too slow to help the blended T), float arithmetic on the
blob (breaks bit-exactness), a denser coder that slows decode (loses on the 4×-weighted T).

## Determinism checklist
- [ ] The compressed file **is** the artifact — deterministic container: fixed magic/version/endian,
      fixed segment + block order, fixed table serialization, no timestamps/seeds/hash-order/uninit pad.
- [ ] 1-worker vs 12-worker output byte-identical.
- [ ] Float special-value matrix (NaN payload / ±0 / inf / subnormal) round-trips exactly.
- [ ] Exact round-trip verified across ≥10 runs.

## Memory & risk
- The scored "M" is **output size**, so the compression-time budget trades against it; keep peak RAM
  modest regardless.
- **Low-ceiling track → ship a solid, correct, deterministic baseline and the single biggest ratio
  win (logs), then stop.** Master plan buys ~5 h here; do not let E eat A/D/C deep time or a sleep
  block. Pin the exact **Mref**, container wrapper, segment geometry, and compression/decompression
  time blend from the frozen harness before trusting any K.
