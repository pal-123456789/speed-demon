# Techniques — every optimisation in this submission, and what it bought

> A complete inventory, organised by track and then by the axis each technique attacks.
> Where a technique was measured, the measurement is given. Where it was **not** measured, this
> document says so rather than implying otherwise.

---

## Cross-cutting techniques

These apply to all four tracks and were chosen once, deliberately.

### Name the compiler target; never use `native`

```
-C target-cpu=alderlake
```

The scoring CPU is a Core i5-13500 — Alder Lake: AVX2, BMI2, PCLMULQDQ, with AVX-512 fused off.
`native` resolves against *whatever machine performs the build*, which is not the machine that runs
the binary. **This was measured rather than assumed: a `native` build was 0.6% to 6.7% slower on all
four tracks.** Naming the target removes the entire class of problem, permanently.

### Compile offline, with no package manager in the scored path

Each track is a **single Rust file** compiled by `rustc` directly. No `Cargo.toml` is required for
the build, no registry is contacted, no dependency is fetched. The scored build runs with
`--network=none`, so this is not a stylistic choice — it is the only shape that always builds.

### Release pages early, in parallel, and exit without teardown

Peak memory is scored, and the cheapest way to lower it is not to hold memory in the first place:

- **Track A** hands each 16 KB record chunk back to the kernel, in one `madvise`, as soon as the
  hours that use it are finished — so the record region shrinks while the output grows.
- **Track B** drops each thread's share of the 254 MB corpus *the moment the scan has read it*, and
  unmaps the term table immediately after the scan.
- **Track C** releases each input piece's pages from the thread that read them, rather than leaving
  all of them for a single release at exit.
- **Track D** frees the old graph and the id map on a side thread while the reverse graph is built.

Every program **exits immediately after its final write**. There is no teardown to pay for.

### Treat source size as a first-class axis

At roughly 26% of the lines per point against 9% of the time, **deleting code is the highest-yield
optimisation available.** Several of the largest wins in this project were pure deletions, and each
track's `PERF.md` records its line count alongside its measurements.

---

## 🅰 Track A — Ingest

**Attack surface: the parser is 50.8% of the run** (measured, not assumed).

| Technique | Axis | What it does |
|---|---|---|
| **Fixed-width timestamp** | time, **source** | The timestamp is 27 bytes and the window is fixed at 72 hours, so the hour index is arithmetic on **four day/hour digits** — no date parsing exists in the program. |
| **One 64-byte scan per line** | time | A single AVX2 pass after the timestamp yields the method's end, the endpoint's end and the line's end together. |
| **Loop-free digit decode** | time | The latency is folded from the 16 bytes before the line end with `maddubs`/`madd` and a lane mask, in a fixed number of instructions regardless of digit count. |
| **Lock-free shared intern table** | time, memory | One table for all twelve threads; a CRC of the name's first and last eight bytes; a **two-step 64-byte vector compare** instead of `memcmp`; one compare-and-swap to publish a new name. Names are copied into a shared arena so the compared names sit together. |
| **4-byte records** | memory | `latency << 13 \| key`, where the key holds a 12-bit endpoint id and the 5xx bit. Latencies of 2¹⁹ µs or more take a side list with their key, so the packing stays exact. |
| **Branch-free selection** | time | Percentiles are exact nearest-rank on a counting sort, then three nested selections. Each partitions around a median-of-three with an **unconditional swap**, so the boundary moves by the comparison's result and never by a branch on the data. After 40 rounds the library's selection finishes. |
| **Sizes known before formatting** | time | A number's decimal width is computed without formatting it, so each hour's exact output length is known before a byte is written. |
| **Map the output, format into it** | time, memory | One thread turns sizes into offsets and maps the file; every thread then formats its own hours straight into it — **no hour waits on another and no write call queues on a lock.** |

**Measured phase split:** `parse 50.8% · hours 24.4% · text 22.0% · rank 2.8% · size+map 0.6%`

---

## 🅱 Track B — Index

**Attack surface: the corpus scan is 70% of the run** (measured).

| Technique | Axis | What it does |
|---|---|---|
| **Queries first** | time, memory | The specification permits reading the query file first, so the corpus is indexed **only for the terms the queries use** — no posting list is ever written for a term no query mentions. |
| **Adaptive term representation** | time, memory | A sample pass estimates each term's document frequency, which decides whether it is stored as a **bitmap** or an **id list**, with the threshold chosen from the data rather than fixed. |
| **`bzhi`-masked token hashing** | time, **source** | The term key is a 64-bit hash of the token's first 32 lowercased bytes, built from two 8-byte loads **masked to the token's own length**. No 32-byte vector key is constructed and **no byte comparison happens per probe** — which is what allows the `keys` array to be deleted outright. |
| **2 MB term table, unmapped after use** | time, memory | The table is mapped on its own, aligned to a 2 MB boundary, `MADV_HUGEPAGE`'d, touched once so the pages exist before huge pages are disabled for the rest of the process — then handed straight back after the scan. |
| **Branchless token ring** | time, **source** | The 8-deep ring of pending lookups **starts full of throwaway entries**, so a lookup is always issued for a full slot and the caller simply skips the first eight ids. **That removes the length branch from the hot lookup entirely.** |
| **AVX2 byte classification** | time | Letters (case-folded with one `or`) and digits are detected in the same 32-byte mask, so token boundaries come out of one comparison per half-line. |
| **Adaptive boolean evaluation** | time | Operators are applied strictly left to right, switching between id lists and bitmaps based on the operands' sizes, with the smallest operand kept for intersection. |
| **Single-term phrase rejection** | time | A phrase candidate is tested on **one non-anchor term first**, so most windows are rejected before the slice comparison is ever entered. |
| **Counts without materialising answers** | time, memory | Results are merged as counts, id sums and smallest ids — the answer lists themselves are never built. |

