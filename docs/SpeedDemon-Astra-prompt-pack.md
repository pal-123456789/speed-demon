# SpeedDemon — Astra Prompt Pack

**Purpose:** generate an *insane-level*, foolproof, 72-hour, **solo**, **all-five-tracks** winning plan for the Speed Demon hackathon (Oct 30 – Nov 2, 2026), using a frontier model ("gpt 6 Astra"). You feed these prompts to Astra; you paste Astra's output back to me; I verify every claim and we assemble the final plan into this folder.

---

## How to use this pack (read first)

1. **Run everything in ONE Astra conversation, in order.** Context carries forward, so you only paste the big CONTEXT BLOCK once (it's embedded in Prompt 1). Prompts 2–8 then build on what Astra already produced.
2. **Order:** Prompt 1 (Master Strategy) → Prompts 2–6 (Track A, B, C, D, E deep dives) → Prompt 7 (Deliverables & Judge strategy) → Prompt 8 (Verification & Foolproofing).
3. **After each Astra reply, paste it back to me.** I check the numbers/formula/claims against the real spec before you rely on it (honest-claims rule: every optimization must be real and measurable, never assumed).
4. **If Astra's output gets cut off**, reply to it with "continue from where you stopped" — don't restart.
5. Each prompt is the text inside the fenced block. Copy the whole block.

> Note on legality: Rules 04/05 — all *project code* must be written inside the 72h window, but *preparing your environment, reading algorithms, and sketching designs beforehand are explicitly allowed*. This plan is design/preparation, which is permitted. No code is committed before kickoff.

---

## PROMPT 1 — MASTER STRATEGY

```
ROLE: You are a world-class performance engineer and competition strategist. You have
shipped database internals, codecs, and query engines; you read flame graphs for fun;
you have won measured, machine-judged performance contests before. You are planning a
SOLO, all-five-tracks assault on the "Speed Demon" 72-hour performance hackathon, and
you will produce a plan so complete and so deep that a strong solo engineer stays
productively occupied for the FULL 72 hours and leaves no cheap point on the table.

READ THIS CONTEXT BLOCK COMPLETELY. It is the entire, frozen ruleset. Treat every number
as authoritative. Do not invent facts; where a per-track reference number is not given
here, say "from the Oct 28 frozen spec/harness" and parameterize the plan around it.

================================ CONTEXT BLOCK ================================
EVENT: "Speed Demon", by Hackathon Raptors. A 72-hour performance championship over
Halloween weekend 2026. Online, free, solo entry (1–4 allowed; I am SOLO). 5 tracks
(A–E). $2,000 prize pool. Tagline: "Every millisecond is a soul."

THE PREMISE: Same task, same data, same machine. Each track gives you (1) a task spec,
(2) a dataset, (3) a deliberately naive-but-correct reference implementation (published
in Go, Python AND Rust at freeze). You re-solve it correctly and fast, in ANY language,
and ship a Dockerfile. THEY build and run it — you never report a number. Your output
must hash SHA-256 identical to the reference's, byte-for-byte, or you score zero.

SCORE = 65% machine (an "efficiency coefficient" K mapped to a 0–100 speed score)
      + 35% human judges (code quality, technique, and a required PERF.md log).

===== THE EFFICIENCY COEFFICIENT K (this is 65% of each track; memorize it) =====
K = (Tref/T)^0.40 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15 × (Xref/X)^0.10

Five axes, five exponents that SUM TO EXACTLY 1.0. It is a weighted GEOMETRIC mean of
five ratios. Every ratio is reference-over-yours, so a ratio of 1.0 = you matched the
naive reference, 10.0 = you beat it tenfold. Because the reference is fixed, your score
NEVER moves when another team submits — the board reorders around you.

  T  wall time        weight 0.40   — the mean of the middle 3 of 5 runs. BIGGEST weight.
  M  peak memory      weight 0.20   — cgroup memory.peak (kernel high-water mark).
  S  source size      weight 0.15   — bytes AFTER canonical formatter, comments stripped,
                                      identifiers normalized to fixed-width tokens
                                      (so renaming vars to 1 letter does NOTHING).
  D  dependency weight weight 0.15  — D = 1 + (non-stdlib runtime deps)
                                      + (vendored source bytes ÷ 50,000). Reference D=1.
  X  scaling          weight 0.10   — (your runtime at 10× input ÷ your runtime at 1×),
                                      compared to the reference's own 10×/1× ratio.

What a 100× win on each axis contributes to the product:
  T:100^0.40=6.31   M:100^0.20=2.51   S:100^0.15=2.00   D:100^0.15=2.00   X:100^0.10=1.58
=> TIME DOMINATES. A zero-dependency submission (D=1) is worth ~10–17% on K before any
other work, for free. Source size is "priced, not punished" — a strong submission is
often LARGER than the naive reference and that is an accepted trade for speed.

CAP: every factor is capped at 50× (no axis can contribute more than 50^exponent). In
practice teams hit the cap on MEMORY, almost never on time.

K → POINTS: speed_score = 50 × log10(K), clamped to [0,100].
  K=10 → 50 pts.  K=100 → 100 pts.  K=1 → 0 pts.  K<1 → 0 (clamped).
  Reference points on the curve: K 1.0/2.0/5.0/10.0/20.0/50.0/100 →
  score 0.0/15.1/35.0/50.0/65.1/85.0/100.0. The curve is logarithmic on purpose:
  the first 2× is an afternoon; 10×→20× is a weekend rewrite.

(TRACK E — Compress — was CUT before the freeze. Four tracks, A–D.)

WORKED EXAMPLE (the organizers' published one, Team Nightshade, Track A) — canonical:
  T 32.05s→1.28s = 25.04× (^0.50=5.004)   <- half the weight is TIME, spend there
  M 210MB→36.8MB = 5.70×  (^0.20=1.416)
  S 56→109 lines  = 0.51×  (^0.15=0.905)  <- wrote 2× more code, LOST points here
  D 1.00→1.00    = 1.00×  (^0.15=1.000)
  K = 6.41  →  speed score 40.4/100.  Judge 82/100.
  Track A total = 40.4×0.65 + 82.0×0.35 = 26.24 + 28.70 = 54.94.
  Lesson the organizers draw: 25× time bought +5.00; the S axis (0.15 weight, capped at
  1.0) punishes verbosity. The formula tells you where to work.

===== THE THREE GATES (pass/fail, checked on EVERY run) =====
Fail ANY gate → zero on that track, and they publish which gate you failed.
  1. CORRECTNESS: SHA-256 of your output file == expected hash, exactly. Spec pins line
     ordering AND float formatting so the check is binary. Right 4/5 runs = FAIL
     (nondeterminism is a correctness bug). The sample ships with its expected hash.
  2. TIMEOUT: hard kill at 10× the reference time. Track A 321s, B 564s, C 144s,
     D 727s. A hang is a failed run, not a slow one.
  3. SANDBOX: no network; dataset mount read-only; root filesystem read-only; scratch is
     a 1 GB tmpfs at /tmp. They diff the container filesystem after every run. Writing
     precomputed answers into the image, phoning home, or touching the dataset mount =
     DISQUALIFICATION FROM THE WHOLE EVENT, not just the track. (Rule: no precomputed
     answers; the scoring dataset uses a seed you have never seen; code that recognizes
     the input and short-circuits is DQ.)

===== THE MACHINE (fsn1) — design for exactly this =====
Hetzner EX44 dedicated, Falkenstein (fsn1) — NOT Helsinki. Intel Core i5-13500:
14 cores / 20 threads (6 P-cores @ up to 4.80GHz, 8 E-cores @ 3.50GHz).
Cache: 544KiB L1d, 704KiB L1i, 11.5MiB L2, 24MiB L3. RAM 64GB DDR4-3200 (2×32GB),
non-ECC. Storage 2×512GB Micron 3500 NVMe RAID1. OS Debian 13 (trixie). Docker +
cgroups v2.
Before scoring they set no_turbo=1 (performance governor, turbo OFF → sustained ~2.5GHz
base, not a boost ceiling) and pin the container to `--cpuset-cpus=0-11` with an 8 GB
memory cap. Design parallelism for exactly 12 workers on 12 scheduled CPUs; spawning
20 workers will hurt.

===== THE EXACT SCORING RUN (verbatim; only team/track/commit substituted) =====
  docker build --network=none -t speeddemon/$TEAM-$TRACK:$COMMIT track-$TRACK
  vmtouch -t /srv/speeddemon/data/track-$TRACK      # dataset pre-warmed into page cache
  docker run --rm --cpuset-cpus="0-11" --cpuset-mems="0" \
    --memory=8g --memory-swap=8g --pids-limit=512 --network=none --read-only \
    --tmpfs /tmp:rw,size=1g,exec --ulimit nofile=8192:8192 \
    -v /srv/speeddemon/data/track-$TRACK:/data:ro -v /srv/speeddemon/out/$RUN:/out:rw \
    -e TRACK=$TRACK speeddemon/$TEAM:$COMMIT
  cat /sys/fs/cgroup/<container>/memory.peak                 # memory read from cgroup
Key consequences: --memory=8g --memory-swap=8g means swap is DISABLED (equal values);
exceeding 8 GB is an OOM KILL = failed run, not slow. Dataset is pre-warmed, so you are
measured on compute, not NVMe. Build is offline AND build time is NOT scored (profile-
guided optimization / codegen at build time is FREE). Only the run is timed.

DOCKERFILE CONTRACT (4 rules, nothing else about the image is scored; base image free):
  1. Read from /data (read-only, filenames fixed by the track spec).
  2. Write to /out — EXACTLY ONE output file, named by the spec. Anything else you leave
     behind is flagged by the filesystem diff.
  3. Exit 0 (any non-zero exit = failed run).
  4. Build offline (--network=none at build; any apt/cargo/pip/curl fetch fails the build
     → vendor everything into the image).
  Expected shape: FROM debian:13-slim / COPY . /app / WORKDIR /app / RUN ./build.sh /
  ENTRYPOINT ["/app/run"]  (reads /data, writes /out, exits 0).

TIMING PROTOCOL: 5 runs, discard highest+lowest, mean of middle 3 = your T. >5% spread
auto re-runs. They run the reference before AND after each batch; >2% reference drift
throws out and rescores the batch. Runs are serialized; nothing else on the box.

LEADERBOARD: during the 72h you push to a submission branch for an automated run on the
10% SAMPLE dataset, rate-limited 1 run / team / 15 min, public. FINAL scoring is on a
HOLDOUT dataset after freeze — the only set that counts. Gap between sample rank and
final rank = the price of overfitting.

===== THE FOUR TRACKS (each = one fixed task on one fixed dataset) =====
Enter as many as you like; 3 is the minimum for the overall title. I am entering ALL 4.
Per-track reference TIME and TIMEOUT are given; other reference axis values (M/S/D) come
from the Oct 28 frozen spec/harness — parameterize around them, don't invent them.
Only the LIBRARY THAT SOLVES THE WHOLE TASK is banned per track (named in each spec);
other dependencies are allowed but charged on the D axis.

TRACK A · INGEST (the parsing track; recommended starter)
  Task: aggregate 3.1M structured log lines (312 MB) into per-endpoint request counts,
  p50/p95/p99 latency, and error rates bucketed by hour. Ref 41.2s / 2940MB. Timeout 412s.
  Data: space-delimited, one request per line. Fields:
    ts         RFC3339, microsecond precision, monotonic per shard, interleaved across shards
    method     enum, 7 values
    endpoint   string, 1,842 distinct, heavy Zipf tail
    status     uint16 (5xx counts toward error rate)
    latency_us uint32 (MICROseconds, not millis)
  Sample lines (exact):
    2026-10-30T18:00:00.114982Z GET /v2/orders/{id} 200 1841
    2026-10-30T18:00:00.115330Z POST /v2/checkout 503 204117
    2026-10-30T18:00:00.115332Z GET /health 200 62
  Naive reference: reads lines, splits on delimiters, parses timestamps with a date
  library, pushes latencies into a growing per-endpoint list, sorts each list at the end.
  Organizer hints: custom line parsing >> general reader; percentiles WITHOUT sorting the
  whole thing; "where most teams discover their allocator"; timestamp parsing is the
  second-biggest cost and nobody expects it.

TRACK B · INDEX (the trade-off track)
  Task: build an inverted index over a 250 MB text corpus, then answer 50,000 boolean +
  phrase queries. BUILD time and QUERY time are measured SEPARATELY and combined into one
  T, weighted 40% build / 60% query. Ref 58.7s. Timeout 587s.
  queries.txt, one per line, prefixed by kind:
    BOOL    62% — operators AND / OR / NOT, left-associative, NO parentheses
    PHRASE  38% — quoted "...", adjacent tokens, exact order, stopwords retained
  Samples: BOOL indemnity AND (nothing) NOT surety | PHRASE "writ of habeas corpus" |
           BOOL bail OR bond OR recognizance NOT forfeiture | PHRASE "in the matter of"
  Hints: posting-list compression pays at this size; intersection order is most of the
  boolean win; phrase queries need positions and positions are where memory goes.
  BANNED: any ready-made search engine.

TRACK C · JOIN (the correctness track; most disqualifications happen here)
  Task: parse a HOSTILE multi-megabyte CSV (204 MB) — quoted commas, escaped quotes,
  embedded newlines, mixed CRLF/LF line endings, a UTF-8 BOM on SOME chunks mid-file and
  not others — join it against a lookup table, emit sorted aggregates. Ref 36.9s. Timeout 369s.
  records.csv: 5 fixed columns id,name,note,region_id,amount (column COUNT is fixed;
  everything else about the encoding is not). Bytes as they appear on disk (⟨BOM⟩=U+FEFF
  appearing mid-file at some chunk boundaries, ␍␊=CRLF, ␊=bare LF):
    ⟨BOM⟩id,name,note,region_id,amount␍␊
    1,"Kaphar, Titus","said ""no"" twice",7,1420.00␍␊
    2,"Betts","a note that␊spans two lines",7,88.50␊
    3,,"",12,0.00␍␊
    4,"trailing space ","comma, quote "" and newline␊together",3,-5.25␊
  Hints: hash join vs sort-merge on data where the answer isn't obvious; quoting rules are
  non-negotiable; a parser 3× faster and wrong scores zero — budget time for verification
  rather than discovering a bug at hour 68. BANNED: any analytics/dataframe library.

TRACK D · TRAVERSE (the algorithms track; highest ceiling)
  Task: a 5,000,000-edge directed weighted graph (180 MB) and 2,000 source-target
  shortest-path queries. Preprocessing is EXPLICITLY ALLOWED and is counted INSIDE your
  timed run — so the build-vs-query trade is the central decision, not a footnote.
  Ref 72.4s. Timeout 724s.
  graph.txt: first line header "n m" (node count, edge count); then one edge per line
  "u v w" as uint32×3 (tail, head, weight), NOT sorted by u. Node ids dense, 0-indexed.
  Weights uint32. Directed; a reverse edge, where it exists, is listed separately.
  Samples: "1740512 5000000" then "0 918442 37" / "918442 0 41" / "1 7 1200" / "1739998 62 9".
  Hints: preprocessing is the whole game; memory layout beats asymptotics at this size,
  consistently; bidirectional search is table stakes, not a strategy; contraction
  hierarchies are legal and expensive to get right in 72h.

TRACK E · COMPRESS (the trade-off IS the subject)
  Task: LOSSLESS compression of a fixed 100 MB mixed corpus. Scored on RATIO and
  DECOMPRESSION throughput together, so you can't win by being slow+small or fast+fat.
  Ref 29.8s, reference overall ratio 2.41×. Timeout 298s. Round-trip must be BIT-EXACT.
  COEFFICIENT CHANGES SHAPE HERE: compressed size replaces peak memory in the M slot;
  compression time is measured but weighted at 1/4 of decompression time.
  BANNED: general-purpose compression libraries — you WRITE THE CODER.
  corpus.bin = 4 concatenated segments, offsets fixed, aligned to 1 MiB, in file order:
    prose  31,457,280 B  English text UTF-8, long runs            ref 2.94×
    logs   27,262,976 B  the Track A line format, repeated fields ref 4.11×
    json   23,068,672 B  nested records, high key repetition      ref 3.32×
    blob   23,068,672 B  float32 sensor frames, low entropy/column ref 1.18×
  Segment boundaries are published; each segment rewards a DIFFERENT model.

===== SUBMISSION (per track) — a repo they can build without asking a question =====
  Dockerfile        meets the 4 rules; builds offline from a clean checkout
  PERF.md           WORTH 8% of the track total — the optimization log (see judges)
  README.md         prose: the approach, dependency list if any, "what I'd do with +24h"
  .speed-demon.toml machine-readable: track letter, team name, language, declared dep count
  src/              OSI-licensed, PUBLIC GitHub at freeze, one directory per track
  demo.mp4          OPTIONAL, <3 min, NOT scored (good ones get into the write-up)

===== THE JUDGE 35% (seen WITHOUT the measured board, on structured forms) =====
  Code quality 15% — "would you merge this?" dense is fine, unexplainable is not; a judge
                      must be able to follow WHY the fast version is fast.
  Technique    12% — evidence of DECISIONS, not brute force; good data layout > hand-SIMD
                      bolted onto the wrong algorithm.
  PERF.md       8% — the optimization log: every step naive→final with the MEASUREMENT
                      that justified it ("naive 41s → flat array not hashmap → 8s → manual
                      line split → 2.1s → tried SIMD timestamp, slower, reverted").
                      Measurements, not adjectives. FAILED experiments are rewarded.

===== THE OVERALL TITLE & PRIZES ($2,000 pool) =====
Per-track winner = best total on that track. OVERALL "Speed Demon" title = GEOMETRIC MEAN
of your track totals, minimum 3 tracks completed (geometric mean refuses to let one
spectacular track carry two mediocre ones: 90/40/40 → 52.4 loses to 60/60/60 → 60.0).
  $800 Speed Demon (highest geo mean, 3+ tracks)   $400 runner-up   $200 third
  $200 Highest K anywhere (single largest K on ANY track — specialists, no 3-track need)
  $100 Lean award (best K with the smallest normalized source)
  $300 Write-Up side quest ($100 × 3, closes Nov 10 — a post on the optimization journey,
       judged on insight; tag Hackathon Raptors)

===== TIMELINE (all UTC) =====
  Oct 28 18:00 (T-48h): specs + reference impls (Go/Python/Rust) + FULL harness published;
                        frozen — nothing about scoring changes after this.
  Oct 30 06:00 (T-12h): 10% sample datasets + their expected output hashes (verify locally).
  Oct 30 18:00 (T-0):   KICKOFF — full datasets, live leaderboard opens, clocks start.
  Oct 31 (midnight):    standings + a written teardown of every deliberately bad decision
                        in the reference implementation (free intelligence — exploit it).
  Nov 2 18:00:          CODE FREEZE — repos locked at the recorded commit.
  Nov 2–4 scoring (holdout) · Nov 3–12 judging · Nov 10 write-up closes · Nov 13 winners.

===== RULES THAT BITE =====
  Any language (builds offline in a Linux x86-64 container; assembly / your own compiler
  eligible). Deps allowed but priced on D; only the whole-task library is banned per track.
  Correct output or no score. No precomputed answers / no input-recognition short-circuit.
  NEW CODE ONLY — all project code written inside the 72h; reading algorithms, sketching
  designs, and preparing the environment BEFOREHAND are explicitly fine; code committed
  before kickoff disqualifies. AI tools expected. Source public (OSI) at freeze. The
  harness is the referee (48h to request a re-run). The formula does not change after kickoff.
============================== END CONTEXT BLOCK ==============================

============================ YOUR TASK: MASTER STRATEGY ============================
Produce the MASTER STRATEGY for a solo, all-five-tracks campaign. This is the skeleton;
the per-track deep dives come in later prompts. Be concrete, quantitative, and decisive —
no hedging, no generic advice. Output in GitHub-flavored Markdown with tables. Deliver:

1. SCORING PHILOSOPHY (the arithmetic that dictates where hours go). State explicitly, with
   numbers, why T (0.40) is the sun everything orbits, why D=1 is a free ~10–17% you lock
   in on day one, why S is "don't bloat" rather than "minimize", why M has sharply
   diminishing returns (and where the 8 GB OOM wall and the 50× cap change that), and why
   X is a "don't lose" sanity axis not a battleground. Give the generic per-axis TARGET
   ratios you will aim for on a typical track and the K/speed-score they imply on the
   100×ln(K)/ln(25) curve. Name the realistic K ceiling for a solo competitor vs. the
   K=25 (=100 pts) saturation point, and argue what K band is actually worth chasing.

2. ATTACK ORDER across A,B,C,D,E for ONE person over 72h, EV-ranked. Build a table scoring
   each track on: expected K ceiling, effort-hours to a safe correct baseline, effort-hours
   to a competitive K, DQ/correctness risk, and CODE REUSE it unlocks for other tracks
   (e.g. a vectorized line-scanner and RFC3339 parser from A feed E's "logs" segment and
   help C; C's CSV state machine; a shared mmap+arena+SIMD core used everywhere). Then give
   the actual sequence and WHY, including which track seals the "Highest K anywhere" bet and
   which track you keep deliberately lean for the "Lean award".

3. SHARED INFRASTRUCTURE built BEFORE touching any single track (this is written during the
   72h, legally, but it is the first thing you build): a zero-dependency core — memory-mapped
   read-only /data reader, bump/arena allocators, a 12-thread (6 P-core) work-stealing or
   chunk-partition executor sized to the pinned cpuset, SIMD byte-scanning primitives, a
   deterministic output writer, and — critically — a LOCAL harness that reproduces the
   scoring box (cpuset pin, --memory=8g no-swap, read-only root, tmpfs /tmp, 5-run median,
   SHA-256 vs the sample hash, memory.peak read from cgroups v2) plus a one-command bench
   runner and a CI push to the sample board. Specify the language of this core and why.

4. THE MASTER 72-HOUR SCHEDULE — hour-blocked, the centerpiece. Requirements:
   - A PRE-KICKOFF prep phase (legal: env, reading, design, local harness, profiling the
     published references, skeleton repos + Dockerfiles) spanning Oct 28 18:00 (specs/harness
     freeze) and Oct 30 06:00 (sample+hashes) up to Oct 30 18:00 kickoff.
   - The 72h from Oct 30 18:00 → Nov 2 18:00 in blocks (≈2–4h each) mapped to tracks and
     milestones, with a hard "all five pass the SHA gate on the sample by hour X" checkpoint,
     the Oct 31 midnight reference-teardown integration, realistic SLEEP blocks for a solo
     human (say so explicitly), daily sample-board pushes, and a frozen last-6-hours
     "no new ideas, only verify + write PERF.md/README + final commit" lockdown.
   - CRITICAL ANTI-UNDERSCOPE RULE: a previous plan was sized so the whole thing was finished
     in one day of a three-day event. Do NOT repeat that. Assume the SAFE correct baseline on
     each track is done early; the schedule's bulk must be the DEEP optimization that chases
     the last 30–50% on each axis (SIMD, PGO/BOLT, custom allocators, cache-blocking,
     algorithm swaps benchmarked head-to-head, posting-list/entropy-coder refinements). Every
     block must have a concrete deliverable and an expected K movement. If a strong engineer
     could finish a block in 30 minutes, it is too shallow — deepen it.

5. PRIZE-TARGETING MAP: how this one campaign simultaneously goes for the $800 overall (geo
   mean of 5 track totals — note the geo mean punishes any weak track, so there is NO
   throwaway track), the $200 highest-K, the $100 lean, and the $300 write-up side quest,
   and what each requires you to NOT skimp on.

6. GLOBAL RISK REGISTER + STOP RULES: the top ~10 risks (hash-gate float/ordering drift,
   nondeterminism across 5 runs, OOM at 8 GB, timeout, oversubscribed threads on 6 cores,
   D-axis creep from a careless dependency, overfitting to the 10% sample, solo burnout) each
   with a mitigation and an explicit stop rule ("if track X isn't SHA-clean by hour N, ship
   the safe baseline and move on").

End with a one-paragraph "definition of done" for the whole campaign. Do not write track-
specific algorithm detail yet — that is Prompts 2–6. Ask me no questions; make the call and
justify it. If you must assume an unpublished per-track reference number, flag it as an
assumption.
```

---

## PROMPT 2 — TRACK A DEEP DIVE (Ingest) + the reusable deep-dive template

```
Still the same campaign. Now go INSANELY deep on TRACK A (Ingest). This answer also
DEFINES the deep-dive template you will reuse for Tracks B–E, so structure it cleanly.
Use the CONTEXT BLOCK and your Master Strategy above. Output Markdown with tables and
short, illustrative code sketches (pseudocode or real) where a mechanism needs them.
Honest-claims rule: label every speedup as EXPECTED (hypothesis to measure) vs. KNOWN;
never state an unmeasured number as fact. Produce these sections:

  §1 REFERENCE TEARDOWN. Reconstruct the naive reference's hot path and attribute an
     estimated % of the 41.2s to each cost: line splitting, RFC3339 timestamp parse via a
     date library, per-endpoint growing list + final sort, hashmap of 1,842 endpoints,
     allocation/GC. State which costs you will attack first by payoff.
  §2 TARGET & AXIS BUDGET. Pick a target K and the per-axis ratio targets (T,M,S,D,X) that
     reach it on the 100×ln(K)/ln(25) curve; show the product arithmetic like the Nightshade
     worked example. Justify each target against effort. Note the memory 50× cap and the
     8 GB OOM wall.
  §3 ALGORITHM & DATA LAYOUT, from first principles. The winning approach (e.g. mmap +
     parallel chunk scan with boundary stitching; interning endpoints via a perfect/!hash
     into a dense id; per-(endpoint,hour) fixed-struct accumulators in SoA; p50/p95/p99
     WITHOUT full sort — e.g. per-endpoint latency histogram over the uint32 µs domain, or
     t-digest/quantile sketch IF it can still hash-match exact percentiles — reason about
     whether an exact-rank definition forces a histogram/counting approach). Present ≥2
     candidate designs in a table and say which you pick and why; judges reward decisions.
  §4 PARSING THE BYTES. Design a branchless/SIMD line scanner and a custom RFC3339
     microsecond timestamp parser (fixed-width field → epoch µs by arithmetic, no library)
     and integer parsers for status/latency. Address the Zipf endpoint tail (fast path for
     the hot few hundred). Show the inner loop shape.
  §5 MEMORY & ALLOCATOR. Arena/bump allocation, zero per-line allocation, SoA accumulators,
     cache-line fit against 11.5MiB L2 / 24MiB L3, exact peak-RSS estimate vs the M target.
  §6 PARALLELISM for EXACTLY 12 threads on 6 P-cores: chunk partition of the mmap, per-thread
     local aggregation, deterministic merge, how you avoid false sharing and oversubscription.
  §7 HASH-GATE CORRECTNESS & DETERMINISM — the make-or-break section. Pin exactly how the
     output must be formatted to match SHA-256: field/line ordering (which key, which sort),
     float formatting of latency/error-rate (decimal places, rounding mode, how parallel
     reductions stay bit-identical to a defined order), integer formatting, newline style,
     trailing newline, encoding. State how you guarantee identical output on all 5 runs
     (no hashmap iteration-order leakage, no FP non-associativity). Describe how you
     reverse-engineer the exact format from the reference + the sample's expected hash.
  §8 THE OPTIMIZATION LADDER (this IS your PERF.md spine). An ordered table: step → mechanism
     → EXPECTED effect → how measured. From naive baseline through each win, DEEP enough to
     fill many hours: single-pass, flat arrays, manual split, custom ts parse, SIMD scan,
     histogram percentiles, thread scaling, PGO/BOLT, cache-blocking. INCLUDE ≥3 experiments
     you expect to FAIL or revert (judges reward documented failures) — e.g. SIMD timestamp
     that loses, mmap vs read() , a lock-free map that underperforms sharding.
  §9 SCALING (X) SAFEGUARD: argue the design is ~linear so your 10×/1× ratio beats the
     reference's, and what would break it.
  §10 LANGUAGE CHOICE for this track + why (tie to D=1 and S bytes).
  §11 HOUR BUDGET for Track A inside the master schedule, block by block, each with a
      milestone and a local SHA-gate checkpoint against the sample.
  §12 TRACK RISK REGISTER: top risks + mitigations + the stop rule for this track.
```

---

## PROMPT 3 — TRACK B DEEP DIVE (Index)

```
Same campaign, same depth. Produce the Track A deep-dive template (§1–§12) now for TRACK B
(Index), honoring these track-specific pivots. Use the CONTEXT BLOCK + prior outputs.

  - T IS A 40/60 BUILD/QUERY BLEND. Treat it as two coupled optimization problems that pull
    against each other and allocate the axis budget accordingly; an index that builds fast
    rarely queries fast. Make the build/query trade an explicit, quantified decision.
  - CORE DESIGN SPACE to reason over (pick and defend): tokenization pass; dictionary +
    dense term ids; posting lists with COMPRESSION (delta + varint/StreamVByte/PForDelta —
    which, and why at 250 MB); skip pointers; for BOOLEAN (62%): intersection ORDER by
    smallest posting first, AND/OR/NOT with left-associative no-paren evaluation; for PHRASE
    (38%): POSITIONAL postings (where your memory goes) and positional-adjacency checking,
    stopwords retained. Consider whether to store positions for all terms or only as needed.
  - §5 MEMORY: positions dominate — budget them against M and the 8 GB wall; discuss
    compressed vs. uncompressed positional index.
  - §6 PARALLELISM: parallel build (shard the corpus, merge dictionaries/postings
    deterministically) and parallel query execution across the 50,000 queries on 12 threads.
  - §7 HASH GATE: pin the exact query-output format and ORDERING (per-query result ordering,
    doc-id ordering, separators, newline) so SHA matches; phrase-match semantics must equal
    the reference exactly (tokenization, stopword handling, punctuation).
  - §8 LADDER must include posting-list compression schemes benchmarked head-to-head, and
    intersection-order heuristics, as explicit measured steps + failed experiments.
  - BANNED: any ready-made search engine. Write the index yourself; keep D=1.
```

---

## PROMPT 4 — TRACK C DEEP DIVE (Join) — the correctness minefield

```
Same campaign, same depth, but CORRECTNESS IS THE TASK HERE — most disqualifications happen
on Track C. Produce the §1–§12 template for TRACK C (Join), with these pivots. Spend the
most ink on §7. Use the CONTEXT BLOCK + prior outputs.

  - §3/§4 THE CSV STATE MACHINE is the heart. Design a correct, fast, branch-lean parser that
    handles EVERY hostile feature shown in the sample bytes: fields quoted with "..."; quoted
    commas; escaped quotes ("" → "); embedded bare-LF and CRLF INSIDE quoted fields; mixed
    CRLF vs bare-LF row terminators; a UTF-8 BOM (U+FEFF) appearing MID-FILE at some chunk
    boundaries (not just at the start); empty unquoted fields; empty quoted fields ("");
    trailing spaces inside quotes; negative and decimal amounts. Decide whether SIMD quote/
    delimiter scanning (classify-and-mask) is safe given embedded newlines, or whether a
    scalar state machine is the correct call — argue it. Reconcile parallel chunking with
    quoted fields that span chunk boundaries (how you find safe split points, or a serial
    pre-scan).
  - JOIN STRATEGY: hash join vs sort-merge against the lookup table — pick with reasoning for
    this data; then emit SORTED aggregates (define the exact sort key + tie-breaks).
  - §7 HASH GATE (expanded): BOM handling in output or not; exact numeric formatting of
    amount sums (fixed decimals? rounding? how to avoid float drift — consider integer/fixed-
    point cents accumulation so sums are bit-exact and deterministic); row ordering;
    delimiter/quote rules in the OUTPUT; newline style; encoding. Show how you build a
    differential tester: run your parser AND the published reference on the sample and diff,
    plus hand-crafted adversarial micro-inputs for every quoting edge case.
  - §8 LADDER: start from a KNOWN-CORRECT (possibly slow) parser, lock the hash, THEN
    optimize without ever breaking it; every optimization step re-verifies the SHA. Include
    the failed-experiment entries (e.g. an over-clever SIMD path that mishandles embedded
    newlines and is reverted).
  - §12 STOP RULE must be strict: correctness-first; if a fast path risks the hash, ship the
    correct one. BANNED: any analytics/dataframe library; D=1.
```

---

## PROMPT 5 — TRACK D DEEP DIVE (Traverse) — highest ceiling

```
Same campaign, same depth. This track has the widest gap between competent and excellent,
and PREPROCESSING IS COUNTED INSIDE YOUR TIMED RUN, so the build-vs-query trade is the whole
game. Produce the §1–§12 template for TRACK D (Traverse), with these pivots. Use the CONTEXT
BLOCK + prior outputs.

  - §3 DESIGN SPACE, ranked by ceiling vs. 72h-solo risk: tuned bidirectional Dijkstra with a
    cache-friendly graph (CSR layout, 4-ary/d-ary or radix/bucket heap) as the SAFE strong
    baseline; then preprocessing accelerators measured against total timed cost for 2,000
    queries — Contraction Hierarchies (legal, high payoff, expensive/risky to get exactly
    right), or a lighter approach (ALT/landmarks + A*, arc-flags/partition). Decide what fits
    a 5M-edge graph and 2,000 queries given preprocessing is amortized over only 2,000
    queries — explicitly compute whether heavy preprocessing even pays at that query count.
  - §5 MEMORY LAYOUT BEATS ASYMPTOTICS at this size: CSR with uint32 ids, edge arrays sized
    against L2/L3, how you build CSR from the unsorted "u v w" edge list fast; directed graph
    + separately-listed reverse edges → build forward AND reverse CSR for bidirectional.
  - §6 PARALLELISM: parallelize preprocessing (CSR build, CH contraction or landmark
    selection) across 12 threads; the 2,000 queries are embarrassingly parallel — but keep
    per-query results deterministic.
  - §7 HASH GATE: shortest-path output format — distance only or the path? exact tie-breaking
    when multiple shortest paths exist (critical: the reference's tie-break MUST be matched —
    reason about how the naive reference would break ties and replicate it); unreachable
    handling; ordering of the 2,000 answers; integer vs formatted distances; newline.
  - §8 LADDER: baseline Dijkstra → CSR + better heap → bidirectional → preprocessing variant,
    each as a measured step with expected total-timed-run effect; failed experiments (e.g. a
    CH build that overruns, or a landmark set that doesn't pay) documented.
  - §9 X-SCALING is subtle because preprocessing is inside the timed run — reason about how
    the 10×/1× ratio behaves when preprocessing dominates, and don't lose the axis.
```

---

## PROMPT 6 — TRACK E DEEP DIVE (Compress) — the coefficient changes shape

```
Same campaign, same depth. On TRACK E the coefficient CHANGES: the M slot is COMPRESSED SIZE
(ratio), and T weights compression at 1/4 of decompression; round-trip must be BIT-EXACT; and
general-purpose compression libraries are BANNED — you write the entropy coder yourself.
Produce the §1–§12 template for TRACK E, with these pivots. Use the CONTEXT BLOCK + prior.

  - RE-DERIVE THE AXIS BUDGET for E's modified coefficient: here you are jointly maximizing
    (decompression speed ratio on T, 0.40) and (compression-ratio on the M slot, 0.20), with
    compression time only 1/4-weighted inside T. So a fast DECODER + strong ratio wins; a slow
    encoder is cheap. Make this trade explicit with numbers.
  - §3 SEGMENT-SPECIALIZED MODELS — the corpus is 4 published, 1 MiB-aligned segments, each
    rewarding a different model; design a per-segment coder and a container format:
      prose (31,457,280B, English, long runs; ref 2.94×)  → order-N context model / PPM-ish
         or a fast static/semi-static entropy coder; reason about dictionary/LZ + entropy.
      logs (27,262,976B, Track A line format, repeated fields; ref 4.11×) → columnar/field
         split + delta + entropy; REUSE the Track A parser knowledge.
      json (23,068,672B, nested, high key repetition; ref 3.32×) → dictionary of keys +
         structural modeling + entropy.
      blob (23,068,672B, float32 sensor frames, low entropy PER COLUMN; ref 1.18×) → column
         transpose / stream split of float32 (sign/exponent/mantissa planes) + delta, then
         entropy; this is where the biggest ratio upside hides vs the weak 1.18× reference.
    Choose the ENTROPY BACKEND (range coder / rANS / tANS / arithmetic) and justify for
    DECODE SPEED, since decode dominates T. Keep ONE fast decoder shared across segments.
  - §5 "MEMORY" here = compressed size: budget bytes per segment vs the ref ratios; the
    product of per-segment sizes sets your overall ratio vs 2.41×.
  - §7 BIT-EXACT ROUND TRIP: decode(encode(x)) == x byte-for-byte for the SHA gate; determinism
    of the coder; the exact output = the compressed file (define container header/layout).
  - §6 PARALLELISM: encode segments/blocks in parallel; keep DECODE parallelizable too, since
    decode speed is the heavily-weighted axis.
  - §8 LADDER: per-segment model improvements benchmarked on ratio AND decode MB/s, with the
    failed experiments (a model that compresses better but decodes too slowly to be worth it —
    exactly the trade this track is about). BANNED: general-purpose codecs; D=1 (write it all).
```

---

## PROMPT 7 — DELIVERABLES SYSTEM & JUDGE (35%) STRATEGY

```
Same campaign. Now design the DELIVERABLES SYSTEM that wins the 35% human-judged portion and
makes every track submittable without a human in the loop. Use the CONTEXT BLOCK + prior
outputs. Produce:

  1. REPO LAYOUT for a 5-track monorepo (public GitHub, OSI license at freeze): one dir per
     track, the shared zero-dep core, the local harness, and where PERF.md/README/.speed-
     demon.toml/Dockerfile live per track. Give the actual tree.
  2. DOCKERFILE TEMPLATE honoring the 4-rule contract: FROM debian:13-slim, offline build
     (no apt/cargo/pip/curl at build — everything vendored), ./build.sh that may do PGO/BOLT
     (build time is free + unscored), ENTRYPOINT reads /data (fixed filenames), writes EXACTLY
     ONE file to /out, exits 0. Note the filesystem diff: leave nothing else behind. Show the
     per-track variant and how $TRACK env is handled.
  3. .speed-demon.toml template (track letter, team name, language, declared dependency count)
     and a reminder that declared dep count must match reality (D axis honesty).
  4. PERF.md TEMPLATE (worth 8%): a header with the final per-axis ratios + K + speed score,
     then the optimization ladder as a MEASUREMENTS table (step, change, before→after on the
     measured axis, verdict), with a dedicated "experiments that failed / were reverted"
     section — the organizers explicitly reward these. Rule: measurements, not adjectives;
     every claimed number must be reproducible by the local harness (honest-claims).
  5. README.md TEMPLATE: plain-prose approach per track, dependency list (ideally "none, D=1"),
     and the required "what I'd do with another 24 hours" section.
  6. CODE-QUALITY-FOR-JUDGES rubric: how to keep optimized code readable and MERGEABLE — a
     comment wherever the trick isn't self-evident, honest error handling, clear structure so
     a judge can follow WHY the fast version is fast (dense is allowed, unexplainable is not).
     Note that judges review ANONYMIZED code WITHOUT seeing the measured board.
  7. WRITE-UP SIDE QUEST ($300) plan: outline a post on the optimization journey (the misread
     profile, the rewrite that got slower, the two-line change that halved it), tag Hackathon
     Raptors, due Nov 10 — reuse PERF.md content. Keep it insight-driven.
  8. OPTIONAL demo.mp4 (<3 min, unscored) go/no-go given solo time budget.
```

---

## PROMPT 8 — VERIFICATION & FOOLPROOFING (the "full-proof" capstone)

```
Same campaign. Produce the VERIFICATION & FOOLPROOFING SYSTEM so nothing fails a gate on the
real box. This is the capstone that makes the plan "full-proof". Use the CONTEXT BLOCK + prior.

  1. LOCAL SCORING-BOX REPLICA: exact local commands/scripts that mirror the harness — docker
     run with --cpuset-cpus (pin to physical cores; note my machine isn't the i5-13500, so
     treat local numbers as RELATIVE, the real board is theirs), --memory=8g --memory-swap=8g
     (prove no-swap), --read-only + --tmpfs /tmp:size=1g, --network=none, --pids-limit=512,
     --ulimit nofile=8192:8192, dataset mounted :ro. Read peak memory from
     /sys/fs/cgroup/<container>/memory.peak (cgroups v2). Run 5×, discard hi/lo, median = T.
  2. THE SHA GATE AUTOMATION: a script that runs the container on the 10% sample, computes
     SHA-256 of /out, and compares to the published expected hash; red/green. Wire it so EVERY
     optimization commit re-verifies before it's allowed to land.
  3. DETERMINISM PROVING: run each track ≥5× (ideally 10×) and assert BYTE-IDENTICAL output
     every time (nondeterminism = correctness bug = gate fail). Enumerate the usual
     nondeterminism sources to kill: hashmap/iteration order in output, floating-point non-
     associativity under parallel reduction (fix a deterministic reduction order or use
     integer/fixed-point accumulation), thread-scheduling-dependent tie-breaks, uninitialized
     memory. Give a per-track determinism checklist.
  4. OOM & TIMEOUT SAFETY MARGINS: a pre-submit check that peak RSS is comfortably under 8 GB
     with headroom, and wall time is far under each track's 10× timeout; what to do if a run
     OOMs (it's a FAILED run, not slow).
  5. SANDBOX COMPLIANCE: assert no network use, dataset mount never written, exactly one
     output file in /out and nothing else (anticipate the filesystem diff), no precomputed
     answers / no input-recognition short-circuit (DQ-from-event risk) — a reviewable checklist.
  6. OVERFIT GUARD: since final scoring is on an unseen-seed holdout, list what must NOT be
     tuned to specific sample bytes (hash-coded constants, exact-size buffers assuming sample
     cardinality, etc.), and how to use the 1-run-per-15-min sample board without chasing it.
  7. THE PRE-FREEZE CHECKLIST (run in the frozen last 6 hours): per track — SHA green on
     sample ✓, deterministic across N runs ✓, under memory/time limits ✓, Dockerfile builds
     offline from clean checkout ✓, exactly one /out file ✓, PERF.md numbers reproduce ✓,
     README + .speed-demon.toml present ✓, repo public + OSI license ✓, final commit recorded
     before Nov 2 18:00 UTC ✓. Make it a literal checkbox list I can execute.

End with the single highest-leverage piece of advice for a solo competitor across all five
tracks, in two sentences.
```

---

## After Astra answers

- Paste each reply back to me. I verify the numbers against the frozen spec, check the
  honest-claims discipline (no unmeasured number stated as fact), and flag anything that
  would fail a gate before you build on it.
- Once Prompts 1–8 are done and verified, I'll assemble the output into a clean plan in
  **D:\SpeedDemon-Ref** (one master plan file + per-track files) and we iterate from there.
- If you want to go even deeper on any single track before building, say so and I'll write a
  follow-up micro-prompt (e.g. "Track B inverted-index merge loop, exhaustively").

**Legality reminder:** this is design/preparation only, which the rules explicitly permit.
No project code is written or committed before kickoff (Oct 30 18:00 UTC).
