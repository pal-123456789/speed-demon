# SpeedDemon — 72h Master Plan ("Insane Level", solo, all 4 tracks)

> **Honest-claims banner.** Every K, speed score, ratio, and runtime in this plan is a
> **TARGET / EXPECTED** value derived from the frozen scoring math — *not* a measured result.
> The only authoritative numbers come off the scoring box (Falkenstein fsn1) via the local
> harness during the contest. Nothing here is stated as an achieved speedup. Mark each number
> KNOWN only once the harness prints it. This plan already has every inflated/understated
> figure Astra produced corrected (see §2, the Corrections Ledger).

> **Supersession notice — read before executing any track block.**
> The organizers' "Changes before freeze (Oct 26)" list supersedes everything published
> earlier. `docs/specs/` was written after this plan and carries the per-track strategy of
> record. Where the two differ, **the spec wins**, and the differences are deliberate:
>
> 1. **Track E is cut.** Four tracks, A to D. "Track E (Compress) is cut: storing the input
>    raw outscored a real codec." Every Track E block, row and hour in this plan is gone.
> 2. **The scaling axis X is gone**, and its weight moved to wall time. The formula is now
>    `K = (T_ref/T)^0.50 × (M_ref/M)^0.20 × (S_ref/S)^0.15 × (D_ref/D)^0.15` — four axes,
>    no fifth.
> 3. **Track D — Contraction Hierarchies are legal, but this query mix rarely repays
>    them.** The organizers' framing is softer than this plan's. The arithmetic still
>    stands: 60–120 s of preprocessing *inside* the timed run against a ~2.9 s total budget,
>    needing a ~30 ms/query saving over only 2,000 queries — about two orders of magnitude
>    off. The chosen structure is CSR + radix heap + bidirectional + 6–8 ALT landmarks with
>    runtime-adaptive batching. The rejection note stays in PERF.md, with the arithmetic,
>    framed as "legal but uneconomic here" rather than "forbidden".
>
> Nothing else in this plan is superseded. The schedule, the triage, the gate order, the
> stop rules, and the scoring math all stand.

Event: **SpeedDemon** (Hackathon Raptors) — 72h machine-judged performance championship.
Window: **Oct 30 18:00 UTC → Nov 2 18:00 UTC 2026** = **Oct 30 23:30 IST → Nov 2 23:30 IST**.
Entry: **solo, all four tracks (A–D)**. Pool: $1,800. Language per track: **Rust, std-only, D=1**.

---

## 0. The bet (TL;DR)

Three facts decide the whole strategy:

1. **Time dominates and it is uncapped.** T carries 0.50 of the weight and has no
   cap, so a 50× time win is worth `50^0.50 = 7.07` on its own. Memory is capped at
   50× (`50^0.20 = 2.19`); source size and dependencies can cost you but never pay.
   A lone track's realistic ceiling is now **K ≈ 15**, not 9–10. So "insane" is
   **three tracks near their real ceilings**, not one mythical K20.
2. **The 72h cannot hold four deep tracks solo.** Summed realistically the four
   optimization programs need ≈90 h+ before sleep. The plan therefore **triages**:
   go deep on **A, D, C** (real ceilings / safe high floor), ship a **solid**
   Track B.
3. **One gate failure zeroes a track.** SHA-256 mismatch, nondeterminism, OOM, or timeout turns
   every optimization on that track into 0 points. So the order is always **correct + deterministic
   baseline first, optimize second**, re-running the SHA + determinism gates after every change.

Overall prize = geometric mean of your best 3+ track totals → **A, D, C are the overall engine.**
B is still submitted (completeness, highest-K-anywhere) but is not where the deep hours go.

---

## 1. Scoring model (anchors)

- **Score = 65% machine (coefficient K → speed score) + 35% judges** (code quality 15 /
  technique 12 / PERF.md log 8).
