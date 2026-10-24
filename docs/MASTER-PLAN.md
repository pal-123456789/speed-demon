# SpeedDemon — 72h Master Plan ("Insane Level", solo, all 5 tracks)

> **Honest-claims banner.** Every K, speed score, ratio, and runtime in this plan is a
> **TARGET / EXPECTED** value derived from the frozen scoring math — *not* a measured result.
> The only authoritative numbers come off the HEL1 box via the local harness during the contest.
> Nothing here is stated as an achieved speedup. Mark each number KNOWN only once the harness
> prints it. This plan already has every inflated/understated figure Astra produced corrected
> (see §2, the Corrections Ledger).

> **Supersession notice — read before executing any track block.**
> `docs/specs/` was written after this plan and carries the per-track strategy of record.
> Where the two differ, **the spec wins**, and the differences are deliberate:
>
> 1. **Track D — Contraction Hierarchies are REJECTED**, not "time-boxed with hard abort
>    gates" as §5 of this plan has it. The amortization arithmetic kills it: 60–120 s of
>    preprocessing *inside* the timed run against a 2.9 s total budget, needing a
>    ~30 ms/query saving over only 2,000 queries — about two orders of magnitude off.
>    The plan's "time-boxed" framing would burn the first hours of the Deep D block on an
>    experiment whose break-even cannot be reached. The chosen structure is CSR + radix
>    heap + bidirectional + 6–8 ALT landmarks with runtime-adaptive batching.
> 2. **Track E is speed-first, not ratio-first.** The M axis is `our_ratio / 2.41`, and
>    across the entire realistic band (2.6×–4.0×) it spans only ≈2.2 points while T spans
>    ≈13. Target ratio 3.1–3.4; never chase the last 5%.
> 3. **The K anchors in this plan's ladder tables match the specs** (A Safe 4.87 / Commit
>    6.27) — those were reconciled already. The Deep D "Target K 7" reads as the Stretch
>    (6.69) → Exceptional (7.51) band in the spec.
>
> Nothing else in this plan is superseded. The schedule, the triage, the gate order, the
> stop rules, and the scoring math all stand.

Event: **SpeedDemon** (Hackathon Raptors) — 72h machine-judged performance championship.
Window: **Oct 30 18:00 UTC → Nov 2 18:00 UTC 2026** = **Oct 30 23:30 IST → Nov 2 23:30 IST**.
Entry: **solo, all five tracks (A–E)**. Pool: $2,000. Language per track: **Rust, std-only, D=1**.

---

## 0. The bet (TL;DR)

Three facts decide the whole strategy:

1. **The 50× per-axis cap makes a single track's realistic ceiling K ≈ 9–10, not 15+.** T caps at
   50^0.40 = 4.78, M at 50^0.20 = 2.19. Even with time *and* memory near the cap a lone track
   tops out around K≈10. So "insane" is **three tracks near their real ceilings**, not one mythical
   K20.
2. **The 72h cannot hold five deep tracks solo.** Summed realistically the five optimization
   programs need ≈108h+ before sleep. The plan therefore **triages**: go deep on **A, D, C**
   (real ceilings / safe high floor), ship a **solid** Track B, and a **correct, decent** Track E
   (lowest ceiling — do not over-invest).
3. **One gate failure zeroes a track.** SHA-256 mismatch, nondeterminism, OOM, or timeout turns
   every optimization on that track into 0 points. So the order is always **correct + deterministic
   baseline first, optimize second**, re-running the SHA + determinism gates after every change.

Overall prize = geometric mean of your best 3+ track totals → **A, D, C are the overall engine.**
B and E are still submitted (completeness, highest-K-anywhere, lean-prize shots) but are not where
the deep hours go.

---

## 1. Scoring model (anchors)

- **Score = 65% machine (coefficient K → speed score) + 35% judges** (code quality 15 /
  technique 12 / PERF.md log 8).
