# Track A — Ingest

**Aggregate 3.1M structured log lines (239 MB) into per-endpoint request counts, p50/p95/p99 latency, and error rates bucketed by hour.**

The task, the input files and the exact output format are in the
[official spec](https://github.com/hackathon-raptors/speed-demon-2026/blob/main/specs/a-ingest.md).

---

## Approach

Track A is a parsing problem with two separable costs: turning bytes into
fields, and turning fields into aggregates. The reference pays for both in the
expensive way — a general-purpose split, a date-library timestamp parse, a
growing `Vec` per endpoint, and a final sort of every one of those vectors.

The fast path does the opposite of that in both directions:

* **Parsing.** The line grammar is fixed-width in the fields that cost the
  most to decode. The timestamp is parsed by arithmetic on its components
  rather than by a date library, and the endpoint is interned to a dense
  integer id so that no string comparison ever happens in the hot loop.
* **Aggregation.** Latencies are not stored and sorted. They are counted into
  a two-level radix histogram over the `u32` microsecond domain, which gives
  *exact* percentiles — the same values a full sort would produce — without
  materialising 3.1M values.

The two are combined by scanning the input in parallel over byte ranges that
are split only at true line boundaries. Each worker accumulates into private
state; the merge is performed afterwards in worker-index order, so the output
is byte-identical regardless of how the OS scheduled the threads.

## Data layout

* **Dense ids.** `endpoint -> epid` is interned once; the hot loop compares and
  hashes `u32` ids, never strings. The Zipf tail is handled by the same path
  as everything else — a small intern table is faster than any special-casing.
* **SoA accumulators.** Count, error count, and the histogram live in
  separate arrays indexed by the group id, so the merge touches contiguous
  memory instead of chasing a struct per group.
* **Two-level radix histogram.** A `u32` latency is split into a 16-bit high
  part and a 16-bit low part. The first pass counts the high parts; the
  percentiles are then resolved by scanning only the relevant high buckets.
  This is exact, and it never sorts.
* **No per-record allocation.** Endpoints are interned into a bump arena; the
  histogram is allocated once per group and reused.

## Memory

The M axis is won by *not retaining* anything. The reference holds every
latency in a growing list (that is where its 2,940 MB goes); this
implementation holds a fixed-size histogram per group, so peak memory is a
function of the number of distinct groups rather than the number of records.

Input is read with a buffered streaming reader rather than `mmap`: pages of a
memory-mapped file are charged to cgroup `memory.peak`, which would inflate
the M axis with data we never need resident.

## Determinism

* Endpoint ids are assigned by a deterministic sorted rule, never by hash-map
  iteration order.
* The group key is `epid * hour_count + hour`, so the output order is fully
  determined by the ids.
* Percentiles are computed from integer counts — no float ever enters the
  ranking, so no rounding mode can change the answer.
* Error rates are integer counts formatted by an explicit routine.
* The parallel merge runs in worker-index order, so 1 worker and 12 workers
  produce byte-identical output.

## Dependencies

**None.** Standard library only, no third-party crates, nothing vendored.
The dependency weight is therefore `D = 1 + 0 + 0 = 1.00`, identical to the
reference's `D_ref = 1.00` — the reference sets no precedent to lose to on
that axis, and every crate added here would be charged directly against it.

The one thing that is *not* a dependency is the Rust toolchain, which is a
build-time requirement rather than a runtime one. The build stage uses the
digest-pinned, allowlisted `rust:1-slim-trixie` image, which ships the
toolchain, so nothing needs to be vendored.

## Building and running

```bash
# the scored image is built PER TRACK, from the track folder as context
docker build --network=none -t speeddemon/$TEAM-A:$COMMIT track-A

docker run --rm \
  --cpuset-cpus="0-11" --cpuset-mems="0" \
  --memory=8g --memory-swap=8g \
  --pids-limit=512 --network=none --read-only \
  --tmpfs /tmp:rw,size=1g,exec \
  --ulimit nofile=8192:8192 \
  -v /srv/speeddemon/data/track-A:/data:ro \
  -v /srv/speeddemon/out/$RUN:/out:rw \
  -e TRACK=A \
  speeddemon/$TEAM-A:$COMMIT
```

The build runs `rustc` directly — no cargo, no registry, no network — so it
works under `--network=none`.

## What I would do with another 24 hours

* Replace the scalar scanner with an AVX2 `vpcmpeqb`/`vpmovmskb` candidate
  locator feeding a branchless field-state machine, with a scalar fallback
  selected by a single runtime CPU check.
* Replace the two-level histogram with a three-level one so the inner scan
  touches fewer cache lines for skewed latency distributions.
* Cache-block the merge: the current merge walks the group array twice, and
  for 1,842 endpoints x 24 hours that array should sit entirely in L2.
* Add a `perf`-driven pass over the merge, which has never been the
  bottleneck and is therefore the one place a surprise is still possible.

## Licence

MIT, at the repository root. The source is public at code freeze.