- **K = (Tref/T)^0.50 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15** —
  weighted geo-mean, exponents sum to 1.0.
  - T = wall time (0.50, dominates, **no cap**). M = peak memory (0.20, **capped at
    50×**). S = source size (0.15) — code lines counted by `scc` after your formatter,
    across the whole repo minus registry packages first published before Oct 1, 2026
    by people outside the team. D = 1 + the number of direct non-stdlib runtime
    dependencies in your manifest (0.15). **S and D can cost you but never pay**; the
    vendored-bytes term is gone. One dependency ⇒ D = 2 ⇒ contributes 2⁻⁰·¹⁵ = 0.90 to K.
  - **Speed score = 50 × log10(K)**, clamped to 0–100. K = 10 is half marks and
    K = 100 is full marks. The published worked example is **K = 6.41 → speed score
    40.4**.
  - **Verify any K by K = ∏(ref/ours)^weight** — recompute, never trust a stated product.
- **3 gates (pass/fail every run):** SHA-256(output) == ref exactly (nondeterminism = fail);
  **10× ref timeout** (A 321 / B 564 / C 144 / D 727 s); sandbox no-net /
  dataset-ro / root-ro / 1 GB tmpfs, filesystem-diffed (precomputed answers = DQ).
  **The speed-floor gate is gone.**
- **Machine: Falkenstein (fsn1)** — a Hetzner dedicated server in Falkenstein, Germany.
  Intel Core i5-13500, pinned `--cpuset-cpus=0-11` = **6 P-cores / 12
  threads** (no E-cores), turbo off ~2.5 GHz, 64 GB, `--memory=8g --memory-swap=8g`
  (**swap OFF, >8 GB = OOM**), Debian 13 + Docker / cgroups v2. **Build is offline
  and unscored** (PGO/BOLT free); only the run is timed. **Timing = 5 runs, discard
  hi + lo, mean of the middle 3** (trimmed mean).
- **Dockerfile contract:** every `FROM` must name an image from
  `speeddemon.dev/allowlist.json`, pinned by digest; nothing else may enter the image
  (no `COPY --from` of another image, no `ADD` of a URL). Builds stay offline, capped
  at 10 minutes; images capped at 2 GB. The allowlist covers debian:13-slim,
  python:3.13-slim, rust, golang, gcc 15 and 16, Eclipse Temurin 25 (JDK and JRE),
  GraalVM native-image 25, Node 22 and 24, .NET 10, Swift 6.2 and Haskell 9. The
  published example pins
  `debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad`.
- **References (measured on the scoring machine, five runs each):** A 32.05 s ·
  B 56.35 s · C 14.44 s · D 72.69 s. A's reference memory is 228 MB and the reference
  source size is 56 lines. **Datasets:** A 239 MB / 3.1M lines · B 256 MB / 50,000
  queries over a 250 MB corpus · C 204 MB / 3.32M records · D 92 MB / 5M edges.
- **One reference language:** one naive Python reference per track, not Go, Python
  and Rust.
- **Timeline:** Oct 28 18:00 UTC specs+refs+harness frozen · Oct 30 06:00 UTC sample
  datasets + expected hashes · **Oct 30 18:00 UTC kickoff** · **Nov 2 18:00 UTC code
  freeze** · scoring Nov 2–4 (gate pass/fail published by Nov 4) · judging Nov 3–12 ·
  Nov 13 winners. Write-up deadline **Nov 10**. The **live board** closes at code
  freeze (one run per team per 15 minutes, via the Speed Demon Board GitHub App);
  final scoring is on a holdout seed. **PERF.md steps must cite live-board run IDs.**
- **Prizes (total $1,800):** $800 / $400 / $200 overall, $200 for two write-ups, and
  $50 for the highest K on each track. **There is no prize for the smallest source any more.**
  Ties on the final total are broken by raw K, then by wall time, then by earliest submission
  commit.

---

## 2. Corrections Ledger (every Astra number this plan overrides)

Astra produced the eight deep-dives; I verified each against the frozen math. Astra's base/commit
rows were reliable, but its stretch/exceptional rows drifted — **inflated** on A, **understated**
on D and E, plus impossible ceilings and hour overshoots. This plan uses the corrected column.

**The rows below are all recomputed on the current formula** (T 0.50 / M 0.20 / S 0.15 / D 0.15,
speed = 50·log10 K, no scaling axis, T uncapped, M capped at 50×). Astra's K column is the dead
five-axis curve and is not comparable — recompute, never port.

