# Speed Demon — Campaign Status & Runbook

> **Read this first.** It is the operational dashboard for the pre-kickoff
> build and the runbook for the 72-hour window.
>
> **Status: PRE-KICKOFF SCAFFOLD — green infrastructure, stub implementations.**
> The machine is ready. The *implementations* are not, by design: rules 04/05
> require all project code to be written inside the 72-hour window
> (Oct 30 15:00 UTC → Nov 2 15:00 UTC).
>
> Last updated: after the restructure to per-track images (Oct 27). See §1a.

---

## 1a. What the Oct 22 audit changed

The audit ran the build scripts rather than reading them. Five of them had
never been executed, and every one of those five was broken. Back then the
harness built a single image for all four tracks, so each defect was a total
loss on submission rather than a degraded score. The image contract is now
**per track** (one image per track folder, built as its own context), so a
defect in one track costs that track alone.

| Defect | Consequence if it had shipped | Fixed |
|---|---|---|
| `--crate-name speeddemon-a` | rustc rejects hyphens; build fails | tracks self-contained; no shared crate names |
| Core rlib built for the host triple, binary linked for musl | link fails with E0461 | `core/` removed — each track compiles its own `src/main.rs` |
| `-C linker-flavor=gnu-lld` | unstable option, hard error on stable | plain `rustc` → the rust base image's default linker (stable) |
| musl "detected" via `--print target-libdir` | succeeds for uninstalled targets; build fails later | gone with the musl build — runtime is the debian:13-slim glibc |
| gnu link in an image with no C compiler | impossible link, no network to fix it | rust:1-slim-trixie ships gcc + libc6-dev for linking |
| Per-track Dockerfiles | referenced `/app/build.sh` with the track dir as context; nothing was in the image | official shape: `COPY . /src` + `RUN ./build.sh`, per-track context, digest-pinned stages |
| `pgo-build.sh` | built with cargo into `target/pgo`, never reached the image; no `llvm-profdata merge`, so `-profile-use` had nothing to read | removed — build-time training is an in-window decision once real code exists |
| `ci.sh` | hardcoded `python3`; called an unimplemented `--s-only`, so the S axis silently passed | interpreter detection, one pass for both axes, 5 new gates |
| No executable bits in git | a fresh clone silently ignores the pre-commit hook | all scripts `100755` |

Two things came out of it that were not repairs:

- `tooling/spec_intake.py` — the Oct 28 checklist. It enumerates all 36
  DEFERRED values, records pins, recomputes the K budget, and refuses to
  report ready while a blocking value is unanswered.
- A harness test that asserts the Rust contract table still agrees with
  `verification/contract.toml`, so the two cannot drift when the specs freeze.

The dashboard below overclaimed before this audit: rows marked GREEN for the
offline build path, dispatch, and one-command CI were describing scripts that
had never been run. They are green now, and `tooling/ci.sh` proves it on
every run.

---

## 1. What is built and proven

