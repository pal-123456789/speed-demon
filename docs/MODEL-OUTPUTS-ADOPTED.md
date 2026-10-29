# MODEL OUTPUTS — triaged and adopted (P-B, P-C, P-D)

> Three answers, reviewed and decided. P-B's ladder priorities are gated on
> measurement (never adopted blind); P-C's document structure is adopted as the
> standing template for every track's PERF.md and the root README; P-D's video
> pack is adopted with two corrections (a duplicated phrase in the narration and
> a capture sequence adapted to what we can actually run).

---

# 1 · P-B — the ladder, gated on measurement

**Pricing rules adopted** (they match our own `k_whatif --tradeoff`): a change
adding ΔS lines to a track of S costs `0.15·ΔS/S` in ln K; a time saving of
fraction g gains `≈0.5·g`; so an addition pays only if `g ≥ 0.3·ΔS/S`. Lines are
cheap — **risk is the real cost, so rank by risk**. Exchange rates: 2× T = +7.5
speed; 2× M = +3.0 (= 1.32× T); M×1.25 = +0.97 (= 1.09× T).

**Step 0 for every track (zero lines, do it first):** sweep threads (6/9/12 —
SMT may help or hurt), slab size and read-buffer size; test `lto="fat"`,
`codegen-units=1` (already on), `panic="abort"` (already on); add per-phase
`Instant` timers and delete them before the freeze. **Diagnose memory at exit by
printing `anon` and `file` from `memory.stat`** — if file-backed pages dominate,
the readers are what the cgroup charges; if anon dominates, it is our buffers.

| Track | Rung order adopted (each with its decide-if) |
|---|---|
| **A** | 1. Selection not sort (**already designed**; take if the percentile phase > 8% of T). 2. **One cursor per line** — parse ts/method/endpoint/status/latency in a single scan, no separate `\n` search (10–20% of the scan; net 0 to −10 lines; take if the scan > 50% of T and > ~60 cycles/line). 3. AVX2 field-boundary search (~40 lines) **only if the scan is still > 55% of T after rung 2**. |
| **B** | 1. Aho–Corasick over term ids replacing positions (net ~0 lines, medium risk — take if positions > 40% of M, or phrase+position build > 20% of T). 2. **Never materialise the result set** — stream into (count, Σid, first-ten); ~10 lines, near-zero risk, take if allocation/copy > 10% of the query phase. 3. Planner: smallest-df first inside AND/NOT runs, distributed over the OR chain; bitmap tier at df ≥ D/16 (~40 lines, medium risk). |
| **C** | 1. **Distinct-name dedupe**: bucket by 64-bit hash, then exact-compare inside the bucket (8 B/row instead of 16; collision-proof because the compare is exact). Take if the distinct phase > 15% of T or its buffers > 40% of M. 2. SWAR special-byte skip for the next `"`/`,`/CR/LF (~25 lines; take if parse > 40% of T). 3. SWAR amount parse (~12 lines, low risk, ~5% T). |
| **D** | 1. **Verify no per-query re-init** (a 7 MB memset × 2000 queries ≈ 1.4 s; our touched-list design already avoids it — prove it with a counter). 2. Bidirectional, taken only if `(B+Q) ≥ 1.2·(B+Tr+Q/s)` (usually holds when Q ≳ 3B and s ≥ 3; if the edge multiset is symmetric, skip the transpose entirely). 3. **Pack `dist<<32|node` into one u64 first** (one line, 10–20% of search) before the radix heap (~30 lines, take if heap ops > 25% of search). |

**Explicitly will not pay** (adopted as prohibitions): A — `mmap`
`MAP_POPULATE`/hugepages (≈15 unsafe lines for page-fault cost that `pread` into
reused buffers never incurs, and it can *raise* `memory.peak`). B — compressed
postings (varint/PFor/Elias–Fano): a 20 MB saving is under 1 point of M for
decode cost in the hot path. C — speculative parsing to skip the parity pre-pass
(≈3 ms across threads; a wrong guess re-parses a chunk). D — contraction
hierarchies/ALT (preprocessing exceeds 2000 queries) and AVX2 over adjacency rows
(mean degree 2.9 < 8 lanes, no scatter in AVX2).

