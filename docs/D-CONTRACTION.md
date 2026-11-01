# Track D: we refuted contraction with a number contraction would have changed

## The measurement that decides D

From our own `track-d/PERF.md`:

```
settles   23,227,091   ->  11,660 per query   = 0.67% of the 1,740,512 nodes
relaxes  217,505,623   -> 109,189 per query   = 2.2% of the 5,000,000 edges
```

From the 72-point entry's research note:

```
queries 300 thread-ms   5.38 M settles (bidirectional, contracted graph)
```

**5.38 M settles over 2,000 queries is 2,690 per query. We settle 11,660. That is 4.34x more,
and the difference is degree-2 chain contraction**, which they run inside their fused construction
pass and price at **88 thread-ms**:

> "Degree-2 chain contraction stays, now free inside the fused pass."
> "Candidate: exact degree-2 contraction, in place. Vertices with out-degree <= ..."

## The circular argument in our own PERF.md, corrected

Line 221 says:

> "ALT landmarks, bounded sweep — REFUTED BY MEASUREMENT — the search already settles 11,660 nodes
> per query"

**That is not evidence against preprocessing. It is evidence that we have no preprocessing.**
11,660 settles is the *uncontracted* count; 2,690 is what the same queries cost on a contracted
graph. We used our own lack of contraction as the reason not to contract.

The same reasoning appears at line 251, where a Contraction-Hierarchy build is "not attempted, on
arithmetic": that arithmetic assumed the settle count stays at 0.67%. On a contracted graph it
does not.

**This does not mean CH or ALT would pay** -- both have prep costs that may still exceed the
saving, and their own note rejects CH beyond degree 2 and ALT landmarks with numbers. **It means
the specific refutation we wrote is void, and degree-2 contraction is untested rather than
excluded.**

## Why degree-2 contraction is the right first move on D

**It attacks the phase that is probably dominant, and it is exact, not approximate.**

A vertex `u` with in-degree 1 and out-degree 1 can be bypassed: its single in-edge `(v,u,b)` and
single out-edge `(u,w,a)` are replaced by one edge `(v,w,a+b)`, and `u` disappears. Distances
between surviving vertices are unchanged, because every path through `u` is now a direct edge of
the same total weight.

**The query-endpoint problem, which is the whole difficulty:** a query may name `u`. So a vertex
must NOT be contracted if it is a query endpoint -- and there are only 2,000 queries, so at most
4,000 of 1,740,512 vertices (0.23%) are protected, and the other 99.77% are candidates.

**The ordering problem:** after contracting `u`, its neighbours' degrees change, so the pass must
be iterative or done in a fixed order with degrees recomputed. Chains are the common case and
chains are acyclic in practice, but the graph is not guaranteed acyclic, so the implementation
must not assume it -- a vertex already bypassed must not be bypassed twice, and a cycle of
degree-2 vertices must terminate.

**What it costs:** one pass over the edge array to compute in/out degree with the query endpoints
excluded, then one pass to rewrite. Their fused version charges 88 thread-ms. Our construction is
smaller, so expect less, but measure it.

**What it should buy:** if the search is ~1.07 core-seconds at 170 ms wall over 12 threads, and
settles drop 4.34x, the search phase shrinks toward a quarter. **Even at a third of that, D's T
gains far more than any of the five borrowed techniques that measured as losses here.**

## How to implement it, concretely, on our D

Our graph after the blob renumbering is `off[0..=n]` and `adj[0..m]` with edges as
`head | weight << hb`, plus `heavy2` for wide weights. Queries are mapped into the new numbering as
`ends`.

1. **Compute in-degree and out-degree** for every vertex: out from `off`, in by counting heads.
2. **Mark bypassable**: `out == 1 && in == 1 && !is_query_endpoint[v]`. Keep a counter of the
   candidates so the pass can stop early if there are none.
3. **Rewrite in one pass, no in-place mutation of the arrays being read.** For each surviving `v`,
   walk its out-edges; if the head `u` is bypassable, follow the chain forward accumulating weight
   until a non-bypassable vertex `w`, and emit `(w, accumulated)`. Cap the chain walk (a cycle
   would otherwise loop) and, on hitting the cap, emit the edge unchanged.
4. **Recompute `off` and compress**, then rebuild the reverse graph from the contracted forward
   graph, and map `ends` through the same survival test -- which is a no-op because endpoints are
   never contracted.
5. **Exactness**: the only output is distances between query endpoints, and every contracted path
   has been replaced by an edge of identical total weight. A bypassed vertex is never an endpoint,
   so no query can ask for a distance to or from one.

**Do NOT attempt the fused 4-slot row layout at the same time.** That is a separate, larger idea
and mixing them makes a failure unattributable -- which is exactly the mistake made with the
structural fast path on track C, where four changes went in at once and the 49% cost could not be
attributed to one of them.

## Method

All of the above is analysis. **The first thing to do is turn on the committed lap timers and read
D's real split**, because the claim "the search is the dominant phase" is inferred from settle and
relax counts and their per-relax cost, not measured here. If construction turns out to dominate,
the same contraction is still the right move -- it removes vertices, which shrinks every later
pass -- but the expected gain changes.

Measure with the rig in docs/MEASURING-LOCALLY.md: WSL, corpus on ext4, anon-only memory, smallest
of several runs.