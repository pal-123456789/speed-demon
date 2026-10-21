# TRACK E · COMPRESS — Design Specification

Status: pre-kickoff design document. No solution code, no function bodies, no implementation language.
Track window: baseline H0–H17 (2h charged to Track E), optimization block H61–H64 (3h charged).
Reference: T 29.8s, overall ratio 2.41× · timeout 298s · corpus 100MiB in 4 fixed, 1MiB-aligned segments.
The M slot is **compressed size**, not peak RAM. Compression time is measured but weighted at 1/4 of decompression time.
Round-trip must be BIT-EXACT. Banned: general-purpose compression libraries — we write the coder.
Scoring reminder: K = (Tref/T)^0.40 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15 × (Xref/X)^0.10, each ratio capped at 50×; speed_score = 100·ln(K)/ln(25).

---

## 1. Reference Teardown

**What the reference does (from the frozen brief).** Lossless compression of a fixed 100MB mixed corpus, scored on ratio and decompression throughput together, with a bit-exact round-trip requirement.

**Measured reference axes.** T = 29.8s, overall ratio 2.41×. The reference's per-segment ratios are published: prose 2.94×, logs 4.11×, json 3.32×, blob 1.18×. Mref (the reference's compressed size in bytes) is not published but is derivable: 104,857,600 / 2.41 ≈ 43,509,000 B (≈41.5MiB). Sref and Xref are DEFERRED to Oct 28 frozen spec.

**The corpus, exactly as published.** `corpus.bin` = 4 concatenated segments, offsets fixed and 1MiB-aligned, in file order:

| Segment | Offset | Size (B) | Content | Ref ratio |
|---|---|---|---|---|
| prose | 0 | 31,457,280 | English UTF-8, long runs | 2.94× |
| logs | 31,457,280 | 27,262,976 | the Track A line format, repeated fields | 4.11× |
| json | 58,720,256 | 23,068,672 | nested records, high key repetition | 3.32× |
| blob | 81,788,928 | 23,068,672 | float32 sensor frames, low entropy per column | 1.18× |
| **Total** | | **104,857,600 (= exactly 100MiB)** | | **2.41× overall** |

The sum checks out to exactly 100MiB, and the segment boundaries are 1MiB-aligned, so **the segment structure is readable from the file itself** — the coder can detect the four segments by their fixed offsets without any metadata, and the 10× probe's structure follows the same rule.