**Memory cheaper than time, where it applies:** A — stream via `pread` into ~12
per-thread buffers (~30 MB) instead of holding ~290 MB (worth ~+10 points ≈ 2.5×
T). D — dense per-thread dist arrays are 7 MB × 12 ≈ 84 MB; the touched-list
design already sidesteps the memset cost, so revisit only if the peak demands it.

---

# 2 · P-C — the judge-facing template (adopted as the standing structure)

Judging weight inside a track: **code quality 15, technique 12, PERF.md 8** (of
100 track points). So **PERF.md must carry the technique evidence too**.

**PERF.md (root, ~4 pages, tracks as sections).** Every number reproducible from
a commit hash.

1. **Summary table** (one screen): per track T, M, S, D, K, speed — labelled
   *"scoring-box protocol, commit abc1234, date"*, with T/M ratios against the
   organizers' reference numbers and whose measurement each is.
2. **Methodology** (once, then referenced): machine + docker flags; the protocol
   (5 runs, drop fastest and slowest, mean of the middle 3, warm cache); how
   `memory.peak` is read; the scc command and version for S; the noise level as
   run-to-run spread; and the single reproduction command.
3. **Per track, fixed order:** where the reference spends its time (the measured
   profile) → design in ~5 lines, each tied to a profile line → **optimization
   ladder table** (Step | Commit | T mid-3 | Δ | M peak | mechanism and evidence)
   → phase breakdown of final T → **floor analysis** (bandwidth/I-O lower bound
   from measured bandwidth, and our % of floor) → **negative results** with
   numbers → **formula accounting** (the K factors and the exchange-rate reasoning
   for each trade) → assumptions and limits.
4. **Residual risk**: what was not verified and what could still be wrong.

**Truthful before/after:** same machine, protocol and input; alternate old/new
binaries within one session so drift cancels; absolute numbers *and* ratios;
two significant figures; no "up to", no best-of-N; state thread-count or input
changes next to the number; treat deltas inside the noise as "no measurable
effect" and keep such a step only if it simplifies the code.

**README (5-minute read):** status table (track · SHA matches published hash ✔ ·
T · M · link) → **one command** (`make verify` from a clean checkout, offline,
comparing hashes) → repo map with the per-track module list and each module's
"must not" → reading guide per track (entry function, hot file, invariants, read
this first) → **decision log** (~6 entries: decision, alternative, measured
reason) → safety/determinism in one paragraph plus an **unsafe inventory**
("zero unsafe", or a table of each block with its soundness argument) →
verification summary linking VERIFY.md → known limits.

**Make the repo match the README:** modules readable in a screen or two; comments
stating invariants and units (not narrating the next line); fmt and clippy clean;
tests next to the code; a `main()` that reads like a table of contents.

**Ten things most teams omit** (all adopted as to-dos): clean-checkout offline
repro with expected hashes · negative-results log · floor analysis ·
counter evidence (`perf stat`) for the two changes that mattered · determinism
stated as an invariant *and* tested by sweeps · **assumption ledger** (each input
assumption, whether checked at runtime, what happens on violation — prefer
failing loudly) · **unsafe inventory** · formula trade-off accounting ·
**edge-case traceability matrix** (documented case → fixture → test → code
location) · **commit history with measured deltas** and milestone tags.

