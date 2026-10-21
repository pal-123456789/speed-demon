# Speed Demon — Campaign Status & Runbook

> **Read this first.** It is the operational dashboard for the pre-kickoff
> build and the runbook for the 72-hour window.
>
> **Status: PRE-KICKOFF SCAFFOLD — green infrastructure, stub implementations.**
> The machine is ready. The *implementations* are not, by design: rules 04/05
> require all project code to be written inside the 72-hour window
> (Oct 30 18:00 UTC → Nov 2 18:00 UTC).
>
> Last updated: pre-kickoff build complete.

---

## 1. What is built and proven

| Component | State | Evidence |
|---|---|---|
| Workspace (5 tracks + shared core) | **GREEN** | `cargo build --offline` from a clean clone — passes |
| Core library (13 unit tests) | **GREEN** | 13/13 pass: arena, interner, chunking, pool, cents, float bits |
| **`verification/harness`** — the scoring-box replica | **GREEN** | 12/12 tests: pure-Rust SHA-256 (known vectors), cgroup v2 peak reader, JSONL ledger, lockdown, contract table |
| `rustfmt` + `clippy -D warnings` | **GREEN** | pre-commit hook enforces both |
| Offline build path (`rustc`, no cargo, no network) | **GREEN** | `build.sh` / `build-all.sh` compile core + 5 binaries with `--extern` |
| One-image-five-tracks dispatch on `$TRACK` | **GREEN** | `run` + root `Dockerfile` + `.dockerignore` |
| 4-gate verifier (SHA / determinism / timeout / sandbox) | **GREEN** | Rust harness (Linux) + `verify.ps1` (Windows) — same checklist, two hosts |
| 5-run trimmed-mean benchmark | **GREEN** | `harness bench` + `bench.ps1` |
| Pre-freeze lockdown (81 checks) | **GREEN** | `pre-freeze.ps1` (Windows) + `harness lockdown` (Linux) |
| **Rule 05 compliance guard** | **GREEN** | `tooling/rule05_guard.py` — proved to block a planted violation |
| **Measurement ledger → generated PERF.md** | **GREEN** | every measurement is a JSONL row; tables are generated, not remembered |
| **Differential tester** | **GREEN** | `tooling/difftest.py` — pinpoints byte/line/column of a hash mismatch |
| **Per-track design specs** | **GREEN** | `docs/specs/*.md` — 5 × ~290 lines, decision-complete, no solution code |
| **Offline PGO pipeline** | **GREEN** | `tooling/pgo-build.sh` — free T-axis win, output-equivalence checked |
| One-command CI | **GREEN** | `sh tooling/ci.sh` — 10 gates, cheapest-fails-first |
| K calculator | **GREEN** | reproduces the published Nightshade example exactly (K 6.2569, speed 56.97, total 65.73) |
| S/D axis measurement | **GREEN** | **D = 1.0000 on all five tracks** |
| Anti-overfit fixture generator | **GREEN** | grammar-preserving data + 11 hand-crafted hostile CSV cases |
| Track implementations (A–E) | **STUB** | scaffold sentinel present — *correct* pre-kickoff state |

### The open FAILs, and why each is correct right now

| FAIL | Why it is correct now | When it clears |
|---|---|---|
| 5× "no scaffold sentinel in source" | The implementations do not exist yet. The check is doing its job: **a stub can never reach the freeze.** | H0–H17, as each track's real implementation lands |
| "working tree clean" | Uncommitted verification-script fixes | immediately, on the next commit |
| "public remote configured" | There is no public GitHub repo yet | before the freeze — push to a public repo under an OSI licence |

**Two SKIPs, both informational:** "sample SHA-256 green" cannot be checked
before the Oct 30 06:00 UTC sample ships, and the per-track reference source
sizes for B–E are not published yet.

---

## 1b. The five decisions that are already locked

These are made, with the arithmetic behind them, in `docs/specs/`. They are
locked **before** the window opens so that H0 is typing, not deciding.

| # | Decision | Why it is locked |
|---|---|---|
| 1 | **Contraction Hierarchies are REJECTED for Track D** | CH preprocessing costs 60–120 s *inside* the timed run, against a Commit budget of 2.9 s total. Amortised over only 2,000 queries it needs a ~30 ms/query saving to break even — about two orders of magnitude off. Chosen instead: CSR + radix heap + bidirectional + 6–8 ALT landmarks, with runtime-adaptive batching by distinct source/target. |
| 2 | **Track B is deliberately blend-robust** | The published build/query blend (40/60) is DEFERRED and may be additive, harmonic or geometric. Balanced bets (β = q = 30×) survive all three; unbalanced bets swing the result by up to 2.1×. Query speedup carries ~4.5× the marginal leverage of build speedup under the additive blend. |
| 3 | **Track E is speed-first, not ratio-first** | The M axis is `our_ratio / 2.41`, and across the entire realistic ratio band (2.6×–4.0×) the M factor only spans 1.0153–1.1066 ≈ **2.2 points**, while the T axis spans ~13. Trading 10% of ratio for 2× compression speed is **+3.2 points net**. Target ratio 3.1–3.4; never chase the last 5%. |
| 4 | **Source bytes are bought for speed, globally** | Doubling the S axis costs 2.1 points; a 1.5× T gain is worth 3.3. So writing more code is a *net win* everywhere — which is why every plan targets a larger source footprint than the reference, not a smaller one. |
| 5 | **D = 1 is a free axis and must be defended** | 0.15 of the weight, ~10–17% of K, for std-only Rust with zero vendored bytes. The rule-05 guard and the D-axis checks exist to keep it that way. |

