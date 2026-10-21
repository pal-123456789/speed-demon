# Track E — Compress

**Losslessly compress a fixed 100 MB mixed corpus — text, logs, JSON, and a binary blob. Scored on ratio and decompression throughput together. Compression time is measured but weighted at a quarter of decompression time.**

---

## Approach

Track E is the one track where the trade-off *is* the subject, and the
coefficient changes shape: the M slot is compressed size, and decompression
time is weighted roughly four times compression time. So the arithmetic says
almost everything about the design:

* **The decoder is what is measured.** Put the complexity in the encoder,
  where it is weighted at a quarter, and keep the decoder simple, branch-light
  and parallel.
* **The ratio is only worth 0.20.** The reference already achieves 2.41x. A
  ratio of 3.0x is a 1.24x on that axis — worth 1.04 in the product. A 2x
  speedup on the decoder is worth 1.32 in the product. *Time beats size on
  this track, and by a lot.*
* **One decoder for all four segments.** Four segment-specific coders means
  four chances to be slow; one canonical decoder with a per-segment
  transform in front of it means the hot path is written and tuned once.

The corpus is four published, 1 MiB-aligned segments that each reward a
different model: English prose, log lines in the Track A format, nested JSON
with repetitive keys, and float32 sensor frames. Each gets a reversible
transform in front of the shared entropy coder, and each keeps a raw-block
fallback so a segment that the transform does not help is stored as-is rather
than compressed badly.

## Data layout

* **Shared rANS decoder.** One canonical implementation for every segment.
  rANS is chosen over a range or arithmetic coder because its decoder is a
  table lookup and a refill — no bit-by-bit arithmetic, and trivially
  parallel across blocks.
* **prose** — a bounded LZ match finder feeding an order-1/2 context model.
  The encoder pays for the search; the decoder just replays it.
* **logs** — the Track A line format, so the field structure is reused:
  timestamps become deltas, latency becomes a delta, and the method and
  endpoint fields become dictionary indices. This is the biggest single ratio
  win, and it comes free from the work already done on Track A.
* **json** — a lexical tokeniser plus a key dictionary, preserving every
  escape, every space and every number's exact spelling. No AST: building
  one costs time and loses information.
* **blob** — float32 frames transposed to separate byte planes and XOR-deltaed
  **on the raw `u32` bit pattern**, never with float arithmetic. This
  preserves NaN payloads, the sign of zero, infinities and subnormals
  exactly, which float arithmetic would silently destroy.

## Memory

On this track the scored "memory" is the compressed output size, so the
compression-time budget trades directly against the M axis. The encoder
works block by block so the peak RAM stays at a few megabytes above the
output buffer, and the output buffer is written once.

The round trip must be bit-exact: `decode(encode(x)) == x` for every byte.
The container header is fixed — magic, version, endianness, segment table,
block table — with no timestamps, no seeds and no hash-order-dependent
serialisation, so the output file is a deterministic function of the input.

## Determinism

* The compressed file *is* the artefact, so it is hashed. It must be
  byte-identical across runs: fixed block order, fixed table serialisation,
  no timestamps, no uninitialised padding.
* 1 worker and 12 workers must produce the same bytes. The encoder is
  parallel per block, and blocks are written in index order.
* The float special-value matrix (NaN payloads, +0 vs -0, infinity,
  subnormals) round-trips exactly, and there is a test for each.

## Dependencies

**None.** Standard library only, no third-party crates, nothing vendored.
The dependency weight is therefore `D = 1 + 0 + 0 = 1.00`, identical to the
reference's `D_ref = 1.00` — the reference sets no precedent to lose to on
that axis, and every crate added here would be charged directly against it.

The one thing that is *not* a dependency is the Rust toolchain, which is a
build-time requirement rather than a runtime one. See `tooling/vendor-toolchain.sh`.

## Building and running

```bash
# the scored image: one image, all five tracks, dispatched on $TRACK
docker build --network=none -t speeddemon/solo:E .

docker run --rm \
  --cpuset-cpus="0-11" --cpuset-mems="0" \
  --memory=8g --memory-swap=8g \
  --pids-limit=512 --network=none --read-only \
  --tmpfs /tmp:rw,size=1g,exec \
  --ulimit nofile=8192:8192 \
  -v /srv/speeddemon/data/track-E:/data:ro \
  -v /srv/speeddemon/out/$RUN:/out:rw \
  -e TRACK=E \
  speeddemon/solo:E
```

The build runs `rustc` directly — no cargo, no registry, no network — so it
works under `--network=none`.

## What I would do with another 24 hours

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

## Licence

MIT, at the repository root. The source is public at code freeze.
