# Track D — Traverse

**Traverse a 5M-edge directed weighted graph and answer 2,000 source-target shortest-path queries. Preprocessing is allowed and is counted inside the timed run.**

---

## Approach

Track D has the widest gap between a competent submission and an excellent
one, because preprocessing is explicitly legal and is counted inside the timed
run. That makes the build-versus-query trade the entire problem.

The number that decides it: the 2,000 queries run in parallel, so a
per-query saving has to be divided by the effective worker count before it
can be compared against a preprocessing cost. "2,000 queries at 18 ms each"
looks like a 36-second budget; at 12-way parallelism it is about 3 seconds
of wall time. Any preprocessing that costs more than that is a loss, however
good it is in principle.

So the plan is a portfolio with a hard rollback:

1. **Bidirectional Dijkstra on forward and reverse CSR** is the permanent
   baseline. It is always correct, and it is the thing we fall back to.
2. **ALT** (A* with landmark distances) is a bounded experiment: sweep the
   landmark count and keep it only if it beats plain bidirectional inside
   the time budget.
3. **Contraction Hierarchies are REJECTED**, on arithmetic rather than on
   nerve: the preprocessing has to be paid inside the timed run, and at the 1x
   ratio the shortcut build alone exceeds the whole query budget it is meant to
   accelerate. No abort gate makes that arithmetic work, so CH is not on the
   ladder at all. The chosen structure is CSR + a radix heap + bidirectional
   search + 6-8 ALT landmarks.

The reverse graph is a *traversal structure*, not a claimed edge: it is
built from the edge list because the format lists reverse edges separately
where they exist, and a search over it never implies a path in the original
directed graph.

## Data layout

* **CSR.** Edges are read once into a `u32` adjacency structure with `u64`
  offsets. The edge list is not sorted by tail, so the CSR is built with a
  counting pass followed by a scatter — a parallel-friendly two-phase build.
* **Both directions.** A forward CSR and a reverse CSR, for bidirectional
  search.
* **Generation stamps.** Each query stamps the nodes it has visited with a
  generation number instead of clearing a visited array, so the per-query
  setup cost is O(1) rather than O(n). With 1.7M nodes and 2,000 queries
  that difference is the whole query budget.
* **Distances.** `u64`, checked on add, so an overflow is a handled error
  rather than a wrong answer. `u32` distances are not used without a proven
  bound on the maximum shortest-path weight.

## Memory

CSR forward plus reverse is roughly 94 MB of `u32` ids and weights, and
the offsets are `u32` at `(n + 1) * 4` bytes per direction — about 7 MB each.
Landmark distances are `k * n * 4` bytes with a per-landmark offset: about
42 MB at 6 landmarks and 56 MB at 8, the range `docs/specs/track-d-traverse.md`
fixes. Per-worker scratch for a bidirectional search is about 334 MB across 12
workers. All of that fits inside the 8 GB ceiling with room to spare, which is
what makes the 6-8 landmark range worth sweeping rather than dismissing.

(These figures previously read `2 * k * n * 8` bytes, "111 MB at 4 landmarks and
891 MB at 32". That was the superseded design: `u64` distances, two copies, no
landmark cap. The spec of record fixes `u32` with per-landmark offsets and
k = 6 to 8, so the old arithmetic described a structure nobody is building.)

The memory axis has a hard cap of 50x, and the 8 GB limit is a hard kill
rather than a slowdown, so a preprocessing variant that runs out of memory
is a failed run, not a slow one.

## Determinism

* Distances are integers. No float ever enters a shortest path.
* If the output contract asks for a path rather than a distance, the tie-break
  is fixed explicitly and exercised by a diamond-graph test suite, since a
  graph with equal-length paths is where an unspecified tie-break produces a
  different answer on a different run.
* Query results are written into per-query slots and emitted in query order.
* Any preprocessing variant is validated against the bidirectional baseline
  on every graph in the fixture set; one mismatch removes it.

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
docker build --network=none -t speeddemon/solo:D .

docker run --rm \
  --cpuset-cpus="0-11" --cpuset-mems="0" \
  --memory=8g --memory-swap=8g \
  --pids-limit=512 --network=none --read-only \
  --tmpfs /tmp:rw,size=1g,exec \
  --ulimit nofile=8192:8192 \
  -v /srv/speeddemon/data/track-D:/data:ro \
  -v /srv/speeddemon/out/$RUN:/out:rw \
  -e TRACK=D \
  speeddemon/solo:D
```

The build runs `rustc` directly — no cargo, no registry, no network — so it
works under `--network=none`.

## What I would do with another 24 hours

* Hierarchical hub labelling or a transit-node routing scheme, both of which
  beat CH on road-like graphs by a wide margin — this graph's structure
  decides whether they are worth the time.
* A proper `Delta-stepping` implementation for the case where the weights
  turn out to be small and bounded, which would change the whole algorithm
  choice.
* Cache-block the CSR for the 24 MiB L3: the working set for a single search
  is much smaller than the graph, and a blocked layout could matter more than
  the search algorithm.
* Measure the preprocessing break-even empirically instead of analytically,
  by running the 10x-scaled query set and finding where CH stops paying.

## Licence

MIT, at the repository root. The source is public at code freeze.
