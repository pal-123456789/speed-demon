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
| `rustfmt` + `clippy -D warnings` | **GREEN** | pre-commit hook enforces both |
| Offline build path (`rustc`, no cargo, no network) | **GREEN** | `build.sh` / `build-all.sh` compile core + 5 binaries with `--extern` |
| One-image-five-tracks dispatch on `$TRACK` | **GREEN** | `run` + root `Dockerfile` |
| 4-gate verifier (SHA / determinism / timeout / sandbox) | **GREEN** | `verify.ps1` — ran on all 5 tracks |
| 5-run trimmed-mean benchmark | **GREEN** | `bench.ps1` |
| Pre-freeze lockdown (81 checks) | **GREEN** | `pre-freeze.ps1` — 74/81, 7 honest FAILs (see §2) |
| K calculator | **GREEN** | reproduces the published Nightshade example exactly (K 6.2569, speed 56.97, total 65.73) |
| S/D axis measurement | **GREEN** | **D = 1.0000 on all five tracks** |
| Anti-overfit fixture generator | **GREEN** | grammar-preserving data + 11 hand-crafted hostile CSV cases |
| Track implementations (A–E) | **STUB** | scaffold sentinel present — *correct* pre-kickoff state |

### The 7 open FAILs, and why each is correct right now

| FAIL | Why it is correct now | When it clears |
|---|---|---|
| 5× "no scaffold sentinel in source" | The implementations do not exist yet. The check is doing its job: **a stub can never reach the freeze.** | H0–H17, as each track's real implementation lands |
| "working tree clean" | Uncommitted verification-script fixes | immediately, on the next commit |
| "public remote configured" | There is no public GitHub repo yet | before the freeze — push to a public repo under an OSI licence |

**Two SKIPs, both informational:** "sample SHA-256 green" cannot be checked
before the Oct 30 06:00 UTC sample ships, and the per-track reference source
sizes for B–E are not published yet.

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

| Command | What it does |
|---|---|
| `cargo build --release --offline` | build everything, no network |
| `cargo test --offline` | 13 core unit tests |
| `powershell -File verification/verify.ps1 -Track A` | the four gates on one track |
| `powershell -File verification/bench.ps1 -Track A -Runs 5` | 5-run trimmed mean, exactly the organizer's protocol |
| `powershell -File verification/pre-freeze.ps1 -All` | the 81-check lockdown |
| `python tooling/k_calc.py --preset nightshade` | reproduce the published worked example |
| `python tooling/k_calc.py --t 30 --m 15 --s 0.62 --d 1 --x 1.05` | price a candidate |
| `python tooling/measure_axes.py --all` | where the S and D axes stand |
| `python tooling/gen-fixtures.py --all` | grammar-preserving fixtures (the anti-overfit guard) |
| `python tooling/gen-fixtures.py --track C --adversarial` | the 11 hostile CSV edge cases |
| `python tooling/gen_docs.py` | regenerate the per-track PERF.md / README.md |
| `sh tooling/vendor-toolchain.sh` | vendor rustc for the offline Docker build |
| `sh tooling/install-hooks.sh` | install the pre-commit gate |

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