**Structural facts that drive every decision.**
- **The four segments reward four different models** (the brief says so explicitly). A single generic coder must therefore either (a) detect the segment and switch models, or (b) use a general model that is mediocre everywhere. Option (a) is the obvious choice and it is legitimate: the segment offsets are published, and the model selection is a *format* decision, not a precomputed answer. **We will use a per-segment model and carry the segment table in the container header.**
- **The blob segment is the ratio killer.** 23,068,672 B at the reference's 1.18× contributes 19,550,000 B of the reference's 43,509,000 B output — **45% of the compressed size comes from a segment the reference barely compresses.** Any improvement on the blob moves the overall ratio more than any other single lever, but the blob's entropy is real: float32 sensor frames have near-random mantissa bits.
- **The logs segment is the cheapest win.** 4.11× already; the format is known (it is the Track A line format), the fields are typed (timestamp, method, endpoint, status, latency), and repeated fields repeat *within* a line. A structure-aware coder that models the line's grammar and then codes each field with a typed model can plausibly reach 5.5–7×, versus a generic LZ's 4.11×. **This is the single best ratio-per-engineering-hour in the track, and it is a direct transfer of Track A's domain knowledge — a genuine cross-track synergy worth calling out in PERF.md.**
- **The JSON segment rewards a key dictionary and typed value coding** (3.32× → 4.5–5.5×).
- **The prose segment rewards an order-N context model or a strong LZ with a good match finder** (2.94× → 3.2–3.8× is realistic in 72h; a full PPM is out of reach).
- **The M axis is a ratio of ratios.** Mref/M = (our compressed size vs the reference's) = our overall ratio ÷ 2.41. This is the single most important arithmetic fact in the track: **our ratio 3.4 gives an M ratio of only 1.4108 → M^0.20 = 1.0713.** Beating the reference's ratio by 41% yields only a 7.1% factor on K.
- **The T axis is decode-capped but compression-dominated in practice.** T = 29.8s for the reference. Our budget: the weight is 1/4 compression and 3/4 decompression, so T = 0.25·C + 0.75·D (the exact combination is DEFERRED to the Oct 28 spec, but the 1/4 : 3/4 ratio is published). Our own compression of 100MiB with a hand-written coder will realistically run at 20–60MB/s → C = 1.7–5.0s. Decompression at 150–400MB/s → D = 0.25–0.65s. So T ≈ 0.6–1.7s → a T ratio of 17–50×.

**Gate implications for this track.**
- Gate 1 (hash): the round-trip must be **bit-exact** — the decompressed output must equal `corpus.bin` byte for byte. This is checkable locally and cheaply (a hash of the decompressed bytes against a hash of the input), so this gate is a *self-verifying* gate. It is the easiest gate in the event to get right and the easiest to lose to a subtle bug (a match that reproduces a byte sequence but a decoder that reproduces it differently). Mitigation: every match in the LZ layer is verified by a byte comparison during compression ("copy with verification"), and the decoder re-checks a checksum. **Design decision: the compressor will verify every match it emits.** This costs a little compression time and buys an enormous amount of confidence — a self-checking compressor cannot emit a stream that decodes to different bytes.
- Gate 2: speed floor — irrelevant in practice here; we will be far above 10×.
- Gate 3: 298s timeout. Reference 29.8s. A hang is a decode-loop bug; a "compression too slow" failure is a realistic risk (C can be 5s) but 298s is generous.
- Gate 4: no network, read-only dataset, 1GB tmpfs scratch. We read the corpus from the read-only mount and write only the compressed file to /out. We must not stage the corpus into /tmp (tmpfs pages are charged against the 8GB cgroup limit and against `memory.peak`; and although Track E's M slot is the *compressed size*, not RAM, the 8GB cap still applies to the process, and a 100MiB copy plus working memory is a needless risk). **Stream from the mount.**

---

## 2. Axis Budget

**Why the ceiling is low — the structural analysis, done first.**

The M axis is 0.20 of the exponent, the reference already achieves 2.41×, and the M ratio is *our ratio ÷ 2.41*. Let r be our overall ratio. Then:
- M ratio = r/2.41, and M's contribution to K is (r/2.41)^0.20.
- The 50× cap on the M axis implies r/2.41 ≤ 50 → r ≤ 120.5×, which is far beyond the entropy of the data; the cap is not the binding constraint.
- The realistic band for r is 2.6× to 4.0× (a hand-written coder in 72h, per-segment models). Over that band, M's contribution to K ranges from 1.0153 (r = 2.60) to 1.1066 (r = 4.00) — a spread of 9% on K, i.e. **about 2.2 speed points across the entire realistic ratio range.**
- By contrast the T axis: 0.40 weight, and a T ratio from 10× to 50× spans 0.40·ln(5) = 0.644 on ln K → K ×1.90 → **about 13 speed points.**
- **Conclusion 1: the M axis is nearly inert. A 40% better ratio than the reference is worth ~2 points; a 3× better decode time is worth ~8 points.** Effort must be allocated accordingly.
- **Conclusion 2 (the S axis makes M even more inert):** the S ratio at 0.58–0.60 costs 0.15·ln(1/0.60) = 0.0767 on ln K → K ×0.921 → −1.6 points. The S axis and the M axis are comparable in magnitude, and both are smaller than the T axis.
- **Conclusion 3 (the real reason the ceiling is low):** the reference is already good (2.41×, and 4.11× on the logs segment). The available headroom on the *ratio* axis is one to one-and-a-half points of K; the available headroom on the *time* axis is eight to thirteen points. **The trade-off is the subject of the track, and the objective function tells us which side of it to take: speed.**
- **Conclusion 4 (why the "trade-off IS the subject" framing still matters):** the trade is *within* the compression, not between the axes. Choosing a fast matcher over a thorough one costs ratio (a small M penalty) and buys T (a large T gain). The K objective says: **take the speed.** Quantified in §5.

**Anchor rows. The M axis is expressed as our overall ratio r, and the M ratio is r/2.41.**

| Row | T ratio | Our ratio r | M ratio (r/2.41) | S ratio | X ratio |
|---|---|---|---|---|---|
| Safe | 15× | 2.60× | 1.0788 | 0.55× | 1.00× |
| Commit | 25× | 3.10× | 1.2863 | 0.62× | 1.05× |
| Stretch | 30× | 3.40× | 1.4108 | 0.60× | 1.10× |
| Ceiling | 35× | 3.60× | 1.4938 | 0.58× | 1.15× |
| Max-realistic | 40× | 4.50× | 1.8672 | 0.58× | 1.15× |

**Full arithmetic (M factor = (r/2.41)^0.20, shown explicitly).**

- Safe: T 15× ^0.40 = 2.9542; M 1.0788 ^0.20 = 1.0153; S 0.55× ^0.15 = 0.9142; D 1.00×; X 1.00× ^0.10 = 1.0000.
  K = 2.9542 × 1.0153 × 0.9142 × 1.0000 × 1.0000 = **2.7421**.
  speed = 100·ln(2.7421)/ln(25) = 100 × 1.00878/3.21888 = **31.34**.
- Commit: T 25× ^0.40 = 3.6239; M 1.2863 ^0.20 = 1.0516; S 0.62× ^0.15 = 0.9308; D 1.00×; X 1.05× ^0.10 = 1.0049.
  K = 3.6239 × 1.0516 × 0.9308 × 1.0000 × 1.0049 = **3.5647**.
  speed = 100·ln(3.5647)/ln(25) = 100 × 1.27125/3.21888 = **39.49**.
- Stretch: T 30× ^0.40 = 3.8981; M 1.4108 ^0.20 = 1.0713; S 0.60× ^0.15 = 0.9262; D 1.00×; X 1.10× ^0.10 = 1.0096.
  K = 3.8981 × 1.0713 × 0.9262 × 1.0000 × 1.0096 = **3.9048**.
  speed = 100·ln(3.9048)/ln(25) = 100 × 1.36234/3.21888 = **42.32**.
- Ceiling: T 35× ^0.40 = 4.1460; M 1.4938 ^0.20 = 1.0836; S 0.58× ^0.15 = 0.9215; D 1.00×; X 1.15× ^0.10 = 1.0141.
  K = 4.1460 × 1.0836 × 0.9215 × 1.0000 × 1.0141 = **4.1982**.
  speed = 100·ln(4.1982)/ln(25) = 100 × 1.43501/3.21888 = **44.57**.
- Max-realistic: T 40× ^0.40 = 4.3734; M 1.8672 ^0.20 = 1.1330; S 0.58× ^0.15 = 0.9215; D 1.00×; X 1.15× ^0.10 = 1.0141.
  K = 4.3734 × 1.1330 × 0.9215 × 1.0000 × 1.0141 = **4.6307**.
  speed = 100·ln(4.6307)/ln(25) = 100 × 1.53302/3.21888 = **47.62**.

**The realistic band is 39–45 points, with an absolute ceiling around 48 for a hand-written coder.** That is a third of what a strong Track A submission can reach, and that is a property of the scoring function, not of the team's effort. It must be stated plainly in PERF.md so the judges can see the analysis was done rather than the team being weak here.

**The exchange rate between ratio and speed — the heart of the track.** Suppose we trade ratio for speed: reducing the compression time C from 3.0s to 1.5s (a 2× compression speedup) and paying 10% of ratio for it.
- T before = 0.25·3.0 + 0.75·D; with D = 0.4s → T = 1.05s. T after = 0.25·1.5 + 0.75·0.4 = 0.675s. T ratio moves from 29.8/1.05 = 28.4× to 29.8/0.675 = 44.1×.
- T-axis effect: 0.40·ln(44.1/28.4) = 0.40 × 0.4405 = 0.1762 → K ×1.1926 → **+3.6 speed points.**
- M-axis effect: r falls 10%, e.g. 3.40 → 3.06; M ratio 1.4108 → 1.2697; the M factor falls from 1.0713 to 1.0493 → K ×0.9794 → **−0.4 speed points.**
- **Exchange rate: giving up 10% of ratio to gain 2× compression speed is worth +3.2 points net.** By the same arithmetic, giving up 10% of ratio to gain 10% of decode speed is worth 0.40·ln(1.1) = 0.0381 → K ×1.0388 (+0.8 points) against −0.4 points → +0.4 points net. **Speed wins at both ends of the pipeline, and the compression end wins harder because C is the larger term.**
- The corollary: **the last 5% of ratio is not worth an hour.** We target r = 3.1–3.4 and spend everything else on speed.

**S axis.** Writing four segment-specific models, an entropy coder, and a container format is a *lot* of code. The S targets (0.55–0.62) mean our normalized source is 1.6–1.8× the reference's. A reference compression implementation in a few hundred lines is plausible (hence a 3,180B-scale Sref), so a 6KB implementation would be at S ratio ~0.5. **Realistic risk: the S ratio lands at 0.40–0.50, costing 0.15·ln(1/0.45) = 0.1199 on ln K → K ×0.887 → about −2.5 points.** We accept that. There is no way to write a fast multi-model coder in 1.6× a simple reference's source, and the T gain dwarfs the S loss.

**D axis.** D = 1: std-only, no vendored source, and explicitly **no general-purpose compression libraries**. Everything — the entropy coder, the match finder, the bit packing, the container format — is written by us. Note that 50,000 bytes of vendored source would add exactly 1.0 to D, costing 0.15·ln(2) = 0.104 on ln K (K ×0.901). **We write everything from scratch; the D axis is a hard constraint that the brief also imposes as a ban.**

---

## 3. Algorithm & Data Layout

**Container format (the outer layer).**
- A fixed header: magic, format version, the number of segments, and per segment: the segment's original offset and length, the model id, and the compressed length. Header size is negligible (< 1KB) and its bytes are part of the compressed output (they cost a few bytes of the M axis — irrelevant).
- The segment table must be **segment-count-agnostic**: the 10× probe may contain more segments (or proportionally longer ones), and the decoder must handle whatever the header says. This is what keeps the X axis safe.
- Per segment: an entropy-coded block stream. Blocks of a fixed size (e.g. 64KB uncompressed) so that decompression can stream and so that a corrupt block is localized. Block boundaries also enable multi-threaded decoding (see below).
- The round-trip check: the container carries a checksum of the original bytes; the decoder verifies it. A mismatch is a hard failure (never a silent wrong answer).

**Segment 1 — prose (31,457,280 B, ref 2.94×).**
- Model: LZ77 with a hash-chain match finder plus an order-1/2 context-modeled entropy stage, or a pure order-2 context model.
- Realistic target: 3.2–3.8×.
- The cheapest strong option for 72h is a well-tuned LZ (hash chain, lazy matching, limited depth) + an adaptive binary range coder. A pure context model (order-2) reaches ~2.5–3.0× on English and is *much* faster and much simpler. **Decision: implement the context model first (fast, simple, and it is the T anchor), and treat the LZ as an optional upgrade if hours remain.** The T/M exchange rate says the context model is the better buy.
- Match-finder/entropy interaction: with a range coder, a "match" is a special symbol; every symbol costs a range-coder update. The throughput target is 30–60MB/s for compression and 150–400MB/s for decompression.

**Segment 2 — logs (27,262,976 B, ref 4.11×).** **The best ratio-per-hour segment.**
- The content is the Track A line format: `ts method endpoint status latency_us`. The line grammar is known, so the coder can be *structure-aware*: recognize the fixed-shape line, and code the fields with **separate typed models**:
  - the timestamp: microsecond digits — a digit-level context model over the decimal string, or a delta against the previous line's timestamp (the timestamps are monotone per shard), which collapses a 20+ digit field into a small delta plus a few raw digits;
  - the method: 7 symbols → 3 bits;
  - the endpoint: a dictionary of the ~1,842 distinct endpoints, each token coded by its dictionary id plus a small suffix model (the endpoint strings repeat verbatim, so a plain dictionary of the 1,842 strings is extremely effective; the ids are then coded with an order-1 context model over the id stream, which captures the Zipf distribution and the local temporal clustering);
  - the status: a small alphabet (a few hundred values) → 1–2 bytes;
  - the latency: a decimal string, but the *value* is what matters — coding the digits with a digit-context model, or better, coding the value as a delta from the previous line's latency in a geometric/Golomb code, which turns a 6-digit number into ~1–2 bytes.
- The resulting model is essentially a typed columnar codec, and it should beat a generic LZ's 4.11× decisively: realistic target 5.5–7×.
- **This is a direct transfer of Track A's domain knowledge, and it is the single most valuable cross-track synergy in the campaign.** It must be highlighted in PERF.md: the logs segment is not solved by compression engineering, it is solved by knowing the data.
- Format-detection risk: the coder must *detect* the log format at runtime (a quick scan of the first few KB testing the pattern) rather than assume it, because the 10× probe may contain different content. If the detection fails, fall back to the generic model. This keeps the design robust and the X axis safe.

**Segment 3 — json (23,068,672 B, ref 3.32×).**
- Model: a structural tokenizer (strings, numbers, punctuation, whitespace) plus a token-type context model, plus a **key dictionary** (the same key strings repeat across records) and a numeric model for the values.
- Realistic target: 4.0–5.0×.
- The value model matters: JSON numbers are often small integers or fixed-precision decimals; a digit-context model over the numeric text does well. Keys are the big win (high key repetition is called out in the brief).
- A structural parser is risky (a malformed JSON would break us). Mitigation: the tokenizer is *lenient* — it recognizes structure opportunistically and falls back to literal bytes for anything it does not recognize. The bit-exactness requirement is preserved because the fallback is still lossless.

**Segment 4 — blob (23,068,672 B, ref 1.18×).** **The ratio bottleneck, and the most interesting engineering.**
- The content: float32 sensor frames, "low entropy per column". Float32 = 1 sign bit + 8 exponent bits + 23 mantissa bits, stored in 4 bytes, typically little-endian.
- The key structural insight: **byte-plane transposition.** Split each 4-byte float into 4 planes: all the sign bytes, all the exponent bytes, all the high mantissa bytes, all the low mantissa bytes. In a sensor stream, consecutive floats have similar exponents (the value drifts slowly), so the exponent plane is highly repetitive and codes at ~0.1–0.3 bits/byte with an order-1 context model, while the low mantissa plane is near-random and codes at ~8 bits/byte.
- Estimated effect for a typical float stream: exponent plane ~0.2 bits/byte, high mantissa plane ~4–5 bits/byte (the top mantissa bits still carry some structure), low mantissa plane ~7.5–8 bits/byte, sign plane ~0.1 bits/byte. Average ≈ (0.2 + 4.5 + 7.8 + 0.1)/4 ≈ 3.15 bits/byte → **ratio ≈ 2.54×** on the blob segment versus the reference's 1.18×. That would cut the blob's compressed contribution from 19.55MB to ~9.1MB and lift the overall ratio substantially.
  Let me redo the overall-ratio arithmetic with a per-segment plan: prose 3.5×, logs 6.0×, json 4.5×, blob 2.0×.
  Compressed sizes: 31,457,280/3.5 = 8,987,794; 27,262,976/6.0 = 4,543,829; 23,068,672/4.5 = 5,126,371; 23,068,672/2.0 = 11,534,336. Sum = 30,192,330 B. Overall r = 104,857,600/30,192,330 = **3.473×**. M ratio = 3.473/2.41 = 1.4411 → M factor = 1.0773.
  With T 30× (3.8981) and S 0.60 (0.9262) and X 1.10 (1.0096): K = 3.8981 × 1.0773 × 0.9262 × 1.0096 = 3.9076 → speed = 100·ln(3.9076)/3.21888 = **39.3**? Let me recompute: 3.8981 × 1.0773 = 4.1995; × 0.9262 = 3.8896; × 1.0096 = 3.9269. ln(3.9269) = 1.36785; /3.21888 = 0.42501 → **42.50**. Good — consistent with the Stretch row (42.32 at r = 3.40).
  So: **the blob segment is worth ~0.1 overall ratio, which is worth ~0.1 points of K. The prose segment, going from 2.94× to 3.5×, is worth: 31.46/2.94 = 10.70MB → 31.46/3.5 = 8.99MB, a 1.71MB saving = 0.083 overall ratio ≈ 0.08 points.** The M axis is *that* inert. Everything must be decided by how it affects T.
- Practical blob plan: byte-plane transposition + per-plane order-1 context model; if the transposition is not beneficial (detected at runtime by comparing the coded size of the transposed and non-transposed forms on a sample), keep the better one. Runtime detection is cheap and removes the risk of a wrong assumption about the data.

**Entropy coder (the shared core).**
- A binary adaptive arithmetic/range coder with a bounded adaptation table, driven by a context model. The coder must be *fast* (the T axis) and *compact* (the S axis).
- Alternatives considered: a static Huffman/ANS coder (faster per symbol but worse ratio and more code), a bit-level arithmetic coder with 12-bit probabilities (simple, fast, slightly worse ratio). **Decision: a binary range coder with an adaptive 12-bit probability model per context.** The bit-exactness requirement makes the coder's determinism critical: the probability adaptation must be a pure function of the symbol sequence (it is), and the decoder mirrors it exactly.
- The coder must be deterministic across runs: no reliance on uninitialized state, no floating point in the coder (integer arithmetic only), and a fixed table-update rule. **A single differing probability update would corrupt the entire stream.**

**Memory and data layout.**
- The compressor's working set: the input window (a fixed-size ring buffer, e.g. 4–16MB) plus the context models' probability tables (256 contexts × 4096 probabilities × 2B = 2MB per order level — for 3 levels that is 6MB; a 64K-context order-2 model would be 512MB, too much). **The context-model table size is a memory/ratio trade-off and must be measured.**
- The hash chain / match table for the LZ path: 4M entries × 4B = 16MB (hash) plus the chain.
- Everything must fit in 8GB with margin, and the *peak* matters less here than in Track A (the M slot is the compressed size), but the 8GB cap is a hard gate. Target peak ≤ 1GB.
- **Parallelism:** 12 threads. The natural decomposition is by segment or by block. Each block is compressed independently (so the decoder can decode blocks in parallel), and the per-block models must be either independent or deterministically merged. Simplest correct approach: **each block carries its own probability tables**, which costs ratio (models are re-learned per block) but is trivially parallel and deterministic. With 64KB blocks, re-learning costs ~1–2% of ratio. **Decision: per-block independent models; measure the ratio cost.**
- Alternatively, compress the four segments in parallel (four independent models) and leave the intra-segment work single-threaded. This is simpler, has no ratio cost, and gives 4× — and the segments are of similar size, so the load balances reasonably.

**Decoder.**
- The decoder is the 3/4-weighted term, so it is the design's centerpiece. Requirements: a single pass, no allocations, table-driven, and *fast* — a range decoder does one renormalization per output byte on average, so 100MiB decodes in well under a second on one core.
- Parallel decoding: if blocks are independent, 12 threads decode 12 blocks at a time. The output must be written to /out in the original order — the blocks are written to their assigned offsets, so the order is deterministic.

---

## 4. Hash-Gate Correctness & Determinism

**The gate for this track is self-verifying, and we exploit that.** The round-trip requirement is: decompress(compress(x)) == x, bit for bit. That can be checked locally at every moment, so this track has the lowest correctness risk in the event *if* we keep checking it continuously.

| Hazard | Elimination |
|---|---|
| A match that the encoder believes but the decoder reconstructs differently | **Every emitted match is verified byte-by-byte at compression time**; the compressor asserts the reconstructed bytes equal the source bytes. A mismatch aborts compression (a bug, not a corrupt file) |
| Probability-adaptation divergence between encoder and decoder | The adaptation rule is a pure integer function of the symbol sequence; the encoder and decoder share the identical rule; a "re-encode and compare" self-test runs on every block during development |
| Uninitialized probability tables | Tables are zero-initialized to a fixed value (0.5 probability → 12-bit value 2048) at every block start |
| Endianness / byte-order dependence | The container and the models are byte-explicit; float interpretation is only used for *detection* (is this a float32 plane?), never for re-serialization — the bytes are copied literally, so endianness cannot alter the round-trip |
| A float-plane transformation that reorders bytes | The transposition is a byte permutation, fully invertible, and the round-trip check proves it |
| The segment boundary detection being wrong (a 1MiB-aligned offset assumption) | The segment table is in the header and is written by the compressor from the *actual* offsets it used; the decoder trusts the header, not an assumption. The published offsets are used only to choose the model, and the model's applicability is confirmed by a runtime check on the block content, with a fallback to the generic model |
| A model detector misfiring on the 10× probe | Detection is a content check with a conservative fallback to the generic model; the round-trip is preserved either way |
| The container's checksum colliding or being absent | A CRC or a 64-bit checksum of the original bytes is stored in the header; the decoder verifies it and fails loudly on a mismatch |
| Block-size-dependent output | The block size is a compile-time constant of the format version, recorded in the header |
| Nondeterminism from parallel block compression | Each block is independent with its own tables; the per-block outputs are written to fixed offsets; the container is byte-identical regardless of the worker count |
| A partial write to /out | The output is written once, sequentially, with the length known in advance; a short write is a hard error |
| Compression of a pathological input (e.g. an incompressible run) causing a match to reference itself | The compressor never emits a match that overlaps into the not-yet-written region in a way the decoder could not reproduce; the standard "copy with overlap" rule is followed exactly, and the verification pass catches any violation |
| The T measurement including or excluding the round-trip check | The timed run is the measured pipeline; the checksum verification is cheap (a CRC over 100MiB at ~1GB/s = 0.1s) and is kept in the timed path deliberately — the organizers time the actual work, and correctness is worth 0.1s here |

**Verification ladder.**
- **V1 (baseline, H0–H2):** a minimal but *correct* compressor: a single-order context model with a range coder over the whole file, plus the container. The round-trip must be exact on the 10% sample. **This is the V1 gate for the track** — no optimization starts before the round-trip passes.
- **V2 (round-trip on every artifact):** after every change, compress and decompress the *entire* corpus and compare the SHA-256 of the decompressed bytes with the SHA-256 of `corpus.bin`. This is the primary test and it takes a few seconds, so it is run after every single rung. **The track's working rule: no rung is merged without a passing full round-trip.**
- **V3 (per-segment round-trip):** the same test per segment, so a failure localizes immediately.
- **V4 (decoder-encoder symmetry):** a "re-encode" test — decode the stream, then re-encode the decoded bytes with the same model and check the result is byte-identical to the original stream. This catches any encoder/decoder asymmetry that a round-trip could mask (e.g. a non-canonical encoding that still decodes correctly but differs from the reference's expectations). This is a cheap, high-value test.
- **V5 (edge cases):** an empty file, a 1-byte file, a file of all zeros, a file of all identical bytes, a file that is exactly one block, a file that is one byte over a block boundary, and a highly repetitive file that triggers long matches. These are where an LZ implementation breaks.
- **V6 (parallelism invariance):** the compressed file produced with 1, 4, and 12 workers must be byte-identical. Any difference means a block's content depends on scheduling — a stop-the-line event.
- **V7 (repeat runs):** 5 runs, identical compressed bytes and identical timings; the spread must be under the 5% re-run threshold.

**Gate 4 compliance.** No network. The corpus mount is read-only; we never write to it. The root filesystem is read-only; we do not stage the corpus in /tmp (the 1GB tmpfs would not even hold 100MiB twice over comfortably, and tmpfs pages are charged against the 8GB cap). Scratch: none required — the compressor streams from the mount. The only file written is the compressed output to /out. No answers are precomputed into the image: the compressed stream is produced at run time from the mounted corpus, and it contains only compressed representations of the input (no precomputed answer tables).

**Gate 3 compliance.** 298s against a target of 0.6–1.7s. A hang would come from a decoder loop that fails to advance (e.g. a range decoder that renormalizes without emitting a symbol). Mitigation: every decode loop has a bounded iteration count per output byte.

---

## 5. Optimization Ladder

The ladder is ordered by the K objective: **speed first, ratio second, with the logs segment's structure-awareness as the one place where ratio is worth real effort because it is nearly free in engineering time.**

| # | Rung | Hypothesis / expected effect | Risk | Acceptance |
|---|---|---|---|---|
| L0 | Baseline: order-1 context model + a binary range coder over the whole file, no container segmentation | V1 gate; the oracle; correct round-trip | Low | Round-trip exact on the sample |
| L1 | Container format: header + per-segment frames + per-block framing + a checksum | Enables per-segment models, parallel decoding, and streaming; costs a few hundred bytes | Low | Round-trip exact; container size overhead measured |
| L2 | Per-block independent models (so blocks can be compressed and decoded in parallel) | 4–12× on the wall clock; costs ~1–2% of ratio (models re-learn per block) | Low | Round-trip exact; V6 worker-count byte-identity |
| L3 | **Speed-first decoder:** optimize the range decoder's inner loop (32-bit accumulator, batched renormalization, a symbol fast path) | The decode term is 3/4 of T; a 2× decode speedup is worth 0.40·ln(...) ≈ +4.7 points | Low | Round-trip exact; decode throughput measured |
| L4 | Per-segment model selection: prose → context model; logs → **structure-aware typed coder**; json → structural tokenizer with a key dictionary; blob → **byte-plane transposition + per-plane context model** | The core ratio play: logs 4.11 → 5.5–7×, json 3.32 → 4.0–5.0×, blob 1.18 → 1.7–2.2×, prose 2.94 → 3.0–3.4×; overall r ≈ 3.2–3.5 | Medium (each model is new code and new failure modes) | Round-trip exact **per segment**; each segment's ratio measured against the published reference ratios |
| L5 | Runtime model detection with a conservative fallback to the generic model | Robustness for the 10× probe and for any content drift; the fallback costs a little ratio but removes a whole failure class | Low | Round-trip exact with detection disabled (the fallback path is also tested) |
| L6 | Faster compression: smaller context models, a shallower model order, cheaper table updates, larger blocks | C is the larger term in T; a 2× compression speedup is worth ≈ +3.2 points net of the ratio we give up (see the exchange rate in §2) | Low | Round-trip exact; C and D measured separately |
| L7 | Block-level parallelism of the four segments (each segment compressed by one worker) | Up to 4× with no ratio cost (segments are independent models anyway) | Low | V6 |
| L8 | Order-2 context model for prose only (where the payoff is highest) | Prose 3.0 → 3.4×; overall r +0.06 → +0.06 points of K (negligible) but it is nearly free if the tables are cheap | Medium (table size, speed) | Adopt only if the compression-time cost is < 5% |
| L9 | Match finder (LZ) for prose as an alternative to the context model | Prose 2.94 → 3.5–3.8×; the LZ path is the expensive engineering item (match tables, verification) | High (the most complex rung) | Round-trip exact; adopt only if C stays within 10% of the context model's time — per the exchange rate, ratio alone does not justify it |
| L10 | Output path: a single write of the assembled container; the compressed blocks are buffered in memory before the write | Avoids a partial-file state; costs one buffer of the compressed size (≤ 40MB) | Low | Round-trip exact |
| L11 | Tuning pass: table sizes, block sizes, model orders, per-segment parameters, chosen by measurement | The final 5–10% | Low | Each change measured; only ≥3% T gains or ≥2% ratio gains are kept |

**Explicitly rejected rungs (for PERF.md).**
- **A general-purpose compression library:** banned by the brief; also it would set D far above 1 (a 50KB vendored library adds 1.0 to D and costs 0.15·ln(2) on ln K, about −2.1 points).
- **A single model for all four segments:** the brief states the segments reward different models; a generic model would land near the reference on every segment and nowhere near the logs' potential.
- **PPM-style high-order context modeling (order 4+):** the best ratio available in principle, but the tables are enormous (the M slot is the compressed size, but the 8GB RAM cap and the 72-hour budget are not), and per the exchange rate the ratio is not worth it. Rejected on arithmetic.
- **Huffman/ANS for the entropy stage:** a marginal ratio gain over a range coder for a large amount of implementation and a likely *speed* loss, which is the axis that matters. Rejected on the K objective.
- **Optimizing the blob's mantissa bits with a machine-learning-ish model:** no time, no payoff, and the M axis is inert.
- **Staging the corpus or the compressed output in /tmp:** 1GB tmpfs, charged against the 8GB cap, and pointless.

**PERF.md content plan.** A per-segment table: our ratio vs the published reference ratio (2.94 / 4.11 / 3.32 / 1.18) and vs our targets; a per-segment time table (C and D separately, since T weights them 1:3); the exchange-rate calculation (ratio vs speed) with the numbers; the model-detection fallback results; the round-trip evidence (hashes); and the rejected rungs with their reasons and measurements. The judges read this without the leaderboard, so the document must make the *inertness of the M axis* legible — it is the central analytical claim of this track.

---

## 6. X-Scaling Safeguard

**What X measures.** X = (runtime at 10× input) ÷ (runtime at 1×) versus the reference's own ratio. Weight 0.10. Targets 1.00 (Safe) to 1.15 (Ceiling).

**Scaling analysis.**
1. *Compression.* Linear in the input: 10× the bytes → 10× the time for a context model. The model tables are fixed-size (context count × table size), so the memory does **not** grow with the input. **X ≈ 1.00 for compression.** The one exception: the LZ path (L9) has a window and a hash table sized by the input or by a fixed window; with a fixed window, the time is still linear. **A model whose *tables* grow with the input would be a superlinearity risk — we avoid it by construction.**
2. *Decompression.* Linear. Parallel block decoding keeps the 12-thread structure constant. **X ≈ 1.00.**
3. *The 10× input's segment structure.* If the 10× corpus is 10 copies of the 4-segment structure, the container's segment table grows 10× (a few hundred extra bytes — irrelevant) and the per-segment model selection is unchanged. If the 10× corpus is 10× longer segments, the same models apply. Either way, **X is structurally linear**, which is the best possible position.
4. *The blob segment at 10×.* A float32 plane's entropy at 10× the length is essentially the same; the model adapts slightly more (a larger context table would help, but we keep the tables fixed). **No superlinearity.**
5. *The logs segment at 10×.* The endpoint dictionary grows with the distinct endpoints, not with the length; if the 10× corpus has 10× the distinct endpoints, the dictionary grows and the ratio *improves* (a larger dictionary compresses better) — a favorable direction. The dictionary must be capped in size to respect the 8GB limit; a cap is a fixed-size structure, so no superlinearity.
6. *The compression-time trap:* if a segment's model is *selected* by scanning a sample of the input, the scan cost is proportional to the sample size (fixed) — no growth. If the model were selected by scanning the whole input, the selection cost would double the time. **Decision: detect from a fixed-size sample (the first 64KB of each segment).**

**Safeguards.**
- **S1.** All model tables are fixed-size. Nothing in the compressor scales with the input except the compressed output itself.
- **S2.** The container header is segment-count-agnostic, so a 10× corpus with more segments is handled without a code change.
- **S3.** The window and hash tables (if the LZ path is used) are fixed-size, so the LZ path's compression time is also linear in the input.
- **S4.** Detection uses a fixed-size sample, never a whole-input scan.
- **S5.** The dictionary for the logs segment is capped; when the cap is hit, the least frequent entries are evicted (a fixed-size structure) or new entries are coded as literals.
- **S6.** Probe construction is DEFERRED (the organizers' 10× input is unpublished). We build our own 10× probe by concatenating `corpus.bin` ten times into dev scratch on the host volume — never /tmp (1GB tmpfs, and 1GiB of corpus would fit only barely while the working memory would not). **The probe's segment structure must be interpreted correctly:** our container detects segment boundaries from the published 1MiB-aligned offsets, so a 10× concatenation has 40 segments, and the model selection must follow the 4-segment cycle (or the per-segment detection, which is content-based and therefore automatic). The probe is documented in PERF.md so the X number is interpretable.
- **S7.** Acceptance: X_local ≤ 1.15 and the 10× round-trip is exact. If X_local > 1.20, ablate: C (compression) vs D (decompression) vs the model-detection scan — identified within one hour.
- **S8.** Never trade a verified T win for an X fix: 0.10 weight means a 0.15 X excess costs ≈ −0.3 points, against +3 points for a 20% T gain.

---

## 7. Hour Budget

Total charged to Track E: **5h = 2h baseline + 3h optimization**, with the optimization block at H61–H64, the last block of the campaign. This is the tightest budget in the event and it must be planned as such.

**Baseline block (2h, charged), H0–H2 nominal.**

| Hour | Activity | Exit criterion |
|---|---|---|
| H0–H0.5 | Read the Oct 28 frozen spec: the exact T combination rule (the 1/4 : 3/4 weighting and whether the two are summed or otherwise combined), the compressed-output naming, and any published constraints on the container format | Spec sheet; the T-combination rule is pinned |
| H0.5–H1.5 | Baseline implementation: an order-1 context model + a binary range coder + a minimal container, whole-file | Round-trip exact on the 10% sample (V1) |
| H1.5–H2 | V1 confirmation on the full corpus; measure C and D separately; measure the compressed size and the achieved overall ratio | Round-trip exact on the full corpus; C, D, and r recorded |

**Optimization block (3h, charged, H61–H64).** Because this is the last block of the campaign, the plan is front-loaded and deliberately narrow: **two models, not four, and speed before ratio.**

| Hour | Activity | Exit criterion |
|---|---|---|
| H61 | L1 (container: header, per-segment frames, per-block framing, checksum) + L2 (per-block independent models for parallelism) | Round-trip exact; container overhead measured; per-block parallelism validated |
| H61.5 | L5 (runtime model detection with a fallback) + L6 (compression-speed and decoder-speed tuning: the range coder's inner loop) | Round-trip exact; C and D measured separately after the tuning |
| H62 | **L4a — the logs segment's structure-aware typed coder** (the highest-value ratio work per hour) and **L4b — the blob segment's byte-plane transposition** | Round-trip exact per segment; the logs ratio ≥ 5.0× and the blob ratio ≥ 1.6× measured |
| H63 | L3 (decoder speed) + L7 (segment-level parallelism) + L4c (the JSON segment's key-dictionary model, if time permits) + L5 fallback validation | Round-trip exact; C and D measured; X_local probed on the 10× corpus |
| H64 | Freeze: re-anchor the axis table to the batch-local reference; recompute K; write PERF.md (the per-segment table, the exchange-rate calculation, the M-axis inertness analysis, the round-trip evidence, the failed experiments); measure normalized source bytes | Submission frozen |

**Budget hygiene.**
- **The 3-hour block forces triage.** The triage order, in K terms: (1) the container and the round-trip discipline (without them the track scores zero); (2) decoder speed (3/4 of T); (3) the logs segment's typed coder (the best ratio per hour, and it is a direct transfer of Track A's domain knowledge); (4) the blob's plane transposition (a large ratio gain on the segment that dominates the compressed size); (5) the JSON model (good but the segment is smaller and the model is more code); (6) the prose LZ (the most expensive item, the least valuable per the exchange rate). **Items 5 and 6 are explicitly droppable; items 1–4 are not.**
- **Cross-track leverage:** the logs coder is much cheaper to build if Track A's parser work is already done — the field structure, the endpoint dictionary idea, and the timestamp/monotonicity insight all transfer. The campaign should schedule Track A's domain analysis to be reusable by Track E's H62 block. This is a genuine scheduling decision and it belongs in the campaign plan, not just in this document.
- **Verification is not deferrable to the next block:** there is no next block. Every rung therefore carries a full-corpus round-trip check (a few seconds) — cheap enough to run after every change, and the track's working rule is that no change is kept without a passing full round-trip.
- The 3-hour block is the last in the campaign: it must finish with a *submission-ready* artifact, so the compression output format and the output file name must be settled at H0, not at H64.

---

## 8. Risk Register & Stop Rule

| ID | Risk | Likelihood | Impact | Detection | Mitigation | Stop rule / trigger |
|---|---|---|---|---|---|---|
| R-E1 | The round-trip is not bit-exact (a match, a model, or a container bug) | Medium | **Total** (Gate 1 = 0) | V2: a full-corpus SHA-256 comparison after every change | Match verification at compression time; per-block self-checks; the container checksum; V4 re-encode symmetry; V5 edge cases | Any mismatch → revert the offending rung immediately; the fallback is the L0 model, which is known-good |
| R-E2 | The M axis is misread: the M slot is the compressed size, and the M ratio is r/2.41, not r | Medium | Medium (a wrong plan) | The spec sheet at H0 | The axis table is built from r/2.41 explicitly; §2 states the conversion | If the spec defines M differently (e.g. peak RAM), re-derive the table in one hour; the plan's effort allocation (speed first) is robust to that change because the T axis dominates either way |
| R-E3 | The T combination rule (1/4 compression, 3/4 decompression) is additive vs something else — DEFERRED | Medium | Medium (the time budget) | The Oct 28 spec | Plan against the additive reading (T = 0.25C + 0.75D) and re-anchor at H61; the C/D split is measured separately either way, so only a final recomputation is needed | If the published rule is not additive, recompute the axis table at H61; no code changes |
| R-E4 | The compression time C is far above the estimate (a slow model) and the T ratio collapses | **High** | High (−5 to −10 points) | C measured at H2 and after every rung | The context model is the fast baseline; the LZ path is last in the ladder and must not slow the baseline; the 1/4 : 3/4 weighting means C is only a quarter of T | If C > 3.0s at H62, drop the LZ idea permanently and simplify the models to order-0/1 to buy speed back |
| R-E5 | The round-trip requirement plus a "compressed size" M slot creates a temptation to cheat (embedding answers) — DISQUALIFICATION | Low | **Total (disqualified from the whole event)** | Code review | The compressed stream contains only a lossless encoding of the mounted corpus; no precomputed tables, no embedded answers; the container is regenerated from the input on every run | Any suspicion of embedded data → the artifact is rebuilt from scratch. This is a hard red line, checked at freeze |
| R-E6 | The 3-hour block is insufficient for four segment models | **High** | Medium | Hour tracking | Triage order in §7; the logs and blob models are the priority; the JSON and prose LZ are droppable | At H63, anything unfinished is dropped, not rushed — a rushed model is a round-trip risk |
| R-E7 | Model detection misfires on the 10× probe or on unexpected content | Medium | Medium (a worse ratio, not a wrong answer) | The 10× probe's ratio and round-trip | Content-based detection with a conservative fallback to the generic model; the fallback path is tested explicitly | If detection misfires, force the generic model (a ratio loss of ~0.3 overall) and keep the round-trip |
| R-E8 | The 50× M cap is mistaken for an achievable target | Low | Low (a wasted-effort trap) | §2's analysis | The band analysis shows the M factor spans only 1.015–1.107 across the realistic ratio range | No hour is spent chasing ratio beyond r = 3.5 |
| R-E9 | Source-size overrun (S ratio at 0.45 or lower), because four models plus an entropy coder plus a container is a lot of code | **High** | Medium (−2 to −3 points) | The normalized byte count per rung | Accept it: the S loss is 0.15·ln(1/0.45) ≈ 0.12 on ln K (−2.5 points), which the T axis can more than repay; if the S ratio falls below 0.40, cut the fourth model rather than the encoder | If the S ratio < 0.40, drop a model and re-measure |
| R-E10 | Parallel block compression produces a different byte stream depending on the worker count | Low | Medium (a different hash than expected, if the expected stream is pinned) | V6 | Per-block independent models; fixed output offsets | Any difference → single-worker compression (still fast enough) |
| R-E11 | The decoder is slower than estimated (the 3/4 term) | Medium | High | D measured at H2 and after every rung | The decoder is the design centerpiece (L3); a symbol fast path and batched renormalization | If D > 1.0s at H63, cut the model's order and re-tune; the decoder's speed is worth more than any model |
| R-E12 | The 10× probe cannot be staged (1GiB of corpus on the dev volume) | Medium | Low | Probe setup at H63 | Host scratch volume; a 2× probe as a fallback | If impossible, report the structural linear-scaling argument with the per-stage measurements, in PERF.md |
| R-E13 | A reference drift >2% voids the batch, so our T ratio is fiction | Medium | Medium | Batch-local reference timings | Re-derive from the batch-local reference | Discard and re-measure |
| R-E14 | The published segment ratios or offsets differ from the brief's | Low | Low | The spec at H0 | Detect at runtime rather than assume | If the offsets differ, the header carries the real ones; the model selection follows the content |
| R-E15 | The output file name or location is wrong | Low | **Total** (a wrong artifact) | The spec at H0; a dry run writing to /out | Read the exact requirement at H0 and write a one-byte file to the right name at H0 as a smoke test | Fixed at H0 — this is not allowed to be discovered at H64 |

**Global stop rule for Track E (in priority order).**
1. **Round-trip stop:** the bit-exact round-trip is the gate. Any failure → revert to the last known-good artifact and re-verify. A correct 2.41×-equivalent submission is worth far more than a broken 3.5×.
2. **Speed stop:** if the measured T ratio is below 15× at H63, cut the model's order and the LZ path to recover speed; the ratio is worth less than the time.
3. **Scope stop:** unfinished models are dropped, never rushed. The triage order in §7 governs.
4. **Parallelism stop:** if the worker count changes the output bytes, compress single-threaded.
5. **S-axis stop:** if the normalized source exceeds ~2.3× the reference's, drop a model rather than the entropy coder (the coder is the shared core; the models are the optional layer).
6. **Freeze:** at H64 the last known-good, round-trip-verified artifact ships, with the re-anchored axis table and a PERF.md that leads with the M-axis inertness analysis and the ratio-vs-speed exchange rate.

**Definition of done for Track E.** The compressed artifact decompresses to a byte-identical copy of `corpus.bin` (SHA-256 verified) on every run; the compressed size and the overall ratio r are recorded, along with the per-segment ratios against the published reference ratios; C and D are measured separately and combined under the published rule; the 10× probe reports X_local ≤ 1.15 with an exact round-trip; PERF.md contains the per-segment table, the exchange-rate calculation, the round-trip evidence, and the failed experiments; and the artifact contains nothing but a lossless encoding of the mounted corpus.