- **K = (Tref/T)^0.40 × (Mref/M)^0.20 × (Sref/S)^0.15 × (Dref/D)^0.15 × (Xref/X)^0.10** —
  weighted geo-mean, exponents sum to 1.0, **each axis capped at 50×**.
  - T = wall time (0.40, dominates). M = peak memory, or **compressed size on Track E** (0.20).
    S = canonical source size after formatter + normalized identifiers (0.15). D = 1 + deps +
    vendored_bytes/50k (0.15; **zero-dep D=1 is a free ~10–17%**). X = 10×/1× scaling (0.10).
  - **Speed score = 100·ln(K)/ln(25)**, clamped. ln25 = 3.218876. K25→100, K5→50, K<1→0.
  - **Verify any K by K = ∏(ref/ours)^weight** — recompute, never trust a stated product.
- **4 gates (pass/fail every run):** SHA-256(output) == ref exactly (nondeterminism = fail);
  speed floor within 10× of fastest; **10× ref timeout** (A 412 / B 587 / C 369 / D 724 / E 298 s);
  sandbox no-net / dataset-ro / root-ro / 1 GB tmpfs, filesystem-diffed (precomputed answers = DQ).
- **Machine HEL1 (Hetzner EX44):** i5-13500, pinned `--cpuset-cpus=0-11` = **6 P-cores / 12
  threads** (no E-cores), turbo off ~2.5 GHz, `--memory=8g --memory-swap=8g` (**swap OFF, >8 GB =
  OOM**), Debian 13 + Docker / cgroups v2. **Build is offline and unscored** (PGO/BOLT free); only
  the run is timed. **Timing = 5 runs, discard hi + lo, mean of the middle 3** (trimmed mean).
- **Timeline:** Oct 28 18:00 UTC specs+refs+harness frozen · Oct 30 06:00 UTC sample+hashes ·
  **Oct 30 18:00 UTC kickoff** · Oct 31 reference-teardown posted · **Nov 2 18:00 UTC freeze** ·
  Nov 13 winners. Write-up side quest due **Nov 10**.

---

## 2. Corrections Ledger (every Astra number this plan overrides)

Astra produced the eight deep-dives; I verified each against the frozen math. Astra's base/commit
rows were reliable, but its stretch/exceptional rows drifted — **inflated** on A, **understated**
on D and E, plus impossible ceilings and hour overshoots. This plan uses the corrected column.