| # | Track / topic | Astra said | Corrected (use this) |
|---|---|---|---|
| 1 | Master §1.7 target-K rows | K 4.94 / 7.96 / 11.0 (old curve) | **K 6.57 / 8.76 / 11.67** (speed 40.9 / 47.1 / 53.4) on the new curve |
| 2 | A §2.3 ladder Safe / Commit | K 5.15 / 6.77 (old curve) | **K 6.57 / 8.76** (speed 40.9 / 47.1) — T 20×/30×, M 10×/15×, S 0.60/0.62 |
| 2 | A §11 ceiling | "K 9–13" | **~15 with T uncapped**; M alone is capped at 50× (factor 2.19) |
| 3 | B time blend | additive-ratio blend (optimistic) | **one total wall time** — the 40/60 build/query split is gone; queries are readable from the start |
| 3 | B §11 ceiling / hours | "K 8–12", "18–22 h" | **~12 with T uncapped**; real block sum **≈ 27 h** |
| 4 | C | (cleanest — no material error) | §2.2 non-plan rows ~1% high, Exceptional conservative; §11 "6–9" within an uncapped-T ceiling |
| 5 | D §2.2 / §2.3 stretch | K 6.14 / 6.9 (old curve) | **K 6.93 / 9.58** (speed 42.0 / 49.1) on the new curve |
| 5 | D §11 "9" | optimistic edge | **T is uncapped, so the old ceiling argument is void**; realistic band K 9–11 |
| 6 | E | (whole track) | **CUT** — "storing the input raw outscored a real codec." No E rows, no E hours |
| 7 | Deliverables: offline build | `vendor/build-tool` placeholder | **debian:13-slim has no rustc + build is `--network=none`** → vendor a pinned rustc or `rustc src/main.rs -O` directly |
| 7 | Deliverables: S & D axes | `vendor/` inside each track dir | **S = code lines by `scc` after the formatter, whole repo minus pre-Oct-1 third-party registry packages**; **D = 1 + direct non-stdlib runtime deps** (vendored-bytes term gone) |
| 8 | Verification §7 "preferred trimmed mean ≤50% of timeout" | reads like a speed target | it is a **DQ-avoidance floor only** (~5× ref time); the real speed target is each track's §2 K budget (single-digit-second runtimes) |
| 8 | Verification "official reruns at >5% spread + reference drift" | stated as official | **unverified — treat as assumption**, not a known rule (keep the local spread check anyway) |

**Recurring Astra tell:** never trust a stated K product — recompute `∏(ref/ours)^w` every time.
Also: Astra's preambles ("I'm saving the file to D:\SpeedDemon-Ref now…") are role-play; it cannot
write to disk. Only this assistant writes to the reference folder.

### 2a. The "Changes before freeze (Oct 26)" sweep — what moved

| Thing | Was | Now |
|---|---|---|
| Tracks | five (A–E) | **four (A–D)**; the fifth track is cut |
| Axes | five: T / M / S / D / X, with the largest weight on time and a tenth of it on scaling | **four: T 0.50 / M 0.20 / S 0.15 / D 0.15** — the scaling axis X is gone, its weight moved to wall time |
| Speed curve | 100·ln(K)/ln(25), K = 25 → 100 | **50·log10(K)**, K = 10 → 50, K = 100 → 100. Worked example **K 6.41 → 40.4** |
| Caps | every axis capped at 50× | **T uncapped; M capped at 50×; S and D can cost but never pay** |
| D axis | `1 + deps + vendored_bytes/50000`, `0.90^deps` | **`D = 1 + direct non-stdlib runtime deps`**; one dep ⇒ D = 2 ⇒ **2⁻⁰·¹⁵ = 0.90** on K |
| S axis | bytes, comments stripped, identifiers normalised | **code lines by `scc` after the formatter, per track folder** (all the build can see), minus pre-Oct-1 third-party registry packages — capped at 1.0 |
| Track B timing | 40% build / 60% query blend | **one total wall time; the queries are readable from the start** |
| Reference language | Go, Python and Rust | **one naive Python reference per track** |
| Machine | the old Finnish host and its EX44 box | **Falkenstein (fsn1)** — same i5-13500, same 64 GB, same limits |
| Gates | four, including a speed-floor gate | **three: correctness, the timeout, the sandbox** |
| Prizes | $2,000, including a $100 prize for the smallest source | **$1,800: $800/$400/$200 overall, $200 for two write-ups, $50 for the highest K on each track** |
| Reference times | A 41.20 s · B 58.70 s · C 36.90 s · D 72.40 s · E 29.80 s | **A 32.05 s · B 56.35 s · C 14.44 s · D 72.69 s** (measured on the scoring machine, five runs each) |
| Timeouts | A 412 / B 587 / C 369 / D 724 / E 298 s | **A 321 / B 564 / C 144 / D 727 s** (10× the new references) |
| Datasets | A 312 MB, B 250 MB corpus, C 204 MB, D 180 MB | **A 239 MB / 3.1M lines · B 256 MB / 50,000 queries over a 250 MB corpus · C 204 MB / 3.32M records · D 92 MB / 5M edges** |
| A reference memory / source | 2,940 MB, 3,180 B | **228 MB, 56 lines** |
| Dockerfile | `FROM debian:13-slim` (a tag) | **allowlist image, digest-pinned; no `COPY --from` of another image, no `ADD` of a URL; build ≤ 10 min, image ≤ 2 GB** |
| Track D hints | preprocessing is the whole game | **queries are local, so parsing and memory layout decide most of the time; CH is legal but rarely repays them** |