**One risk already identified:** if the page cache is charged to the memory
cgroup on HEL1, Track A's M ratio collapses to ~8.6× and K drops ~2 points.
There is a scheduled H1 measurement for exactly this, because it changes the
memory design of the biggest track.

---

## 2. The contract, pinned and unpinned

`verification/contract.toml` is the single source of truth. Every value the
plan could not honestly know is listed there as `DEFERRED` rather than
invented. On **Oct 28 18:00 UTC** (specs + harness freeze) fill them in and
every K budget in the plan becomes real.

**Pinned already (from the public event page):**

* Scoring: `K = ∏(ref/ours)^w`, weights `T .40 / M .20 / S .15 / D .15 / X .10`,
  every axis capped at 50×, `speed = 100·ln(K)/ln(25)`, track total
  `0.65·speed + 0.35·judges`, overall = geometric mean of ≥3 tracks.
* Reference times: A 41.20 s · B 58.70 s · C 36.90 s · D 72.40 s · E 29.80 s.
* Timeouts: A 412 · B 587 · C 369 · D 724 · E 298 s.
* Machine: i5-13500, `--cpuset-cpus=0-11` (6 P-cores / 12 threads), turbo off,
  `--memory=8g --memory-swap=8g` (**swap off**), `cgroup v2 memory.peak`.
* Run protocol: 5 runs, discard hi+lo, mean of middle 3; >5% spread re-runs;
  >2% reference drift rescores the batch.
* Docker contract: read `/data`, write exactly one file to `/out`, exit 0,
  build with `--network=none`.

**Still DEFERRED — pin these on Oct 28:**

| Track | The decision that changes the most work |
|---|---|
| A | output ordering, percentile rank definition, error-rate format, group key |
| B | **the build/query blend formula** (additive vs harmonic — a 24% swing), boolean operator precedence, tokenisation |
| C | quoting rules for embedded newlines, mid-file BOM semantics, amount scale, aggregate key |
| D | distance-only vs path output, unreachable spelling, max distance bound |
| E | **Mref for the compressed size axis**, the time blend (0.8/0.2?), and the container format — because *the compressed file is the hashed artefact* |

---

## 3. Runbook

### Before Oct 28 (now) — done

```bash
cargo build --release --offline        # builds
cargo test --offline                   # 13/13
sh tooling/install-hooks.sh            # installs the pre-commit gate
```

### Oct 28 18:00 UTC — spec freeze

1. Read all five frozen specs end to end.
2. Fill `verification/contract.toml` — every `DEFERRED` becomes a fact.
3. Replace the TARGET K budgets in `docs/MASTER-PLAN.md` with the real
   `Tref/Mref/Sref`.
4. Load the sample expected hashes into
   `tracks/<track>/verification/expected/<output>.sha256` (Oct 30 06:00 UTC).

### Oct 30 06:00 UTC — sample data

```bash
# pin the expected hashes (one line each: "<sha256>  <output-file>")
echo "<sha256>  ingest.txt" > tracks/A-ingest/verification/expected/ingest.txt.sha256

# and confirm the gate is live
powershell -File verification/verify.ps1 -Track A
```

### Oct 30 18:00 UTC — H0, the window opens

Order of work (from `docs/MASTER-PLAN.md` §5):

| Block | H | Work |
|---|---|---|
| Scaffold | H0–H2 | repo, Dockerfiles, harness replica, SHA hook, `.speed-demon.toml` |
| Baseline C then A | H2–H6 | correct scalar CSV machine → SHA-green; then ingest baseline |
| *Sleep 1* | H6–H11 | 5 h — protected |
| Baselines D, B, E | H11–H17 | CSR bidirectional → index → raw-block rANS |
| **Milestone** | **~H17** | **all five SHA-green + deterministic → tag `baseline-green`** |
| Deep A | H18–H28 | 10 h |
| *Sleep 2* | H28–H33 | 5 h |
| Deep D | H33–H43 | 10 h |
| Deep C | H43–H51 | 8 h |
| *Sleep 3* | H51–H56 | 5 h |
| Solid B | H56–H61 | 5 h |
| Targeted E | H61–H64 | 3 h |
| Buffer | H64–H66 | catch up the furthest-behind track, finish PERF.md |
| **LOCKDOWN** | **H66–H72** | **no new ideas — verify only** |

### The 72-hour stop rules

1. A track earns optimisation time only after it is SHA-green and
   byte-identical across ≥5 runs.
2. Any change that breaks the gate is **reverted, not debugged under time
   pressure**.
3. Protect the three sleep blocks and the 6-hour lockdown. They are not
   buffer.
