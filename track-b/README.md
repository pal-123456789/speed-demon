# Track B — Index

**Build an inverted index over a 250 MB text corpus, then answer 50,000 boolean and phrase queries. Build time and query time are measured together, as one total wall time. There is no build/query split any more.**

---

## Approach

Track B is two optimisation problems that pull against each other, and the
scoring formula rewards the one you spend your hours on. The build/query
split is gone. The whole program is one wall time, so the build and the
queries are simply both part of the number you are measured on. A weighted
*time* blend
(`1 / (0.4/rB + 0.6/rQ)`) is always at most an additive *ratio* blend
(`0.4*rB + 0.6*rQ`), and it is dominated by the slower phase.

That is the whole argument for spending time on the build. Building an
index is memory-bound — tokenise, sort, encode — and the reference's naive
build is where the bigger constant is. The query side is then made fast
enough that it stops being the limiter:

* **Build.** Tokenise in parallel over document ranges, merge the dictionaries
  deterministically in byte order, and encode postings in a blocked delta
  format.
* **Boolean queries.** Evaluate in the frozen left-associative order, with
  the smallest posting list intersected first.
* **Phrase queries.** Anchor on the rarest term in the phrase and verify
  adjacency positionally.

## Data layout

* **Dictionary.** Terms are interned to dense `u32` ids; the dictionary is merged
  in byte-sorted order so the term ids are a deterministic function of the
  corpus.
* **Postings.** Delta-encoded gaps in blocks, with a per-block base and
  exception list. StreamVByte and PForDelta are compared head to head on
  this corpus rather than assumed.
* **Positions.** Positional postings are kept for every indexed term. Keeping
  positions only for the terms that appear in the visible sample would be
  overfitting to a query set we have not been given, and the holdout dataset
  uses a seed we have never seen.
* **Stopwords are retained.** The spec says stopwords are retained, and
  dropping them changes phrase-match results.

## Memory

Positions are where the memory goes. The build is streamed rather than
materialised: postings are encoded block by block and written into a
pre-sized arena, so the peak is the size of the final index rather than the
size of the intermediate token list.

Input is read with a buffered streaming reader rather than `mmap`, for the
same reason as Track A: mapped pages are charged to cgroup `memory.peak`.

## Determinism

* Term ids come from a byte-sorted dictionary merge — not hash-map iteration.
* Query results are written into per-query slots and emitted in query-file
  order, never in completion order.
* The boolean evaluator reproduces the frozen left-associative, no-parentheses
  semantics exactly, including how `NOT` binds. Conventional precedence
  (AND before OR) is *not* what the reference does.
* Phrase matching is positional and exact, on the same tokenisation the
  reference uses.
* 1 worker and 12 workers produce byte-identical output.

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
docker build --network=none -t speeddemon/$TEAM-B:$COMMIT track-B

docker run --rm \
  --cpuset-cpus="0-11" --cpuset-mems="0" \
  --memory=8g --memory-swap=8g \
  --pids-limit=512 --network=none --read-only \
  --tmpfs /tmp:rw,size=1g,exec \
  --ulimit nofile=8192:8192 \
  -v /srv/speeddemon/data/track-B:/data:ro \
  -v /srv/speeddemon/out/$RUN:/out:rw \
  -e TRACK=B \
  speeddemon/$TEAM-B:$COMMIT
```

The build runs `rustc` directly — no cargo, no registry, no network — so it
works under `--network=none`.

## What I would do with another 24 hours

* Try a skip-pointer variant on the posting lists and measure whether the
  extra index bytes pay for themselves on this corpus size — at 250 MB the
  answer may genuinely be no, and saying so with a measurement is worth more
  than the change.
* Build a small SIMD galloping intersect for the two-list case.
* Try a `WAND`/`MaxScore`-style upper-bound pruning for the boolean
  conjuncts, which is where a real engine gets its next order of magnitude.
* Revisit the build/query split with a real harmonic measurement rather than
  an estimate, and re-target the remaining hours at whichever phase the
  measurement says is actually limiting.

## Licence

MIT, at the repository root. The source is public at code freeze.