---

## 3. EV triage — where the deep hours go

| Tier | Tracks | Why | Hours | Target band |
|---|---|---|---|---|
| **Deep** | **A (Ingest)** | T-dominant and now uncapped, well-understood parsing/aggregation | ~12 (2 base + 10 opt) | **K 9–12** |
| **Deep** | **D (Traverse)** | Highest ref (72.69 s) and a local-query mix where parsing and memory layout decide the time | ~12 (2 + 10) | **K 9–11** |
| **Deep** | **C (Join)** | The DQ track — a safe **high floor** if the CSV state machine is exact | ~10 (2 + 8) | **K 8–9** |
| **Solid** | **B (Index)** | More moving parts; M is its weakest axis | ~7 (2 + 5) | **K 8** |

Overall-prize math (geo-mean of top 3 = A, D, C ≈ 11 / 10 / 9) → K ≈ 9.9 → **speed ≈ 50** before
the 35% judge points. A or D hitting ~12 is the **highest-K-on-a-track ($50)** shot. D=1 everywhere
keeps the D axis at 1.0. The write-up reuses PERF.md for the **$200** write-up prize.

---

## 4. Pre-kickoff prep (Oct 28–30, NOT part of the 72h — design only, legal)

No solution code is written or committed before kickoff. These hours lock the design so H0 is pure
typing:

- **Oct 28 18:00 UTC (specs/refs/harness freeze):** read every frozen spec end to end. **Pin the
  deferred values** this plan currently defers: exact input filenames + the single `/out` filename
  per track; exact output format / rounding / ordering / comparator; Track C's amount scale +
  tie-break + BOM rule; Track D's output contract (distance-only vs path) + unreachable spelling.
- **Resolve the §2 Corrections Ledger against the real refs** — replace every TARGET K with the
  frozen Tref/Mref/Sref so the budgets become real.
- **Build the harness replica + repo skeleton design** (Dockerfiles, `build.sh` with a *pinned
  offline rustc*, `.speed-demon.toml`, SHA-gate hook, cgroup-v2 `memory.peak` reader) — ready to
  type at H0, not committed early.
- **Oct 30 06:00 UTC (sample + hashes):** load the expected sample hashes into
  `tracks/<track>/verification/expected/<output>.sha256` — per track, which is
  where `verify.ps1` and `pre-freeze.ps1` actually read them (there is no
  repo-level `verification/expected/`). Sanity-read the sample bytes (grammar
  only — never tune to them).

---

## 5. The 72-hour schedule (fills the full duration; H0 = Oct 30 18:00 UTC = 23:30 IST)

Doctrine: **all four tracks correct + SHA-green + deterministic before any optimization.** Then deep
ladders on A → D → C, solid B, then a 6-hour frozen lockdown. Three 5-hour sleep blocks
land in local early-morning. Total = 57 h hands-on + 15 h sleep = **72 h, no idle slack.**