---

## 🅲 Track C — Join

**Attack surface: every record, and three independent random reads per record.**

| Technique | Axis | What it does |
|---|---|---|
| **Block classification** | time | Each 64-byte block becomes **quote, comma and LF masks simultaneously**. Separators outside quotes are listed into an array, and each record then consumes **five of them**. |
| **Prefix-XOR quote state via `PCLMULQDQ`** | time, **source** | "Inside quotes" is the prefix XOR of the quote mask — **one carry-less multiply by all-ones per block**, carried forward in a register. A `""` pair turns quoting off and on again with no special case, which deletes the entire quoted-field scanner. |
| **Three numbers in one vector** | time | The record's id, region and amount are decoded **together in one `__m256i`**, using `_mm256_sllv_epi64` for the per-lane leading-zero mask, then folded with `maddubs`/`madd`. Three separate decodes become one operation. |
| **Length from quote counts** | time | A text field's *value* length comes from its quote count rather than from re-scanning it: `bytes − pairs − outer quotes`. |
| **Direct-address region table** | time | Region ids map to country groups through a byte table. **A missing id reads as the unmatched group, so the hot path contains no comparison.** |
| **Huge-page shared state** | time | The bitset of names-seen-per-country and the per-thread totals live in **one 2 MB-aligned `MADV_HUGEPAGE` block**, so a record's mark lands on a huge page instead of missing the first-level TLB across 4 MB of 4 KB pages. |
| **One cache line per country** | time | `Totals` is `#[repr(align(64))]` and exactly 64 bytes, so **a record's update touches one line instead of straddling two.** |
| **Pipelined record handling** | time | Three random reads per record — the region table, the name table, the bitset word — are each issued **a fixed number of records ahead**, so all three have a steady lead and the pipeline never stalls on one. |
| **Small thread stacks** | memory | 256 KB, because an 8 MB stack can be backed by a whole 2 MB huge page once touched. |

---

## 🅳 Track D — Traverse

**Attack surface: construction is 66% of the run, search 33%** (measured).

| Technique | Axis | What it does |
|---|---|---|
| **One 32-bit edge word** | memory, time | The neighbour is in the low bits and the weight above. **Weights above the field take a marker and a side list, so weights up to 2³²−1 stay exact** while the common case stays in one word. |
| **16-bit intra-part offsets** | memory | Edges are filed under their tail's part of a 16,384-node range, so an offset inside a part needs only 16 bits and records can be packed two-per-word. |
| **Chunked, arena-backed filing** | time, memory | Rather than ~1,300 growable per-part lists to allocate, move and fault, records go into one arena per thread in 1,024-record chunks, and only chunks actually reached are ever written. |
| **No atomics in the build** | time | Each part is built independently — out-degrees counted into a 64 KB counter array and edges placed by cursor — so **the adjacency build takes no atomic operations at all.** |
| **BFS locality relabelling** | time | Ids arrive randomly permuted; the graph is cut into **512-node blobs grown breadth-first**, so consecutive new ids are physically close and a search touches a contiguous id range. |
| **4-ary heap** | time | Four children per node means **four children share one cache line**; the heap only ever holds the live frontier, so it stays small. |
| **Paged, self-sizing distance store** | memory | Distances live in pages handed out on first touch and returned after each query, so **a thread's memory follows its largest search** rather than the whole graph. |
| **Side-thread teardown** | time | The old graph, the edge store and the id map are dropped **on a side thread while the reverse graph is being built** — the free costs nothing on the critical path. |
| **Prefetch ladders** | time | Offset reads are prefetched 8 queue entries ahead and edge reads 4 ahead, and each expansion prefetches the next row it will need. |

---

## What is *not* in here, and why

**Mechanisms that were measured and rejected are not listed as techniques**, because they are not in
the submission. They are recorded instead in [JOURNEY.md](JOURNEY.md), with the measurement that
killed each one — `native`, fat LTO, grouped probing, narrow distance labels, page-drop variants,
degree-2 contraction, atomic-free transposition, and several others.

**Two known inefficiencies remain and are documented rather than hidden** — the [README's](<../README.md>)
"Honest limits" section names them, and each track's `PERF.md` carries the detail.