| Component | State | Evidence |
|---|---|---|
| 4 self-contained track folders (track-a..track-d) | **GREEN** | `sh track-*/build.sh` from a clean clone — all four produce `./run` offline |
| Track scaffolds (rustc-only, rule-05-safe) | **GREEN** | every `src/main.rs` is a stub; `rule05_guard` scans all four folders |
| **`verification/harness`** — the scoring-box replica | **GREEN** | 20/20 tests: pure-Rust SHA-256 (known vectors, held-out sample), JSONL ledger, lockdown, contract table *and contract.toml agreement*. `cgroup.rs` has **no unit test** — it is exercised only by live container runs |
| `rustfmt` + `clippy -D warnings` | **GREEN** | both run as best-effort gates in `tooling/ci.sh` (skip only when the component is not installed); the pre-commit hook also enforces them once installed |
| Offline build path (`rustc`, no cargo, no network) | **GREEN** | `track-*/build.sh` compiles each folder's `src/main.rs` directly; the `ci.sh` "scored build path" gate builds all four, and the Dockerfile runs the identical script |
| Per-track image contract (one image per track) | **GREEN** | each track folder is its own build context; entrypoint IS the track binary — no dispatcher. `check_base_pinned.py` verifies digest-pinned, allowlisted stages |
| 3-gate verifier (SHA / timeout / sandbox) | **GREEN** | Rust harness (Linux) + `verify.ps1` (Windows). **The speed floor gate is gone** — there are three gates, not four. Inside the harness, an unpinned expected hash is now a **FAIL**, not a green tick |
| 5-run trimmed-mean benchmark | **GREEN** | `harness bench` + `bench.ps1` |
| Pre-freeze lockdown | **AMBER — failures** | `pre-freeze.ps1` (Windows) + `harness lockdown` (Linux). The failures are the correct pre-kickoff state; see §1a |
| **Rule 05 compliance guard** | **GREEN** | `tooling/rule05_guard.py` — proved to block a planted violation |
| **Measurement ledger → generated PERF.md** | **GREEN** | every measurement is a JSONL row; tables are generated, not remembered. `python tooling/gen_docs.py --check` is the CI gate that proves the committed docs *are* the generator's output |
| **Differential tester** | **GREEN** | `tooling/difftest.py` — pinpoints byte/line/column of a hash mismatch |
| **Per-track design specs** | **GREEN** | `docs/specs/*.md` — 4 × ~290 lines, decision-complete, no solution code; the P0/P1/P2 feature inventory + ladder + H0 runbook per track live in `docs/depth/` |
| Build-time training (PGO, AOT) | **DEFERRED** | an in-window decision once real code exists — the rules allow training on generated fixtures, but a profile of a stub is worse than none; nothing pre-built |
| One-command CI | **AMBER — 1 red gate** | `sh tooling/ci.sh` — cheapest-fails-first (`--fast` skips only the container runs). The red gate is the lockdown above |
| K calculator | **GREEN** | reproduces the published worked example exactly (K 6.41, speed score 40.4) |
| S/D axis measurement | **GREEN** | **D = 1.0000 on all four tracks** |
| Anti-overfit fixture generator | **GREEN** | grammar-preserving data + 11 hand-crafted hostile CSV cases |
| **SPEC.md intake** | **GREEN** | `tooling/spec_intake.py` — DEFERRED values enumerated, K budget recomputed from pins, refuses "ready" while a blocker is unanswered |
| Track implementations (A–D) | **STUB** | scaffold sentinel present — *correct* pre-kickoff state |

### The open FAILs, and why each is correct right now

`harness lockdown` reports **failed checks**: 4 × "no scaffold sentinel in
source", 4 × "sample SHA-256 green", "public remote configured", and the
working tree. Every one of them clears by doing the work rather than by
changing a check.

| FAIL | Why it is correct now | When it clears |
|---|---|---|
| 4× "no scaffold sentinel in source" | The implementations do not exist yet, and rule 05 forbids writing them before Oct 30 15:00 UTC. The check is doing its job: **a stub can never reach the freeze.** | H0–H17, as each track's real implementation lands |
| 4× "sample SHA-256 green" | The expected hashes are pinned from the Oct 30 06:00 UTC sample. This used to be reported as SKIP, which let the lockdown certify a correctness gate it had proved it could not check — a placeholder file containing `TODO-fill-this-in` counted as a pin. It is now a FAIL until a real 64-hex pin exists | Oct 30, when the sample ships and the hashes are pinned |
| "public remote configured" | There is no public GitHub repo yet. This now requires an https/ssh URL — it previously accepted any two `git remote -v` lines, so a `file://` path passed | before the freeze — push to a public repo under an OSI licence |
| "working tree clean" | The tree is mid-audit; this is the real state, not a measurement artefact. It previously **vanished entirely on a host without git**, which is how a dirty tree with no remote could print "ALL CHECKS PASSED — safe to freeze" | the final commit before the freeze |

There is **no** longer a "two informational SKIPs" row: the source-size skip it
described never existed in `pre-freeze.ps1` (which had exactly one in-loop
skip), and the SHA skip is now a failure, not a skip.

### 1b. Two defects that each silently disabled a scoring axis

Both are fixed, and both are now guarded, because neither is visible in any log
or exit code:

* **PGO never applied.** `build-one.sh` passed `-C profile-use=<directory>`.
  rustc treats that as a warning — `warning: <dir>: is a directory` — and
  **exits 0**, compiling with no profile, while the log printed
  "[build-one] PGO profile found". Even with the flag corrected the profile was
  unreachable: it lived in `target/pgo/prof`, and `.dockerignore` excludes both
  `target/` and `*.profdata`. Guarded by `tooling/check_pgo_wiring.sh`, which
  fails on the old form.