| Block | H (UTC off) | IST clock | Work |
|---|---|---|---|
| Scaffold | H0–H2 | Oct 30 23:30–01:30 | Repo, 4 Dockerfiles, harness replica, SHA-gate pre-commit hook, `.speed-demon.toml` stubs. Commit scaffolding first. |
| Baseline C+A | H2–H6 | 01:30–05:30 | **Track C baseline first while freshest** (scalar 4-state CSV machine → SHA-green), then **Track A baseline**. Correctness only. |
| **Sleep 1** | H6–H11 | 05:30–10:30 | 5 h. |
| Baseline D | H11–H13 | 10:30–12:30 | Bidirectional Dijkstra on fwd+rev CSR → SHA-green. |
| Baseline B | H13–H15 | 12:30–14:30 | Authored inverted index + boolean/phrase → SHA-green. |
| Baseline hardening | H15–H17 | 14:30–16:30 | Determinism sweeps (5 runs × 3 worker counts) on all four baselines; first live-board pushes; PERF.md step-0 rows. |
| **Milestone** | **~H17** | **~16:30** | **All 4 tracks SHA-green + deterministic. Tag `baseline-green`.** |
| Commit/meal | H17–H18 | 16:30–17:30 | Commit all baselines; PERF.md step-0 rows. |
| **Deep A** | H18–H28 | 17:30–03:30 | A optimization ladder: custom byte scanner, dense endpoint/hour IDs, SoA, thread-local aggregation, two-level radix percentiles, arena. **Target K 9–12.** |
| **Sleep 2** | H28–H33 | 03:30–08:30 | 5 h. |
| **Deep D** | H33–H43 | 08:30–18:30 | D ladder: tuned bidirectional Dijkstra, ALT landmarks (bounded experiment, 6–8 with per-landmark offsets), and — because the queries are local — parsing and memory layout first. Contraction Hierarchies are **legal but rarely repay this query mix** (see the notice at the top of this document), so they are not on the ladder. **Target K 9–11.** |
| **Deep C** | H43–H51 | 18:30–02:30 | C ladder: SIMD candidate-byte locator feeding the scalar state machine, fixed-point cents, parallel structural pre-scan, differential tester vs hostile corpus. **Target K 8–9.** Correctness first. |
| **Sleep 3** | H51–H56 | 02:30–07:30 | 5 h. |
| **Solid B** | H56–H61 | 07:30–12:30 | B: StreamVByte/PForDelta postings, smallest-first AND, rarest-anchor phrase, **build-side speedups** — one total wall time, so a build win is worth as much as a query win. **Target K 8.** |
| **Extra deep A** | H61–H64 | 12:30–15:30 | Second pass on A's two biggest cost centres (timestamp parse and the per-endpoint aggregation), plus a re-measure of the whole axis table. |
| Buffer | H64–H66 | 15:30–17:30 | Catch up the furthest-behind track; finalize PERF.md, READMEs, dep-count honesty, S-axis source check (whole-repo line count). |
| **LOCKDOWN** | H66–H72 | 17:30–23:30 | **No new experiments.** Pre-freeze checklist ×4 (§9), clean-checkout build+SHA+determinism, record final commit hashes into PERF.md, publish, final commit **before Nov 2 18:00 UTC.** |

**#1 schedule risk:** Track C's baseline is the hardest to get SHA-exact (hostile CSV). If H2–H6
slips, the H17 all-green milestone slips — protect it by keeping C's baseline a *correct scalar*
machine (no SIMD, no cleverness) and deferring all C speed work to the H43 deep block.

---

## 6. Cross-cutting doctrine (applies to all four tracks)

- **Baseline-first green ladder.** A track earns optimization time only after it is SHA-green and
  byte-identical across ≥5 runs. Re-run the SHA + determinism gates after *every* change; a change
  that breaks the gate is reverted, not debugged under time pressure.
- **Honest-claims, always.** PERF.md carries measurements with the exact harness command + commit,
  never adjectives. Label KNOWN / EXPECTED / REVERTED / BLOCKED-BY-FROZEN-HARNESS. No unmeasured
  speedup is ever stated as fact.
