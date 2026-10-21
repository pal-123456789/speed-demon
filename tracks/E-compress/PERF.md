# PERF.md — Track E (Compress)

> **The optimisation log for Track E.** The organizers weight this at
> 8% of the track total, and they are explicit about what they want:
> *"Measurements, not adjectives. The failed experiments count — they are
> often the most interesting part and judges are told to reward them."*
>
> **Honesty legend used throughout this file:**
>
> | Label | Meaning |
> |---|---|
> | `KNOWN` | measured on this machine with the harness replica in `verification/`, reproducible by anyone who runs the same command |
> | `EXPECTED` | a hypothesis. It has NOT been measured. It is not a claim. |
> | `REVERTED` | tried, measured, and thrown away |
> | `BLOCKED` | could not be attempted — pin the frozen spec first |

---

## 1. The task

Losslessly compress a fixed 100 MB mixed corpus — text, logs, JSON, and a binary blob. Scored on ratio and decompression throughput together. Compression time is measured but weighted at a quarter of decompression time.

| Axis | Reference | This submission | Ratio | Weight | Contribution |
|---|---:|---:|---:|---:|---:|
| T · wall time | 29.80 s | _(fill)_ | _(fill)_ | 0.40 | _(fill)_ |
| M · compressed size | compressed size is the M axis | _(fill)_ | _(fill)_ | 0.20 | _(fill)_ |
| S · source size | (pin from the Oct 28 spec) | _(fill)_ | _(fill)_ | 0.15 | _(fill)_ |
| D · dependencies | 1.00 | 1.00 | 1.00x | 0.15 | 1.000 |
| X · scaling | 10.40x | _(fill)_ | _(fill)_ | 0.10 | _(fill)_ |
| **K** | | | | | **_(fill)_** |
| **speed score** | | | | | **_(fill)_ / 100** |

`K = (Tref/T)^0.40 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15 x (Xref/X)^0.10`,
each ratio capped at 50x, `speed score = 100*ln(K)/ln(25)`.
Recompute with `python tooling/k_calc.py --t _ --m _ --s _ --d 1 --x _`.

On this track the M slot holds COMPRESSED SIZE, and the ratio that feeds K is (our ratio) / (the reference's 2.41x) — not the raw ratio. That distinction is worth about 18% of K, and it is the easiest mistake to make on this track.

> These numbers are **not filled in yet**, on purpose. They are only written
> down once the harness has produced them. A target is not a result.

---

## 2. The ladder

Every row below is a step that was actually taken, with the measurement that
justified keeping it. Rows marked `EXPECTED` have not been measured yet.

| # | Change | T (s) | M (MB) | vs. previous | Verdict |
|---|---|---:|---:|---:|---|
| 1 | Raw-block container + minimal rANS, exact round trip | _ | _ | _SHA-green_ | EXPECTED |
| 2 | One shared canonical decoder, per-segment dispatch | _ | _ | _the decoder is what is measured_ | EXPECTED |
| 3 | Logs transform reusing the Track A tokenizer | _ | _ | _the biggest ratio lever_ | EXPECTED |
| 4 | prose / json / blob transforms, each with a raw fallback | _ | _ | _per-segment models_ | EXPECTED |
| 5 | Parallel rANS decoder across blocks | _ | _ | _12 threads on 6 P-cores_ | EXPECTED |
| 6 | Encoder-side search improvements that do not slow the decoder | _ | _ | _cheap on the 4x-weighted axis_ | EXPECTED |

---

## 3. Experiments that failed, and were reverted

This section is deliberately the longest part of the file. Each of these was
implemented, measured, and removed.

| Experiment | Hypothesis | Result | Why it was reverted |
|---|---|---|---|
| A full PPM encoder | _ | _ | too slow to be worth it on the blended T, and it slowed the decoder as well |
| Float arithmetic on the blob segment | _ | _ | destroys NaN payloads and the sign of zero — breaks bit-exactness |
| A denser coder that decoded slower | _ | _ | loses on the axis that carries the weight |
| One model for all four segments | _ | _ | each segment rewards a different model; the blend was worse than the reference on two of them |

---

## 4. Where the time actually goes

_(fill after a profile — the flame graph, the `perf stat` counters, and the
one measurement that surprised you)_

---

## 5. Determinism

* The compressed file *is* the artefact, so it is hashed. It must be
  byte-identical across runs: fixed block order, fixed table serialisation,
  no timestamps, no uninitialised padding.
* 1 worker and 12 workers must produce the same bytes. The encoder is
  parallel per block, and blocks are written in index order.
* The float special-value matrix (NaN payloads, +0 vs -0, infinity,
  subnormals) round-trips exactly, and there is a test for each.

---

## 6. Memory

On this track the scored "memory" is the compressed output size, so the
compression-time budget trades directly against the M axis. The encoder
works block by block so the peak RAM stays at a few megabytes above the
output buffer, and the output buffer is written once.

The round trip must be bit-exact: `decode(encode(x)) == x` for every byte.
The container header is fixed — magic, version, endianness, segment table,
block table — with no timestamps, no seeds and no hash-order-dependent
serialisation, so the output file is a deterministic function of the input.

---

## 7. What I would do with another 24 hours

* A proper order-3 context model for the prose segment, with the
  match-finding cost kept entirely on the encoder side.
* Adaptive per-segment model selection: measure each candidate model on each
  segment and pick per block, which is where a real codec earns its ratio.
* A SIMD rANS decoder — the decode loop is a serial dependency per block, but
  the blocks are independent, and a wider interleaved decode across four
  blocks could hide the latency.
* Write the size/speed Pareto frontier explicitly (ratio against decode
  MB/s) and publish it; the trade-off is the subject of the track, so
  showing the frontier is worth more than a single point on it.

---

## 8. Reproducing these numbers

```bash
# 5 runs, discard the highest and lowest, mean of the middle three
pwsh -File verification/bench.ps1 -Track E -Runs 5

# the four gates: correctness, speed floor, timeout, sandbox
pwsh -File verification/verify.ps1 -Track E

# the full lockdown checklist
pwsh -File verification/pre-freeze.ps1 -Track E
```

Commit hash of this measurement: `_(fill)`.
