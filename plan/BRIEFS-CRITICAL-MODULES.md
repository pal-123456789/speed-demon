# BRIEFS — the critical modules, at implementation-grade design level

> Design material for the four modules where a wrong decision costs a whole
> track. Everything here is grounded in the official specs, the vendored
> reference and the oracle — not inference. Written before kickoff (designing is
> explicitly allowed); the Rust lands in `track-*/src` only after H0.
>
> **Line budgets are measured with our scc-aligned counter.** S is only paid
> above the reference line count (A 56 · B 91 · C 108 · D 43), so A and D have
> almost no headroom and B/C have one.

---

## A · `line` + `groups` + `rank` (the 48.7%)

**Contract.** A line is exactly
`ts SP method SP endpoint SP status SP latency LF`, with `ts` 27 bytes
(`YYYY-MM-DDTHH:MM:SS.ffffffZ`), `endpoint` 1..125 bytes from
`A-Za-z0-9/-_.{}:` starting with `/`, `status` three digits 100..599,
`latency` decimal µs 0..4294967295 with no leading zeros. Every line ends LF,
including the last; ASCII; no CR, no empty lines.

**The 13-byte key.** The hour bucket is `ts[0..13]` = `YYYY-MM-DDTHH`, emitted as
`YYYY-MM-DDTHH:00:00Z`. Never parse the calendar: the reference spends 48.7% of
its run in `strptime`, and all of that disappears here. µs is looked at only as
an integer (it is never emitted), so `SS.ffffff` needs no arithmetic.

**Pipeline.**

1. `slab`: `pread` fixed 1 MiB reads, extended to the next LF so slabs never
   split a line; the tail slab carries a virtual LF. Slab boundaries must be
   invisible: the acceptance test is that 64 B and 1 MiB slabs give identical
   output.
2. `line`: one forward scan per line, splitting on single spaces. Endpoint
   pointer/length only (no copy, no interning per line — interning happens in
   `groups`). `5xx` is `status[0] == b'5'` **and** the status is exactly three
   digits (the grammar guarantees it; do not "fix" statuses outside 100..599
   beyond treating them as non-5xx). Latency: accumulate `v = v*10 + (c-48)`
   into u32.
3. `groups`: per-thread open-addressed table keyed by `(hour, endpoint bytes)`
   with a byte-compare on collision (hash 64-bit, compare full key — a hash-only
   match is a correctness bug). Slot: `{count: u32, errs: u32, lat_ptr: u32, lat_len: u32}`
   with latencies appended to bump-chunked per-slot arenas (chunk = 64 KiB), so
   no per-group `Vec` and no reallocation inside the loop.
4. `rank`: nearest rank `k = (p*n + 99) / 100` (integer), then
   `select_nth_unstable` at `k-1`; compute p99 first and reuse the partition for
   p95/p50 (narrowing), or sort in place for `n <= 16` where selection overhead
   loses. `k` is ≥ 1 for every n ≥ 1 and p ≥ 1.
5. `emit`: merge per-thread tables, sort groups by `(hour, endpoint)` with hour
   compared as 13 raw bytes and endpoint bytewise, then write each line into one
   large output buffer and a single `write`. `ppm = errs as u64 * 1_000_000 / count`
   with integer division.

**Edge cases that must be exercised** (all appear in the tiny corpus):
single-request group; a group of 2; all latencies equal; latency `0`; latency
`4294967295`; every endpoint from the 13 fixed ones including the 125-byte
longest; statuses across all five classes; the hour at the span start
(`2026-10-30T18:00`) and end, across the October→November boundary; two
midnights; the file's first and last line; a group whose only request is a 5xx
(so `ppm = 1_000_000`).

**Pitfalls.** Do not rely on order (no bound on disorder — the eight shards are
interleaved arbitrarily). Do not emit a group with zero requests. Do not write
a trailing empty line. Do not compare hours as strings with a separator
difference (`T13` vs `T14` must sort correctly bytewise — it does).