* **The dispatcher answered the wrong track.** `run` filtered `$TRACK` through
  `tr -cd 'A-Z'` and then defaulted the empty result to `A`. Measured with stub
  binaries: `TRACK=7`, `TRACK=1`, `TRACK=0` and `TRACK=-` each ran **track A
  and exited 0**. Only `Z` failed. A wrong-track answer scores as a silent zero
  at best; it is now a validation error, exit 1.

---

## 1b. The four decisions that are already locked

These are made, with the arithmetic behind them, in `docs/specs/`. They are
locked **before** the window opens so that H0 is typing, not deciding.

| # | Decision | Why it is locked |
|---|---|---|
| 1 | **Contraction Hierarchies are legal for Track D, but this query mix rarely repays them** | CH preprocessing costs 60–120 s *inside* the timed run, against a Commit budget of 2.9 s total. Amortised over only 2,000 queries it needs a ~30 ms/query saving to break even — about two orders of magnitude off. Chosen instead: CSR + radix heap + bidirectional + 6–8 ALT landmarks, with runtime-adaptive batching by distinct source/target. The organizers' framing — legal, but rarely repaid by this query mix — is what PERF.md states, with the arithmetic behind it. |
| 2 | **Track B is one total wall time, and its queries are readable from the start** | The 40/60 build/query split is gone, so build and query are no longer scored separately and the blend question disappears. The query set is available before the corpus is indexed, which is the track's central structural asymmetry and it is legal. |
| 3 | **The source-size axis is code lines, and it can only cost** | S = code lines counted by `scc` after the formatter, across the whole repo minus registry packages first published before Oct 1, 2026 by people outside the team. It is priced, not rewarded. Byte-identical copies of the published generators and references are free. |
| 4 | **D = 1 is a free axis and must be defended** | 0.15 of the weight, and `D = 1 + direct non-stdlib runtime deps` means one dependency ⇒ D = 2 ⇒ 2⁻⁰·¹⁵ = 0.90 on K. The rule-05 guard and the D-axis checks exist to keep it at 1.00. |

**One risk already identified:** if the page cache is charged to the memory
cgroup on the scoring box, Track A's M ratio collapses and K drops. There is a
scheduled H1 measurement for exactly this, because it changes the memory design
of the biggest track.

---

## 2. The contract, pinned and unpinned

`verification/contract.toml` is the single source of truth. Every value the
plan could not honestly know is listed there as `DEFERRED` rather than
invented. On **Oct 28 18:00 UTC** (specs + harness freeze) fill them in and
every K budget in the plan becomes real.

**Pinned already (from the public event page):**

* Scoring: `K = ∏(ref/ours)^w`, weights `T .50 / M .20 / S .15 / D .15`,
  `speed = 50·log10(K)`, track total `0.65·speed + 0.35·judges`, overall =
  geometric mean of ≥3 tracks.
* Caps: **wall time has no cap**; **memory is capped at 50×**; **source size and
  dependencies can cost you but never pay.**
* Reference times (measured on the scoring machine, five runs each): A 32.05 s ·
  B 56.35 s · C 14.44 s · D 72.69 s.
* Timeouts: A 321 · B 564 · C 144 · D 727 s.
* Datasets: A 239 MB / 3.1M lines · B 256 MB / 50,000 queries over a 250 MB
  corpus · C 204 MB / 3.32M records · D 92 MB / 5M edges. A's reference memory
  is 228 MB and the reference source size is 56 lines.
* Machine: Falkenstein (fsn1), i5-13500, `--cpuset-cpus=0-11` (6 P-cores /
  12 threads), turbo off, `--memory=8g --memory-swap=8g` (**swap off**),
  `cgroup v2 memory.peak`.
* Run protocol: 5 runs, discard hi+lo, mean of middle 3; >5% spread re-runs;
  >2% reference drift rescores the batch.
* Docker contract: every `FROM` names an image from
  `speeddemon.dev/allowlist.json`, **pinned by digest**; nothing else may enter
  the image (no `COPY --from` of another image, no `ADD` of a URL); read `/data`,
  write exactly one file to `/out`, exit 0, build with `--network=none`; builds
  capped at 10 minutes and images at 2 GB.
* Gates: **three** — correctness, the timeout, the sandbox. The speed floor
  gate is gone.
