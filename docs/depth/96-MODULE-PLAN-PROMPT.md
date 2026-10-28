# PROMPT 01 — module decomposition and build order (for a high-level model)

Paste the block below into the high-level model. The answer comes back and is
integrated, verified and committed like any other module.

---

You are a principal performance engineer. I am competing in "Speed Demon 2026",
a 72-hour optimisation hackathon. I need a module decomposition and a build
order for four tracks, in Rust, standard library only.

**Scoring.** `K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15`,
`speed = 50·log10(K)` clamped to [0,100]; track total = `0.65·speed + 0.35·judge`;
overall = geometric mean of the best three tracks. `T` = wall time (uncapped),
`M` = peak memory from cgroup v2 `memory.peak` (capped at 50×), `S` = code lines
counted by scc over the track folder after canonical formatting, where prose and
data files are free (capped at 1.0), `D` = 1 + direct non-stdlib runtime
dependencies (capped at 1.0). I use no crates, so `D = 1.00`. **Every line of
Rust costs `S`**, so terseness has a price and must be justified by measurement.

**Correctness.** Exactly one output file with a fixed name, gated by its
SHA-256; exit 0; byte-identical across runs; the image builds offline; no network
at run time; one file to `/out`; input is read-only at `/data`.

**The four tracks** (reference implementations are naive single-threaded Python;
`T_ref`/`M_ref` measured on the scoring box):

- **A · INGEST** — 3.1M lines `ts method endpoint status latency_us` (27-byte
  RFC3339 UTC timestamps, 1,842 endpoints, eight shards written in time order
  but interleaved on arrival so the file is NOT sorted, no bound on disorder).
  Group by `(UTC hour, endpoint)`; per group emit count, p50/p95/p99 by nearest
  rank (`(p·n+99)//100`), the 5xx count, and `ppm = errs·1_000_000//count`.
  Sort by `(hour, endpoint)` and emit one line per group.
  Reference: **32.05 s / 228 MB**. Measured profile: **48.7% timestamp parsing**,
  10.5% aggregation, **2.4% line splitting**.

- **B · INDEX** — 250 MB `docs.txt` (`id<TAB>text`) plus `queries.txt`.
  Tokenise `[a-z0-9]+` lower-cased over raw bytes (any byte ≥ 0x80 separates);
  BOOL is strictly left-to-right with no precedence (`a OR b AND c` = `(a OR b)
  AND c`); PHRASE needs its terms at k consecutive token positions, with all
  separators invisible, so it may cross a sentence boundary but never a document
  boundary. Per query emit `count`, the exact sum of matching ids, and the ten
  smallest matching ids. Reference: **56.35 s / 2.1 GB**.
  Measured: 26% tokenise, 23% build, 17% phrase, 6% boolean.

- **C · JOIN** — 200 MB hostile CSV (quoted commas, escaped `""` quotes,
  embedded LF/CRLF, BOM at the file start and mid-file at record starts, mixed
  line endings, empty and quoted-empty names) plus `regions.csv`.
  Amounts are exact 2-decimal money parsed to integer cents (`i64`, no floating
  point, no rounding); join `region_id` to country, unknown → `_UNMATCHED`
  (which sorts last); per country emit count, `id_sum`, `amount_sum`,
  `amount_min`, `amount_max`, distinct name count, `name_bytes`, `note_bytes`;
  money is printed as integer cents with two decimals and never as `-0.00`.
  Reference: **14.44 s / 350 MB**. Measured: **24% amount parse**, 20%
  aggregation, 10% CSV parse, **3.7% join**.

- **D · TRAVERSE** — 1.74M nodes, 5M directed unsorted edges `u v w`, and 2,000
  queries. Distance-only Dijkstra; `-1` when unreachable; `s == t` gives 0.
  Reference: **72.69 s / 852 MB**. Measured: **97.1% search**, **0.4% edge
  parsing**.

**My hardware.** 12 usable hardware threads (6 P-cores + E-cores, AVX2 only, no
AVX-512), 8 GB RAM, no swap, 1 GB tmpfs at `/tmp`, `/data` read-only, one output
file to `/out`.

**Deliver, concisely:**

1. A module decomposition per track: name, single responsibility, input/output
   types, and the boundary (what the module must NOT do).
2. The build order across 72 hours with rough hour estimates, optimising
   score-per-hour — `T` carries 0.50 and is uncapped, `M` 0.20, while `S` and
   `D` can only cost.
3. For each module, the single acceptance test that proves it, using the naive
   Python reference as the oracle.
4. The top five risks that would zero a track (wrong output, timeout, memory
   cap, non-determinism, sandbox) and the cheapest mitigation for each.
5. What NOT to build: things that look attractive but do not pay under this
   scoring formula.

Be specific and terse. Assume an expert reader; no introductory prose.