**Cut these five:** "22 CI gates" as a headline (group by the failure class they
prevent, and show which actually caught a bug) · superlatives and unbounded
claims ("fastest", "zero overhead", "byte-identical on all inputs" → "on the
tested corpus, and by construction under these assumptions") · predicted scores
and cherry-picked timings · unmeasured micro-optimisation lists · self-verification
presented as independent.

**VERIFY.md — layered, each layer stating what it cannot catch:**

| Layer | What it is | Catches | Does not catch |
|---|---|---|---|
| 0 · Ground truth | organizers' reference + published expected hashes | any wrong output on their inputs | inputs they do not test |
| 1 · Conformance corpus | every documented edge case → fixture | spec misreadings | undocumented behaviour |
| 2 · Differential lab | optimized vs naive on generated inputs | optimizer-introduced divergence | shared misreading of the spec |
| 3 · Invariant sweeps | thread counts, chunk sizes, repeats, memory budget | non-determinism, boundary bugs, spikes | semantic errors |
| 4 · Sandbox gates | read-only /data, one output file, offline build, exit 0 | environment failures | — |

Plus, to make it convincing: a **bug ledger** (we already have one: the six
fixture defects the oracle caught, the four build/deadline defects, the five UX
defects, and the twenty hostile-review findings — list each with its fixing
commit); **mutation evidence** (inject 10–20 deliberate bugs — an off-by-one in
the rank, a missed mid-file BOM, `-0.00`, an AND/OR precedence swap — and report
caught N of N; schedule this for D2/D3); a short **"not verified"** list; and a
**map from each scorer gate (SHA, timeout, sandbox) to the checks protecting it**.

---

# 3 · P-D — the video pack (adopted; two corrections applied)

**Correction 1:** the narration began "This is This is my submission" — a
duplication; fixed below.
**Correction 2:** the 60-second capture originally assumed docker images and a
`memory.peak` read inside a container. **We run the capture with our own tooling**
(no Docker on the dev host); the docker variant is kept as the alternative if the
capture is made on a Docker-capable machine.

## 3.1 Slide outline — 13 slides, ~4:00

| # | Time | On-screen | Cue (first words of the paragraph) |
|---|---|---|---|
| 1 | 0:00 | Title: "Speed Demon 2026 — four tracks, Rust standard library only", repo URL | This is my submission |
| 2 | 0:11 | Four panels A–D: input size + organizer-measured reference (32.05 s/228 MB, 56.35 s/2.1 GB, 14.44 s/350 MB, 72.69 s/852 MB) | Track A ingests |
| 3 | 0:37 | `K = (Tref/T)^0.5 · (Mref/M)^0.2 · (Sref/S)^0.15 · (Dref/D)^0.15`, `speed = 50·log10 K`; note S,D capped at 1.0 and D = 1.00 | Each solution is scored |
| 4 | 1:04 | Three gates: SHA-256 match, timeout, sandbox | Before speed counts |
| 5 | 1:22 | A: unsorted lines → (hour, endpoint) groups → rank selection; `k=(p·n+99)//100`, `ppm=errs·10^6//count` | Track A groups |
| 6 | 1:45 | B: docs → token stream → query-term postings + phrase matcher; `a OR b AND c = (a OR b) AND c` | Track B tokenizes |
| 7 | 2:05 | C: hostile CSV with each trap marked; `12.34 → 1234`; `-0.00` never printed | Track C reads |
| 8 | 2:26 | D: graph + `off[]`/`edge[]`; "unreachable = −1, s = t = 0" | Track D converts |
| 9 | 2:47 | Results table: track, T ref, T mine, M ref, M mine; footnote with commit hash + protocol | Here are the measured |
| 10 | 3:01 | Fixed slabs → any workers → ordered merge → one file; "same hash at 1, 2, 6, 12 CPUs" | Output is byte for byte |
| 11 | 3:18 | Checklist: reference, published hashes (4/4), 58/58 tests, guardrail, CI | For correctness I relied |
| 12 | 3:34 | Two columns: **Independent evidence** vs **Supporting checks** | I also built |
| 13 | 3:48 | Repo URL, commit hash, "Next: one-minute terminal run" | The repository contains |

## 3.2 Narration for ElevenLabs (596 words — paste as-is)

This is my submission to Speed Demon twenty twenty-six. It has four optimization tracks, all written in Rust with only the standard library and no external dependencies.

Track A ingests three point one million log lines and reports latency percentiles and error rates. Track B indexes a two hundred fifty megabyte corpus and answers boolean and phrase queries. Track C joins a hostile C S V file to a lookup table. Track D answers two thousand shortest path queries on a graph with one point seven four million nodes and five million edges.

Each solution is scored against a naive Python reference. The score multiplies four ratios of reference to mine: time to the power of one half, memory to the power of one fifth, and code size and dependency count each to the power of three twentieths. Speed is fifty times the base ten logarithm of that product. Time counts most, and extra code or dependencies can only cost points.

Before speed counts, three gates must pass. The output file must match a published S H A two fifty six hash exactly, the run must finish within the time limit, and it must stay inside the sandbox: no network, read-only input, and one output file.

Track A groups log lines by hour and endpoint. The file is not sorted, so grouping cannot depend on arrival order. Percentiles use nearest rank and are exact, computed by selection rather than a full sort or an approximate sketch. The error rate is an integer in parts per million, so no floating point touches the output.

Track B tokenizes the corpus in a single streaming pass and builds postings only for terms that the queries use. Boolean queries are evaluated strictly left to right, as the specification requires. Phrase queries are matched against the token stream in the same pass, and never cross a document boundary.

Track C reads the C S V with one state machine that handles quoted commas, escaped quotes, embedded line breaks and byte order marks. Amounts are parsed directly to integer cents, so money is exact and never passes through floating point. Rows with an unknown region are grouped under a final unmatched bucket.

Track D converts the edge list into a compact adjacency array, then answers each query with Dijkstra's algorithm using integer distances. Queries run in parallel on all available threads, and answers are written in query order. Unreachable pairs print minus one, and a query from a node to itself prints zero.

Here are the measured running time and peak memory for each track beside the Python reference. Times follow the scorer's protocol: five runs, dropping the fastest and the slowest, and averaging the middle three.

Output is byte for byte identical across runs and thread counts. Work is divided into fixed pieces that do not depend on how many threads are running, results are merged in a fixed order, and no output depends on hash map iteration order.

For correctness I relied on independent sources. The organizers' reference implementation and their published expected hashes are the ground truth. The organizers' test suite of fifty-eight tests passes, their Dockerfile guardrail passes, and all of our continuous integration gates pass.

I also built a differential testing lab and a conformance corpus covering every documented edge case. These are supporting checks. The reference and the published hashes remain the only independent evidence that the outputs are correct.

The repository contains the code, a performance write-up and the verification results. Next is a one minute silent terminal run, ending with the hash check. Thank you for watching.

## 3.3 The 60-second terminal capture (silent, music over it)

**On-camera sequence (our tooling — works on this host):**

| Time | Command | What appears |
|---|---|---|
| 0:00–0:07 | `git log -1 --format='%h %s'; git status --short \| wc -l; du -sh verification/data/track-*` | commit hash + subject, `0`, input sizes |
| 0:07–0:20 | `python tooling/official-data.py --track all --scale sample` | 8 published-hash `ok` lines |
| 0:20–0:38 | `for t in a b c d; do .\tooling\run.ps1 bench-rung.sh $t --label demo; done` | per-track build + determinism + middle-3 T |
| 0:38–0:50 | `for c in 1 2 6 12; do SD_THREADS=$c ./track-a/run ...; sha256sum out/summary.txt; done` | the same hash prefix at 1, 2, 6 and 12 CPUs |
| 0:50–1:00 | `sha256sum -c expected.sha256; echo exit=$?` | four `...: OK` lines, then `exit=0` |

**If the capture is made on a Docker-capable machine**, use the scorer's own
shape instead (records wall time inside the container and reads the container's
`memory.peak`):

```sh
measure() { docker run --rm --network none --cpuset-cpus=0-11 --memory=8g \
  -v "$DATA:/data:ro" -v "$OUT/$1:/out" --entrypoint sh "$PFX-$1" \
  -c 's=$(date +%s%N); /app/run; e=$(date +%s%N); echo $(( (e-s)/1000000 )) $(cat /sys/fs/cgroup/memory.peak)' ; }
```

**Rules:** record at real speed (the numbers on screen must be the real ones — no
speed-ramp); 1080p, ~22 pt font, dark theme, `PS1='$ '`; four small static
captions only if the output supports them ("Commit under test", "Five runs,
middle three averaged", "Same hash at 1, 2, 6 and 12 CPUs", "Published hashes:
4 of 4 match"); music must be **public domain or properly licensed, checking the
recording as well as the composition** — fade in at 4:00, out over the last two
seconds.