* Prizes: $1,800 total — $800 / $400 / $200 overall, $200 for two write-ups,
  $50 for the highest K on each track. There is no prize for the smallest source.
* Ties on the final total are broken by raw K, then by wall time, then by
  earliest submission commit.

**Still DEFERRED — pin these on Oct 28:**

| Track | The decision that changes the most work |
|---|---|
| A | output ordering, percentile rank definition, error-rate format, group key |
| B | boolean operator precedence, tokenisation, and the result format (doc ids vs counts) |
| C | quoting rules for embedded newlines, mid-file BOM semantics, amount scale, aggregate key |
| D | distance-only vs path output, unreachable spelling, max distance bound |

---

## 3. Runbook

### Before Oct 28 (now) — done

```bash
sh track-a/build.sh && sh track-b/build.sh && sh track-c/build.sh && sh track-d/build.sh
cargo test --offline --manifest-path verification/harness/Cargo.toml # 20/20 harness
sh tooling/install-hooks.sh            # installs the pre-commit gate
```

### Oct 28 18:00 UTC — spec freeze

1. Read all four frozen specs end to end.
2. Fill `verification/contract.toml` — every `DEFERRED` becomes a fact.
3. Replace the TARGET K budgets in `docs/MASTER-PLAN.md` with the real
   `Tref/Mref/Sref`.
4. Load the sample expected hashes into
   `track-<t>/verification/expected/<output>.sha256` (Oct 30 06:00 UTC).

### Oct 30 06:00 UTC — sample data

```bash
# pin the expected hashes (one line each: "<sha256>  <output-file>")
# NOTE: already pinned from the scaffolds' deterministic output — track-a/..D/
# verification/expected/*.sha256 exist; replacing a pin after H0 requires the
# SHA gate to go green again with the real implementation.
echo "<sha256>" > track-a/verification/expected/summary.txt.sha256

# and confirm the gate is live
powershell -File verification/verify.ps1 -Track A
```

### Oct 30 15:00 UTC — H0, the window opens

Order of work (from `docs/MASTER-PLAN.md` §5):

| Block | H | Work |
|---|---|---|
| Scaffold | H0–H2 | repo, Dockerfiles, harness replica, SHA hook, `.speed-demon.toml` |
| Baseline C then A | H2–H6 | correct scalar CSV machine → SHA-green; then ingest baseline |
| *Sleep 1* | H6–H11 | 5 h — protected |
| Baselines D, B | H11–H15 | CSR bidirectional → index |
| Baseline hardening | H15–H17 | determinism sweeps on all four, first live-board pushes |
| **Milestone** | **~H17** | **all four SHA-green + deterministic → tag `baseline-green`** |
| Deep A | H18–H28 | 10 h |
| *Sleep 2* | H28–H33 | 5 h |
| Deep D | H33–H43 | 10 h |
| Deep C | H43–H51 | 8 h |
| *Sleep 3* | H51–H56 | 5 h |
| Solid B | H56–H61 | 5 h |
| Extra deep A | H61–H64 | 3 h |
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

### Nov 2 15:00 UTC — the freeze

```bash
powershell -File verification/pre-freeze.ps1 -All     # must be 0 failures
git tag final-<commit>
git push origin main                                   # public repo
```

Then record the final commit hash in every `PERF.md`.

---

## 3a. Oct 28 18:00 UTC — SPEC.md intake

The organizers freeze the specs on Oct 28; the window opens Oct 30 15:00 UTC.
36 values in `verification/contract.toml` are marked `DEFERRED` until then;
24 of them block a track outright (get one wrong and the submission writes a
file nobody asked for). `tooling/spec_intake.py` is the checklist for that
day so the work is bookkeeping rather than archaeology.

```bash
python tooling/spec_intake.py --status    # every DEFERRED value + where SPEC.md answers it
python tooling/spec_intake.py --check     # the gate; fails while a blocker is unanswered
python tooling/spec_intake.py --kbudget   # the K budget from the pinned numbers
```

`--check` runs the harness consistency test, so it catches the one failure
mode that reading cannot: a value pinned in `contract.toml` while the Rust
table keeps the old number. If that test fails, the fix is to update
`verification/harness/src/contract.rs` to match, not the other way round —
`contract.toml` is the record of what the organizers froze.

Then, in order:

1. Update the four `track-*/.speed-demon.toml` output filenames if the spec
   renamed anything; the lockdown cross-checks them against the contract.
2. Re-run `python tooling/fix_encoding.py --scan .` after pasting any spec
   text into a file — the PowerShell round-trip that produced the Oct 22
   mojibake is still the default way text gets edited on this host.
3. Re-read `docs/specs/*.md` against the frozen spec and correct any decision
   the real spec contradicts. The Track D Contraction-Hierarchy decision is the
   one most likely to be affected: it rests on the preprocessing budget, so if
   the frozen D spec allows untimed preprocessing the arithmetic changes.
4. `sh tooling/ci.sh` must be all green before the window opens.

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
./target/release/sd-harness prewarm --file /srv/speeddemon/data/track-a/requests.log
./target/release/sd-harness hash    --file out/summary.txt
```

`bench` reproduces the organizer's protocol exactly: the verbatim
`docker run` flags (`--cpuset-cpus=0-11`, `--memory=8g --memory-swap=8g`,
`--network=none`, `--read-only`, the 1 GiB tmpfs), peak memory from the
kernel's cgroup `memory.peak`, 5 runs with the high and low discarded, and
all three gates reported.

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
python3 tooling/spec_intake.py --status    # the Oct 28 checklist
python3 tooling/check_base_pinned.py       # image contract: digest-pinned allowlisted stages
sh tooling/install-hooks.sh                # the pre-commit gate
```

### The tools that exist because of how the 72 hours will go wrong

| Tool | The failure it prevents |
|---|---|
| `tooling/rule05_guard.py` | Committing a solution before kickoff → disqualification from the whole event |
| `tooling/difftest.py` | Losing 3 hours to a hash mismatch that is actually a trailing newline |
| `tooling/spec_intake.py` | Pinning the frozen values by hand on Oct 28 and getting one wrong |
| `verification/harness` (ledger) | Writing PERF.md from memory instead of from measurements |
| `verification/harness` (lockdown) | A stub reaching the freeze because nobody checked |
| `tooling/check_base_pinned.py` | A hand-edited Dockerfile that drifts off the allowlist, or a base image that is not digest-pinned |
| `tooling/ci.sh` | Discovering a broken gate 20 minutes before a commit you needed |

---

## 5. The three things most likely to cost points

1. **A hash mismatch on Track C.** It is the track the organizers say most
   submissions fail. The scalar state machine is the oracle; SIMD is only a
   candidate *locator*. Never let a fast path near the quoting rules.
2. **A non-offline build.** `docker build --network=none` fails the build, and
   a failed build scores zero on **that track** (images are per-track, so the
   damage is contained). The digest-pinned `rust:1-slim-trixie` build stage
   ships the toolchain, so nothing is vendored and nothing is fetched.
3. **A deadline that moves.** The scoring run is on a **holdout** dataset
   generated at a seed you have never seen. Anything tuned to the sample's
   bytes is a liability, and code that recognises the input is
   disqualification from the whole event. The fixture generator exists so that
   every optimisation is gated on data that is *not* the sample.

## 6. The pre-kickoff design, in one place

`docs/specs/` holds one decision-complete design specification per track,
each with the same eight sections: Reference Teardown, Axis Budget,
Algorithm & Data Layout, Hash-Gate Correctness & Determinism, Optimization
Ladder, Scaling Safeguard, Hour Budget, Risk Register & Stop Rule.

They contain **no solution code** — only design, arithmetic, and decisions.
That is both the point and the legal position: rule 05 requires the code to
be written inside the window, and a design document is what turns H0 into
typing instead of thinking.

| Spec | Lines | The headline decision |
|---|---|---|
| `docs/specs/track-a-ingest.md` | 285 | Two-level radix histogram for exact percentiles without sorting; the timestamp parse is the second-biggest cost and nobody expects it |
| `docs/specs/track-b-index.md` | 261 | One total wall time with the queries readable from the start; StreamVByte postings; positions are where the memory goes |
| `docs/specs/track-c-join.md` | 303 | 12-feature hostile-input matrix with a parallel-vs-sequential parity scan before the fast path is enabled |
| `docs/specs/track-d-traverse.md` | 291 | Contraction Hierarchies are legal but rarely repay this query mix; CSR + radix heap + bidirectional + ALT landmarks |