**Budget.** Reference S = 56. Target ≤ 450 code lines for all five modules
including the parallel driver; every line above 56 must be justified by the
`--tradeoff` test (T gain ≥ S ratio^0.3).

---

## B · `tok` + `phrase` + `eval` (build side and phrase)

**Contract.** `docs.txt` is `id LF? text`, id 0..N-1 in ascending file order
(do not assume; renumber by ordinal only if ids are not ascending, and "smallest
ids" means smallest **id**). Tokens are maximal runs of `[a-z0-9]` over the
**bytes**, with every byte outside that set — including all bytes ≥ 0x80 —
acting as a separator. Lower-casing is **ASCII only**: `Zürich` (UTF-8 `Z C3 BC
…`) becomes `z` and `rich`, because `C3` and `BC` are separators. A Python
`str.lower()` view would differ; the reference works on bytes.

**`tok`.** 256-byte class table: `b'A'..=b'Z'` → `+32`, `b'a'..=b'z'` and
`b'0'..=b'9'` → identity, everything else → 0 (separator). One pass per document,
emitting `(start, len)` token spans; a rolling 64-bit hash identifies the term,
with a byte-exact comparison against a term arena on collision. Never allocate a
`String` per token; never tokenise the id field.

**Build.** Only the terms the queries mention need postings ("indexing only the
terms the queries use is a legitimate strategy" — the spec says this
explicitly). Intern query terms first into a hash set; a document's tokens are
then filtered by that set into per-term posting vectors (`u32` ids, appended in
document order → each posting list is already ascending). Documents are numbered
by ordinal; keep a map to the file id if they differ.

**`phrase`.** A phrase matches when its terms occupy k **consecutive token
positions**, with all separators invisible (space, `. `, hyphen, em dash,
no-break space, a quoted `""` sequence — anything). Two consequences: a phrase
may cross a sentence boundary, and it must **never** cross a document boundary.
Implementation: for each phrase term, keep its positions; match with a k-cursor
(`next_pos = max(pos, prev+1)`) or, when the shortest list is long, walk the
shortest list and test the others by binary search. Repeats must repeat for real
(`"x x x"` needs three consecutive `x`). A phrase whose term is absent matches
nothing. Upgrading to an Aho–Corasick automaton over term ids is optional and
only worth it once `phrase` is a measured share.

**`eval`.** Strict left-to-right fold with no precedence: `R = R ∩ S(t)` for
AND, `∪` for OR, `\` for NOT (binary). A single-term query is `S(t1)`. Evaluate
over ascending `u32` vectors: gallop (`exponential search`) when one side is
much shorter, plain merge otherwise; union can defer the sort by concatenating
two sorted runs and merging them. `NOT t` means "documents of R that do not
contain t" — which is `R` minus a sorted list, never "everything not in t".

**Output.** `count`, then the **exact sum of matching ids** (u64; at full scale
≤ N(N-1)/2 < 2^35), then `min(10, count)` smallest ids ascending. Empty ⇒
`0 0`. One line per query, same order as `queries.txt`, LF-terminated.

**Edge cases:** empty document; a document of only separators (`§§ ¶`, `* * *`);
`007` is one token and does not match `7`; `Case No. 2026-cv-00123` yields `case
no 2026 cv 00123`; `U.S.C.` → `u s c`; a punctuation-joined word (`habeas-corpus`
→ `habeas corpus`); a term at the very start/end of a document; a phrase of
length 1; a phrase repeated 4 times; a query referencing a term that occurs
nowhere; `NOT` as the first operator is impossible by grammar (queries always
start with a term).

**Budget.** Reference S = 91; target ≤ 700 code lines.

---

## C · `parity` + `records` + `cents` (the hostile CSV)

**Contract.** CSV dialect = Python `csv` with the excel dialect: fields separated
by `,`, quoted with `"`, an embedded `"` written `""`, records terminated by
LF/CRLF/CR — including **inside quoted fields**, where the terminator is data.
Every record has exactly 5 fields: `id, name, note, region_id, amount`. A UTF-8
BOM (`EF BB BF`) must be stripped when it appears at the start of a **record**
(not only at file start): the worked example has one after an LF, before record
5.

**`parity` (quote state at chunk boundaries, exact).** Count the number of `"`
bytes in a chunk, mod 2. The chunk's starting quote-state is the XOR of the
parity of all preceding chunks. This is exact because a `""` pair inside a
quoted field toggles the state twice (net zero) and every other `"` toggles once;
the only state that matters at a chunk boundary is "inside quotes" — and by
construction a boundary can fall anywhere, including between the two quotes of an
escaped pair, which the parity method handles because it counts raw bytes. A
chunk may therefore start mid-record or mid-field.

**`records`.** Walk with the chunk's initial state; a record ends at an unquoted
LF/CRLF/CR. Field extraction keeps `(start, len, quoted)` slices into the slab —
no per-field allocation. `""` inside a quoted field decodes to one `"` lazily
(only names/notes are ever materialised, and only for aggregation) — but note
that `name_bytes`/`note_bytes` are the byte lengths of the **decoded** value
(so `""` counts as 1 byte, and an embedded CRLF inside a quoted note counts 2).

**`cents`.** Parse `-?\d+\.\d\d` to `i64` cents by integer arithmetic
(`v = v*10 + digit` over the integer part, then two fraction digits), with no
float anywhere. The reference does not round: the grammar guarantees exactly two
decimals. `-0.00` must normalise to `0`.

**`agg` per country:** `count`, `id_sum` (i64), `amount_sum` (i64 cents),
`amount_min`, `amount_max`, `names_distinct` (compare name bytes exactly; a
cheap 128-bit hash + sort/dedup is acceptable only if collisions are
impossible-enough — prefer exact byte comparison on the dedup, since a wrong
count is a zero), `name_bytes`, `note_bytes`. Unknown region → `_UNMATCHED`.
**Ordering is bytewise on the key**, which places `_UNMATCHED` last because
`_` (0x5F) > `Z` (0x5A) — no special case needed, and none should be added.

**Money printing:** `-` if negative, integer part with no leading zeros (`0`
when |v| < 1.00), `.`, exactly two digits: `-525 → -5.25`, `-5 → -0.05`,
`0 → 0.00` (never `-0.00`), `142000 → 1420.00`.

**Edge cases (all in the corpus and the worked example):** quoted comma in a
name; `""` twice in one field; a note spanning two lines; a CRLF inside a quoted
field; a mid-file BOM; a trailing space in a name (`Betts ` ≠ `Betts`); empty
unquoted name vs quoted-empty `""` (same value → one distinct); a region id of
`4000000` (absent from regions.csv); a negative amount; `-0.75`; a duplicate
region id in regions.csv.

**Budget.** Reference S = 108; target ≤ 800 code lines.

---

## D · `csr` + `search` + `drive` (the 97.1%)

**Contract.** `graph.txt`: first line `n m`, then m lines `u v w`: u,v are u32
with `0 <= u,v < n`, w is a non-negative weight; edges are in **no order** (not
sorted by u, v, or w) and may repeat (parallel edges: the minimum wins), may be
self-loops, and may have weight 0. `queries.txt`: 2,000 lines `s t`, no header;
`s == t` gives distance 0; unreachable gives `-1`. Output: one line per query in
order, plain integers.

**`csr`.** Two passes over the mapped file (or slabs): pass 1 counts out-degrees
into an `AtomicU32` array (one relaxed `fetch_add` per edge — contention is
irrelevant at 5M edges but the atomics let the passes be parallel), prefix-sum to
`off[n+1]`, pass 2 scatters with `fetch_sub` from the end of each range (keeps
each row's insertion order unspecified — harmless, since the minimum is taken at
relaxation time). Store `edge[i] = (w << 32) | v` to halve the memory traffic.
Also build the transpose (reverse edges) for the bidirectional phase: it is a
second CSR of ~47 MB at full scale, and it must be built, not guessed.

**`search`.** Start with uni-directional Dijkstra on the CSR using a std
`BinaryHeap` as the baseline (a monotone radix heap is the first upgrade: u64
items `key << 32 | node`, monotone buckets, no decrease-key). Then bidirectional:
alternate forward/backward expansion, maintain `µ`, stop when `tf + tb >= µ`,
update `µ` **unconditionally** on every edge relaxation, and treat an empty heap
as `+∞` on that side. Distance labels are u64 by default; `u32` labels are
provably safe when `(n-1) * wmax < 2^32` (`wmax <= 2468` at full scale) — decide
from the parsed `wmax`, do not template on it. Scratch: a per-thread distance
table plus a "touched" list to reset only what a query wrote (never `memset` the
whole table per query), and a per-thread `BinaryHeap` reused across queries.

**`drive`.** Queries are independent: an atomic counter hands out the next query
index, each worker writes `ans[qi]` into a preallocated array, and the output is
written in index order at the end (never in completion order). Repeated queries
must be answered independently (a cache is a determinism risk, not a win).

**Edge cases (all covered by the organizers' own D tests):** parallel edges where
the lighter one is listed first *and* last; three-way and exactly-duplicate
parallel edges; a zero-weight cycle; a self-loop; a source with no out-edges; a
target with no in-edges; `s == t`; an unreachable pair; a chain of maximum
weights summing within int64 (never overflow to negative); the transpose
mattering (direction matters: `u→v` does not imply `v→u`).

**Budget.** Reference S = 43 — the tightest track. Target ≤ 420 code lines
including the driver; justify every line above 43.

---

## The one acceptance command per module

| Module set | Command |
|---|---|
| A all | `python tooling/spec-cases.py --run --build --track a` then `python tooling/conformance.py --track a --diff` |
| B all | same with `--track b` (the tiny corpus covers every documented B edge case) |
| C all | same with `--track c` (the worked example is in the corpus) |
| D all | same with `--track d` |
| Any | `.\tooling\run.ps1 determinism-check.sh all` (36 configs × 3 runs) and `.\tooling\run.ps1 ci` |

---

# CORRECTIONS — from the hostile review (accepted 20 of 21, before any code)

Every item below was verified against the specs and the oracle before being
accepted. These are the bugs that would each have cost a track's SHA gate.

## Correctness (accepted, must be implemented)

| # | Failure it found | The fix now in the design |
|---|---|---|
| **A1** | Percentile narrowing: for n = 17–19, `k95 == k99`, so selecting inside `v[..k99-1]` is out of range (panics, and n ≤ 16 never hits it because those are fully sorted). | Narrow **inclusively** (`v[..k99]`) and reuse the p99 value when `k95 == k99`. The acceptance test sweeps **n = 1..200** against `sorted[(p*n+99)/100 - 1]`. |
| **A2** | A missing final LF would drop or mis-own the last line. | The tail slab carries a **virtual LF**; EOF is a terminator. (CR handling stays defensive only — the A spec guarantees LF-only, no CR.) |
| **B1** | "Ascending u32 postings" assumed sorted unique ids; gallop/merge and "ten smallest" are silently wrong otherwise. | **Assert ascending + unique** during the build (one compare per document); rank-map if it ever fails. |
| **B2** | Global token positions could let a phrase span the end of doc *i* and the start of doc *i+1*. | Positions are **document-relative** and the cursor is reset at every document start. |
| **B3** | Counting occurrences instead of documents (`go go go` matched by `go go` twice); repeated terms need a cursor **per slot**. | **Break after the first match per document**; one cursor per phrase slot, offset by slot index (`pos[slot] >= prev + slot`). |
| **B4** | "Concatenate then merge" **double-counts** documents present in both operands. | Union is a **dedup-merge** on equality (equality implies the same id, so drop the duplicate); `NOT` is a set **difference**. |
| **B5** | **Query text must go through the same tokeniser.** `PHRASE "visit Zürich"` is *three* tokens (`visit … z … rich`), not two. | Tokenise every query term/phrase with the document tokeniser; **k = the token count**, and a term that yields zero tokens can never match. |
| **C1** | Parity gives the quote state, **not** whether an offset is a record start; naive "skip to the next record start" loses a record. | Offset `s` is a record start **iff `buf[s-1] == '\n'` and the state is even**; a record belongs to the chunk holding its **first byte**. |
| **C2** | Parity is exact only if every `"` is structural — a bare `"` in an unquoted field (`5" nail`) or text after a closing quote flips the state for the rest of the file. | **Validate**: the walker's end state must equal the next chunk's predicted start state. On mismatch, **re-parse that chunk serially** and continue. Exactness becomes verified, not assumed. |
| **C3** | Comparing raw `(start,len,quoted)` slices counts `Bob` and `"Bob"` as two names, `,,` and `,"",` as two empties, and inflates `name_bytes` by the quotes. | Distinctness and `name_bytes`/`note_bytes` use the **decoded content**: the slice minus the outer quotes with `""` collapsed to `"`. Exact byte comparison on content — no hashing on the correctness path. |
| **C4** | For `|cents| < 100`, truncating division loses the sign: −5 prints `0.05`. | `neg = c < 0; u = c.unsigned_abs();` print the sign, then `u/100`, `.`, `u%100` as exactly two digits. |
| **C5** | A quoted numeric (`"12.34"`) keeps its quotes in the slice; a CRLF file leaves `\r` on the last field. | Parse **over the content**; terminator is CRLF/LF and a CR inside quotes stays data. |
| **C6** | `id_sum`/`amount_sum` can wrap in 64 bits where the reference uses bigints. | **`u128`/`i128` accumulators** (free in registers; removes the class). |
| **D1** | With `INF = u32::MAX`, `du + w + other[v]` **wraps** to a tiny µ, so µ becomes wrong; `tf + tb` with an empty heap treated as ∞ wraps too. | **No INF in arithmetic**: `INF = u64::MAX/4`, `saturating_add`, and an **explicit break when either heap is empty**. |
| **D2** | `s == t` with a cycle through `s` returned the cycle's weight instead of 0. | **Return 0 early** when `s == t`. |
| **D3** | `key << 32 | node` needs key < 2^32, but µ can reach roughly **twice** the one-direction bound `(n−1)·wmax`. | Pack a **21-bit node** (`n ≤ 2^21` at 1.74M), which allows keys to 2^43; **all sums in u64**. |
| **D4** | Touching only *popped* nodes leaves relaxed-but-never-popped labels stale into the next query — wrong answers that vary with thread count. | Push to the touched list on the **first label write** (`INF → finite`), in **both** directions; a debug build asserts both tables are all-INF after every query. |

## Structural changes (accepted)

1. **A: drop the per-slot arenas.** Use one per-thread flat `Vec<u64>` of
   `gid(31) | err(1) | lat(32)` appended per line, then a **counting sort by
   gid** after remapping thread-local endpoint ids. This removes the arena
   memory blowup (a 4 KiB page per tiny group, ~44k groups ⇒ hundreds of MB,
   potentially worse than the reference's 228 MB), removes the non-contiguous
   select (percentiles now run on **contiguous slices**), and deletes the arena
   code. Largest single win in the review.
2. **D: lazy labels.** Store `dist + 1` with **0 meaning unset**, so the
   `alloc_zeroed` pages stay untouched until written (a `vec![INF; n]` fill
   faults every page eagerly: 12 threads × 2 directions × 1.74M × 8 B ≈ 334 MB).
   Combined with the touched list, residency tracks work actually done.
3. **Measure before upgrading.** The radix heap, the u64-label path and the
   transpose CSR are **not built until a measurement shows the win**: count pops
   per query on the std-`BinaryHeap` baseline and require bidirectional to beat
   it by ≥2×. This removes D1–D3 risk and ~50 MB until it is earned.

## The one finding I did not accept as a defect

**A2's CRLF case** is outside Track A's contract — the spec states every line
ends with a single LF and there is no CR — so it is not a correctness risk. The
EOF-as-terminator half is adopted anyway because the tail slab needs it.