- **Determinism is a correctness property.** Kill all nondeterminism sources: no hash-map iteration
  into output (dense IDs or sorted keys), no parallel float reduction (integer/fixed-point, or a
  fixed worker-index reduction order), explicit tie-breakers (never "whoever finished first"), no
  uninitialized bytes, no PID/clock/address-seeded behavior.
- **Anti-overfit (DQ risk).** Never branch on the sample's hash/size/line-count, never emit a stored
  answer, never bake a precomputed table of expected output. Use the *published grammar* (field
  structure, column count, segment boundaries), not memorized sample bytes. Validate on
  grammar-preserving generated fixtures with different values/cardinalities.
- **Sandbox hygiene.** Reads only `/data` (ro), writes exactly one file to `/out`, scratch only in
  `/tmp`, exits 0, leaves an empty filesystem diff. `--read-only` root makes this largely automatic.
- **D=1 everywhere.** Rust standard library only; hand-write every codec/parser/index. With
  `D = 1 + direct non-stdlib runtime deps`, one dependency costs 2⁻⁰·¹⁵ = 0.90 on K — 10% of
  the score, for free, by not adding one.
- **mmap vs cgroup caution.** A memory-mapped input file can be charged to `memory.peak`. Where the M
  axis matters (A, B), prefer a buffered streaming read over mmap and measure both.

---

## 7. Deliverables system (judge 35%) — corrected

- **Monorepo**, public at freeze with a root OSI license. One directory per track
  (`track-a` … `track-d`), each a self-contained build context: `Dockerfile`, `build.sh`,
  `.speed-demon.toml`, `README.md`, `PERF.md`, `src/`, `verification/expected/`. No shared
  core crate. A judge should understand a track without leaving its directory.
- **Dockerfile:** every `FROM` must name an image from `speeddemon.dev/allowlist.json`, **pinned by
  digest** (the published example is
  `debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad`), and
  **nothing else may enter the image**: no `COPY --from` of another image, no `ADD` of a URL.
  `build.sh` runs with **no network** (no apt/cargo-fetch/pip/curl). The build stage is the
  digest-pinned, allowlisted `rust:1-slim-trixie` image — it ships rustc plus gcc/libc6-dev
  for linking, so nothing is vendored — and `./build.sh` compiles `src/main.rs` directly with
  `rustc`, no cargo. PGO/BOLT allowed (build unscored). Builds are capped at
  **10 minutes** and the image at **2 GB**. The binary reads fixed `/data` filenames, writes exactly one
  `/out` file, exits 0, leaves nothing else. **Byte-identical copies of the published generators and
  references are free** — so copying a generator in for build-time training costs nothing.
- **PERF.md (8%)** — the strongest judge lever: header with per-axis ratios + K + speed score (local,
  "official scoring is the organizer's"), then the optimization ladder as a **measurements table**
  (step, commit, change, before→after on the measured axis, verdict, **live-board run ID**) and a
  dedicated **"experiments that failed / were reverted"** table (organizers reward these). Every
  number reproducible by the harness.
- **README** per track: plain-prose approach, dependency list ("none, D=1"), and the required
  **"what I'd do with another 24 hours"** section.
- **Code for judges:** anonymized, no board shown — the code must explain *why* it's fast. Comment
  every non-obvious trick and every invariant; separate algorithm from output formatting; no dead
  experiment code in the production path. Dense is fine; unexplainable is not.
- **Write-up ($200, two prizes):** the optimization-journey post (the misread profile, the rewrite
  that got slower, the two-line change that halved it), tag Hackathon Raptors, due **Nov 10**, reuse
  PERF.md.
- **demo.mp4:** unscored — **go only after** all four tracks are frozen and the packet is done, and
  only if it costs <60 min. Never let it eat a sleep or verification block.

---

## 8. Verification & foolproofing capstone — corrected

- **Local scoring-box replica** (relative numbers only — the laptop is not the fsn1 scoring box):
  ```
  docker run --rm --cpuset-cpus=0-11 --cpuset-mems=0 --memory=8g --memory-swap=8g \
    --pids-limit=512 --network=none --read-only --tmpfs /tmp:rw,size=1g \
    --ulimit nofile=8192:8192 -v DATA:/data:ro -v OUT:/out:rw -e TRACK=X IMAGE
  ```
  Confirm effective limits via `docker inspect` (Memory=8589934592, MemorySwap=8589934592,
  Readonly=true). **Do not rely on `/tmp` being `exec`** — never execute from `/tmp`.