| # | Track / topic | Astra said | Corrected (use this) |
|---|---|---|---|
| 1 | Master §1.7 target-K rows | K 4.94 / 7.96 / 11.0 | **K 4.02 / 5.83 / 7.71** (speed 43.2 / 54.8 / 63.5) — K11 exceeds the cap |
| 2 | A §2.3 ladder Safe / Commit | K 5.15 / 6.77 | **K 4.87 / 6.27** (Astra under-applied the S^0.15 penalty). Headline K8.05 was correct |
| 2 | A §11 ceiling | "K 9–13" | **impossible — cap ≈ 9**; strike 13 |
| 3 | B time blend | additive-ratio blend (optimistic) | **harmonic/weighted-time blend** — build (40%) is the harder phase; **prioritize build speedups** |
| 3 | B §11 ceiling / hours | "K 8–12", "18–22 h" | **cap ≈ 10** (M is B's weakest axis); real block sum **≈ 27 h** |
| 4 | C | (cleanest — no material error) | §2.2 non-plan rows ~1% high, Exceptional conservative; §11 "6–9" within cap |
| 5 | D §2.2 / §2.3 stretch | K 6.14 / 6.9 | **K 6.69 / 7.51** (Astra ~8% understated); commit 5.04 correct |
| 5 | D §11 "9" | optimistic edge | **within the ~9.7 cap — not impossible**; realistic band K 5–7 |
| 6 | E §2 competitive M-axis | M = 2.8× → K 3.12 | §5's own budget supports **M ≈ 1.27×** → **competitive K ≈ 2.66**; realistic band **K 2.7–3.5** |
| 6 | E §2 stretch | K 4.2 | **K 4.43** (ratios); still gated by lowest-ceiling reality |
| 6 | E §11 hours | "14–16 h" | block sum **≈ 19 h** |
| 7 | Deliverables: offline build | `vendor/build-tool` placeholder | **debian:13-slim has no rustc + build is `--network=none`** → vendor a pinned rustc or `rustc src/main.rs -O` directly |
| 7 | Deliverables: S & D axes | `vendor/` inside each track dir | **exclude the build toolchain from the S-axis source scan and D-axis dep count** or it inflates S / miscounts D |
| 8 | Verification §7 "preferred trimmed mean ≤50% of timeout" | reads like a speed target | it is a **DQ-avoidance floor only** (~5× ref time); the real speed target is each track's §2 K budget (single-digit-second runtimes) |
| 8 | Verification "official reruns at >5% spread + reference drift" | stated as official | **unverified — treat as assumption**, not a known rule (keep the local spread check anyway) |

**Recurring Astra tell:** never trust a stated K product — recompute `∏(ref/ours)^w` every time.
Also: Astra's preambles ("I'm saving the file to D:\SpeedDemon-Ref now…") are role-play; it cannot
write to disk. Only this assistant writes to the reference folder.

---

## 3. EV triage — where the deep hours go

| Tier | Tracks | Why | Hours | Target band |
|---|---|---|---|---|
| **Deep** | **A (Ingest)** | Real ceiling ~9, T-dominant, well-understood parsing/aggregation | ~12 (2 base + 10 opt) | **K 7–8** |
| **Deep** | **D (Traverse)** | Highest ref (72.4 s) + highest cap (~9.7); preprocessing-in-timed-run is rich | ~12 (2 + 10) | **K 7** |
| **Deep** | **C (Join)** | The DQ track — a safe **high floor** if the CSV state machine is exact | ~10 (2 + 8) | **K 6–7** |
| **Solid** | **B (Index)** | More moving parts; M is its weakest axis | ~7 (2 + 5) | **K 6** |
| **Baseline+** | **E (Compress)** | **Lowest ceiling** (M only 0.20 wt, ref already 2.41×, fast-decoder-capped T) | ~5 (2 + 3) | **K 3–3.5** |

Overall-prize math (geo-mean of top 3 = A, D, C ≈ 7 / 7 / 6.5) → K ≈ 6.8 → **speed ≈ 60** before the
35% judge points. A or D hitting ~8 is the **highest-K-anywhere ($200)** shot. D=1 everywhere is the
**lean ($100)** shot. The write-up reuses PERF.md for the **$300** side quest.

---

## 4. Pre-kickoff prep (Oct 28–30, NOT part of the 72h — design only, legal)

No solution code is written or committed before kickoff. These hours lock the design so H0 is pure
typing:

- **Oct 28 18:00 UTC (specs/refs/harness freeze):** read every frozen spec end to end. **Pin the
  deferred values** this plan currently defers: exact input filenames + the single `/out` filename
  per track; exact output format / rounding / ordering / comparator; Track B's exact build/query
  blend formula + Bref/Qref/Mref; Track C's amount scale + tie-break + BOM rule; Track D's output
  contract (distance-only vs path) + unreachable spelling; Track E's Mref + container wrapper +
  segment geometry + compression/decompression time blend.
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

Doctrine: **all five tracks correct + SHA-green + deterministic before any optimization.** Then deep
ladders on A → D → C, solid B, targeted E, then a 6-hour frozen lockdown. Three 5-hour sleep blocks
land in local early-morning. Total = 57 h hands-on + 15 h sleep = **72 h, no idle slack.**

| Block | H (UTC off) | IST clock | Work |
|---|---|---|---|
| Scaffold | H0–H2 | Oct 30 23:30–01:30 | Repo, 5 Dockerfiles, harness replica, SHA-gate pre-commit hook, `.speed-demon.toml` stubs. Commit scaffolding first. |
| Baseline C+A | H2–H6 | 01:30–05:30 | **Track C baseline first while freshest** (scalar 4-state CSV machine → SHA-green), then **Track A baseline**. Correctness only. |
| **Sleep 1** | H6–H11 | 05:30–10:30 | 5 h. |
| Baseline D | H11–H13 | 10:30–12:30 | Bidirectional Dijkstra on fwd+rev CSR → SHA-green. |
| Baseline B | H13–H15 | 12:30–14:30 | Authored inverted index + boolean/phrase → SHA-green. |
| Baseline E | H15–H17 | 14:30–16:30 | Raw-block container + minimal rANS, exact round-trip → SHA-green. |
| **Milestone** | **~H17** | **~16:30** | **All 5 tracks SHA-green + deterministic. Tag `baseline-green`.** |
| Commit/meal | H17–H18 | 16:30–17:30 | Commit all baselines; PERF.md step-0 rows. |
| **Deep A** | H18–H28 | 17:30–03:30 | A optimization ladder: custom byte scanner, dense endpoint/hour IDs, SoA, thread-local aggregation, two-level radix percentiles, arena. **Target K 7–8.** |
| **Sleep 2** | H28–H33 | 03:30–08:30 | 5 h. |
| **Deep D** | H33–H43 | 08:30–18:30 | D ladder: tuned bidirectional Dijkstra, ALT landmarks (bounded experiment, 6–8 with per-landmark offsets), preprocessing break-even inside the timed run. Contraction Hierarchies are **rejected** — see the notice at the top of this document — and are not on the ladder. **Target K 7.** |
| **Deep C** | H43–H51 | 18:30–02:30 | C ladder: SIMD candidate-byte locator feeding the scalar state machine, fixed-point cents, parallel structural pre-scan, differential tester vs hostile corpus. **Target K 6–7.** Correctness first. |
| **Sleep 3** | H51–H56 | 02:30–07:30 | 5 h. |
| **Solid B** | H56–H61 | 07:30–12:30 | B: StreamVByte/PForDelta postings, smallest-first AND, rarest-anchor phrase, **build-side speedups (harmonic-blend fix)**. **Target K 6.** |
| **Targeted E** | H61–H64 | 12:30–15:30 | E: the single biggest ratio win (logs field/delta dict reusing the Track A tokenizer) + a fast parallel rANS decoder. **Target K 3–3.5.** |
| Buffer | H64–H66 | 15:30–17:30 | Catch up the furthest-behind track; finalize PERF.md, READMEs, dep-count honesty, S-axis source check (exclude toolchain). |
| **LOCKDOWN** | H66–H72 | 17:30–23:30 | **No new experiments.** Pre-freeze checklist ×5 (§9), clean-checkout build+SHA+determinism, record final commit hashes into PERF.md, publish, final commit **before Nov 2 18:00 UTC.** |

**#1 schedule risk:** Track C's baseline is the hardest to get SHA-exact (hostile CSV). If H2–H6
slips, the H17 all-green milestone slips — protect it by keeping C's baseline a *correct scalar*
machine (no SIMD, no cleverness) and deferring all C speed work to the H43 deep block.

---

## 6. Cross-cutting doctrine (applies to all five tracks)

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
- **D=1 everywhere.** Rust standard library only; hand-write every codec/parser/index. This is a free
  ~10–17% on the D axis and the lean-prize shot.
- **mmap vs cgroup caution.** A memory-mapped input file can be charged to `memory.peak`. Where the M
  axis matters (A, B), prefer a buffered streaming read over mmap and measure both.

---

## 7. Deliverables system (judge 35%) — corrected

- **Monorepo**, public at freeze with a root OSI license. One directory per track
  (`tracks/A-ingest` … `tracks/E-compress`), each self-contained: `Dockerfile`, `build.sh`, `run`,
  `.speed-demon.toml`, `README.md`, `PERF.md`, `src/`, `tests/`. Shared `core/` only for code used by
  ≥2 tracks. A judge should understand a track without leaving its directory.
- **Dockerfile:** `FROM debian:13-slim`; `build.sh` runs with **no network** (no apt/cargo-fetch/
  pip/curl) — **the Rust toolchain must already be in the build context** (vendor a pinned rustc, or
  compile std-only with `rustc src/main.rs -O`, no cargo). PGO/BOLT allowed (build unscored).
  `run` reads fixed `/data` filenames, writes exactly one `/out` file, exits 0, leaves nothing else.
  **Keep the vendored toolchain out of whatever the S-axis / dep-count tools scan.**
- **PERF.md (8%)** — the strongest judge lever: header with per-axis ratios + K + speed score (local,
  "official scoring is the organizer's"), then the optimization ladder as a **measurements table**
  (step, commit, change, before→after on the measured axis, verdict) and a dedicated **"experiments
  that failed / were reverted"** table (organizers reward these). Every number reproducible by the
  harness.
- **README** per track: plain-prose approach, dependency list ("none, D=1"), and the required
  **"what I'd do with another 24 hours"** section.
- **Code for judges:** anonymized, no board shown — the code must explain *why* it's fast. Comment
  every non-obvious trick and every invariant; separate algorithm from output formatting; no dead
  experiment code in the production path. Dense is fine; unexplainable is not.
- **Write-up ($300):** the optimization-journey post (the misread profile, the rewrite that got
  slower, the two-line change that halved it), tag Hackathon Raptors, due **Nov 10**, reuse PERF.md.
- **demo.mp4:** unscored — **go only after** all five tracks are frozen and the packet is done, and
  only if it costs <60 min. Never let it eat a sleep or verification block.

---

## 8. Verification & foolproofing capstone — corrected

- **Local scoring-box replica** (relative numbers only — the laptop is not HEL1):
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
  SHA gate for the affected track; integration commits run all five + determinism + offline build.
- **Determinism:** ≥5 runs (10 for the final candidate) asserting byte-identical output; per-track
  checklists in the track files (§ each).
- **OOM/timeout = failed run, never "slow."** Internal safety floors: peak ≤ 80% of 8 GiB; worst run
  ≤ 70% of timeout. **These floors are DQ-avoidance only — the actual speed target is each track's
  §2 K budget (single-digit-second runtimes, 5–10× under the reference), far below these floors.**
  Never "fix" an OOM with swap (disabled); fix it structurally (stream, compact int widths, release
  arenas). If a candidate can't be proven safe, ship the last known-good faster baseline.
- **Timeouts (hard kill):** A 412 · B 587 · C 369 · D 724 · E 298 s (= 10× ref). Verified correct.
- **Pre-freeze checklist (run in the frozen last 6 h, per track):** offline build from clean checkout
  ✓ · sample SHA green ✓ · byte-identical ×10 ✓ · peak mem + time under floors ✓ · exactly one
  `/out` file ✓ · empty filesystem diff ✓ · PERF.md numbers reproduce ✓ · README + `.speed-demon.toml`
  present, dep count honest ✓ · repo public + OSI license ✓ · final commit recorded before Nov 2
  18:00 UTC ✓. Then the **clean-checkout test**: `git clone` the public repo, checkout the final
  commit, `docker build --network=none` all five, re-run SHA/determinism/sandbox from that tree.

**Highest-leverage move (Astra's capstone, endorsed):** freeze a correct, deterministic baseline for
every track before spending hours on speed — one gate failure zeroes all of a track's gains — then
attack the bottleneck with the largest weighted contribution, re-running SHA + determinism after
every change.

---

## 9. Prizes targeted & stop rules

- **$800 overall** (geo-mean of best 3) → A + D + C deep.
- **$200 highest-K anywhere** → push A or D toward ~8.
- **$100 lean** → D=1 across all five.
- **$300 write-up** → PERF.md journey post by Nov 10.
- **$400 / $200 runner-up/third** → upside of the same three deep tracks.

**Stop rules:** (1) a track is "done for now" at its target K band — bank it and move on rather than
chase a brittle +0.3. (2) Any optimization that can't hold the SHA/determinism gate is reverted
immediately. (3) Protect the three sleep blocks and the 6-hour lockdown — they are not negotiable
buffer. (4) At H64 freeze scope: whatever isn't green stays at its last green baseline.

---

## 10. Per-track build files

Detailed, corrected build guides (algorithm, data layout, optimization ladder, determinism
checklist, memory budget, risks) live beside this file:

- `track-A-ingest.md` · `track-B-index.md` · `track-C-join.md` · `track-D-traverse.md` ·
  `track-E-compress.md`
- Full Astra deep-dive prompts + the frozen CONTEXT BLOCK: `SpeedDemon-Astra-prompt-pack.md`.

*All numbers here are TARGET/EXPECTED until the HEL1 harness measures them.*