4. At H64, whatever is not green stays at its last green baseline.
5. A track is "done" when it reaches its target K band — bank it and move on
   rather than chase a brittle +0.3.

### Nov 2 18:00 UTC — the freeze

```bash
powershell -File verification/pre-freeze.ps1 -All     # must be 0 failures
git tag final-<commit>
git push origin main                                   # public repo
```

Then record the final commit hash in every `PERF.md`.

---

## 4. Tool reference

### The one command

```bash
sh tooling/ci.sh              # every gate, cheapest-fails-first
sh tooling/ci.sh --fast       # skip the container runs
sh tooling/ci.sh --tracks A,C
```

### On the scoring box (Linux — no PowerShell, no Python needed)

```bash
cargo build --release --offline -p sd-harness

./target/release/sd-harness bench   --track A --runs 5 --image speeddemon/solo:final
./target/release/sd-harness verify  --track A
./target/release/sd-harness ledger
./target/release/sd-harness plan
./target/release/sd-harness lockdown
./target/release/sd-harness prewarm --file /srv/speeddemon/data/track-A/access.log
./target/release/sd-harness hash    --file out/ingest.txt
```

`bench` reproduces the organizer's protocol exactly: the verbatim
`docker run` flags (`--cpuset-cpus=0-11`, `--memory=8g --memory-swap=8g`,
`--network=none`, `--read-only`, the 1 GiB tmpfs), peak memory from the
kernel's cgroup `memory.peak`, 5 runs with the high and low discarded, and
all four gates reported.

### On the dev machine

```bash
cargo build --release --offline
cargo test --offline
python3 tooling/measure_axes.py --all      # the S and D axes
python3 tooling/k_calc.py --preset nightshade
python3 tooling/gen-fixtures.py --all      # the anti-overfit corpus
python3 tooling/gen_docs.py                # regenerate PERF.md / README.md
python3 tooling/rule05_guard.py            # the pre-kickoff compliance check
python3 tooling/difftest.py out.txt ref.txt  # why did the hash mismatch?
sh tooling/vendor-toolchain.sh             # vendor rustc for --network=none
sh tooling/pgo-build.sh --track A          # offline profile-guided build
sh tooling/install-hooks.sh                # the pre-commit gate
```

### The tools that exist because of how the 72 hours will go wrong

| Tool | The failure it prevents |
|---|---|
| `tooling/rule05_guard.py` | Committing a solution before kickoff → disqualification from the whole event |
| `tooling/difftest.py` | Losing 3 hours to a hash mismatch that is actually a trailing newline |
| `verification/harness` (ledger) | Writing PERF.md from memory instead of from measurements |
| `verification/harness` (lockdown) | A stub reaching the freeze because nobody checked |
| `tooling/pgo-build.sh` | Leaving 10–20% of T on the table because the build is free |
| `tooling/ci.sh` | Discovering a broken gate 20 minutes before a commit you needed |

---

## 5. The three things most likely to cost points

1. **A hash mismatch on Track C.** It is the track the organizers say most
   submissions fail. The scalar state machine is the oracle; SIMD is only a
   candidate *locator*. Never let a fast path near the quoting rules.
2. **A non-offline build.** `docker build --network=none` fails the build, and
   a failed build scores zero — on every track, because the image is shared.
   `tooling/vendor-toolchain.sh` is the fix, and it must be run on a
   networked machine *before* the hack.
3. **A deadline that moves.** The scoring run is on a **holdout** dataset
   generated at a seed you have never seen. Anything tuned to the sample's
   bytes is a liability, and code that recognises the input is
   disqualification from the whole event. The fixture generator exists so that
   every optimisation is gated on data that is *not* the sample.

## 6. The pre-kickoff design, in one place

`docs/specs/` holds one decision-complete design specification per track,
each with the same eight sections: Reference Teardown, Axis Budget,
Algorithm & Data Layout, Hash-Gate Correctness & Determinism, Optimization
Ladder, X-Scaling Safeguard, Hour Budget, Risk Register & Stop Rule.

They contain **no solution code** — only design, arithmetic, and decisions.
That is both the point and the legal position: rule 05 requires the code to
be written inside the window, and a design document is what turns H0 into
typing instead of thinking.

| Spec | Lines | The headline decision |
|---|---|---|
| `docs/specs/track-a-ingest.md` | 285 | Two-level radix histogram for exact percentiles without sorting; the timestamp parse is the second-biggest cost and nobody expects it |
| `docs/specs/track-b-index.md` | 261 | Blend-robust balanced bets; StreamVByte postings; positions are where the memory goes |
| `docs/specs/track-c-join.md` | 303 | 12-feature hostile-input matrix with a parallel-vs-sequential parity scan before the fast path is enabled |
| `docs/specs/track-d-traverse.md` | 291 | CH rejected on amortization arithmetic; CSR + radix heap + bidirectional + ALT landmarks |
| `docs/specs/track-e-compress.md` | 311 | Speed-first: the M axis is only worth 2.2 points across the whole ratio band; per-segment models, byte-plane transposition for the float blob |