- **Peak memory** = cgroup v2 `memory.peak` (try `/sys/fs/cgroup/docker/<id>/memory.peak` then
  `/sys/fs/cgroup/system.slice/docker-<id>.scope/memory.peak`). **Never substitute `ps`/`time` RSS.**
- **Timing** = 5 runs, discard hi+lo, mean of middle 3 (trimmed mean). Reject the batch on any
  nonzero exit, SHA mismatch, differing hashes, timeout, missing peak-memory, or >1 file in `/out`.
  Keep a local >5% spread re-run as a stability check (its "official" status is unverified).
- **SHA gate automation** wired into a pre-commit hook: every optimization commit re-runs the sample
  SHA gate for the affected track; integration commits run all four + determinism + offline build.
- **Determinism:** ≥5 runs (10 for the final candidate) asserting byte-identical output; per-track
  checklists in the track files (§ each).
- **OOM/timeout = failed run, never "slow."** Internal safety floors: peak ≤ 80% of 8 GiB; worst run
  ≤ 70% of timeout. **These floors are DQ-avoidance only — the actual speed target is each track's
  §2 K budget (single-digit-second runtimes, 5–10× under the reference), far below these floors.**
  Never "fix" an OOM with swap (disabled); fix it structurally (stream, compact int widths, release
  arenas). If a candidate can't be proven safe, ship the last known-good faster baseline.
- **Timeouts (hard kill):** A 321 · B 564 · C 144 · D 727 s (= 10× the reference).
- **Pre-freeze checklist (run in the frozen last 6 h, per track):** offline build from clean checkout
  ✓ · sample SHA green ✓ · byte-identical ×10 ✓ · peak mem + time under floors ✓ · exactly one
  `/out` file ✓ · empty filesystem diff ✓ · PERF.md numbers reproduce ✓ · README + `.speed-demon.toml`
  present, dep count honest ✓ · repo public + OSI license ✓ · final commit recorded before Nov 2
  18:00 UTC ✓. Then the **clean-checkout test**: `git clone` the public repo, checkout the final
  commit, `docker build --network=none` all four, re-run SHA/determinism/sandbox from that tree.

**Highest-leverage move (Astra's capstone, endorsed):** freeze a correct, deterministic baseline for
every track before spending hours on speed — one gate failure zeroes all of a track's gains — then
attack the bottleneck with the largest weighted contribution, re-running SHA + determinism after
every change.

---

## 9. Prizes targeted & stop rules

The pool is **$1,800** in total.

- **$800 overall** (geo-mean of your best three track totals) → A + D + C deep.
- **$400 / $200 runner-up / third** → upside of the same three deep tracks.
- **$200 for two write-ups** → PERF.md journey post by Nov 10.
- **$50 for the highest K on each track** → push A or D toward ~12.
- **There is no prize for the smallest source any more** — so the S axis is priced as a cost and
  nothing more.

**Stop rules:** (1) a track is "done for now" at its target K band — bank it and move on rather than
chase a brittle +0.3. (2) Any optimization that can't hold the SHA/determinism gate is reverted
immediately. (3) Protect the three sleep blocks and the 6-hour lockdown — they are not negotiable
buffer. (4) At H64 freeze scope: whatever isn't green stays at its last green baseline.

---

## 10. Per-track build files

Detailed, corrected build guides (algorithm, data layout, optimization ladder, determinism
checklist, memory budget, risks) live beside this file:

- `track-a-ingest.md` · `track-b-index.md` · `track-c-join.md` · `track-d-traverse.md`
- **INSANE-LEVEL depth arsenal** — per-track feature inventories (P0/P1/P2), the
  optimization ladder, determinism traps, edge-case matrices, and the H0 runbooks:
  `docs/depth/00-INDEX.md` and `docs/depth/track-{A,B,C,D}-*-DEEP.md`.
- Full Astra deep-dive prompts + the frozen CONTEXT BLOCK: `SpeedDemon-Astra-prompt-pack.md`.

*All numbers here are TARGET/EXPECTED until the harness measures them on the scoring box.*
