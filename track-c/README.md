# Track C — Join

**Parse a hostile multi-megabyte CSV — quoted commas, escaped quotes, embedded newlines, mixed line endings, a UTF-8 BOM on some chunks and not others — join it against a lookup table, and emit sorted aggregates.**

The task, the input files and the exact output format are in the
[official spec](https://github.com/hackathon-raptors/speed-demon-2026/blob/main/specs/c-join.md).

---

## Approach

This is the correctness track, and the organizers say plainly that most
submissions will fail the hash gate here. So the design is built around one
rule: **a correct scalar parser is the oracle, and nothing replaces it.**

The fast path is a candidate-byte *locator*, not a semantic shortcut. A SIMD
pass finds the next occurrence of `"` , `,` or a newline in a 32-byte window;
the scalar state machine then decides what that byte means in context. A
purely SIMD parser would have to understand quoting to skip embedded
newlines, and that is exactly where the subtle bugs live.

The other half of the problem is *where a row ends*. A newline inside a
quoted field means "nearest newline" chunking — the obvious way to split the
file for parallel parsing — will silently split records and produce wrong
answers. So a serial structural pre-scan finds true row boundaries first, and
the parallel workers only ever start on a proven boundary.

Amounts are accumulated as integer cents. A parallel `f64` reduction is not
associative, so the sum would depend on thread scheduling — which is a
correctness bug, not a rounding curiosity, and it would fail the gate four
runs out of five.

## Data layout

* **Four-state scalar machine.** `FieldStart / Unquoted / Quoted / AfterQuote`.
  Every quoted field is scanned byte by byte with a quote-run rule, so `""`
  collapses to `"` exactly once.
* **BOM handling.** A BOM is recognised *in context* (at the start of a
  field where the reference would expect one), never stripped globally — a
  global `EF BB BF` strip would corrupt a BOM that legitimately appears
  inside a quoted field.
* **Integer cents.** `i64` per row, `i128` for the aggregate. No float ever
  touches the money path.
* **No trimming.** Trailing spaces inside quotes are data, and the sample
  shows them being preserved.
* **Dense region ids.** The lookup table is interned once; rows carry a
  `u32` region id.

## Memory

The parser is a streaming state machine over a buffered reader, so peak
memory is the size of the aggregate plus the lookup table — not the size of
the file. The one exception is the structural pre-scan, which needs to know
where the row boundaries are; that is stored as a compact array of offsets,
not as a copy of the data.

`mmap` is avoided for the same reason as the other tracks: mapped pages are
charged to cgroup `memory.peak`.

## Determinism

* Row boundaries come from the structural pre-scan, never from a naive newline
  search.
* Sums are integer cents, aggregated in a fixed order — no float reduction,
  so no reassociation is possible.
* Output rows are emitted in the frozen sort order with the frozen
  comparator, and never in hash-map iteration order.
* The differential tester compares this implementation against the published
  reference byte for byte, and reports the first differing byte with its
  offset and context.

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
docker build --network=none -t speeddemon/$TEAM-C:$COMMIT track-C

docker run --rm \
  --cpuset-cpus="0-11" --cpuset-mems="0" \
  --memory=8g --memory-swap=8g \
  --pids-limit=512 --network=none --read-only \
  --tmpfs /tmp:rw,size=1g,exec \
  --ulimit nofile=8192:8192 \
  -v /srv/speeddemon/data/track-C:/data:ro \
  -v /srv/speeddemon/out/$RUN:/out:rw \
  -e TRACK=C \
  speeddemon/$TEAM-C:$COMMIT
```

The build runs `rustc` directly — no cargo, no registry, no network — so it
works under `--network=none`.

## What I would do with another 24 hours

* A second, independent parser — a deliberately naive one — kept as a
  cross-check that runs on a small sample of every batch. Two independent
  implementations agreeing is much stronger evidence than one implementation
  being careful.
* SIMD the quote-run rule itself (count consecutive quotes) rather than just
  locating the candidates.
* Replace the structural pre-scan with a single-pass resumable state machine
  that can be checkpointed at a chunk boundary, removing the need for a
  separate pass.
* Measure hash join against sort-merge on the real lookup-table cardinality
  rather than assuming, and document the result either way.

## Licence

MIT, at the repository root. The source is public at code freeze.
