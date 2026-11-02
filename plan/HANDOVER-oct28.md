# HANDOVER — what was done on Oct 28, and what to do at kickoff

> **Read this first when we resume (kickoff Oct 30 15:00 UTC).** The 72-hour
> operating plan, the module loop and the video plan are in
> [`docs/KICKOFF-PLAN-pal.md`](KICKOFF-PLAN-pal.md).
> Written Oct 28 19:10 UTC. HEAD at the time: `d4790a9`, working tree clean,
> 256 tracked files, the whole CI gate wall green.
>
> Everything below is committed. Nothing is pending except four human actions
> (§6). This file is the reminder: the state, the commands, and the exact
> sequence to run when the clock starts.

---

## 1. The one-paragraph version

On Oct 28 the organizers published the full harness repo (specs, generators,
reference implementations, harness, tests) and moved kickoff/freeze to
**15:00 UTC**. We read all of it and turned the project from "our best guess at
the contract" into **"we hold the contract, its data, its oracle, its tests and
its calibration numbers, and we prove it on every commit"**. The DEFERRED
ledger is closed, the official sample data is installed with the official
expected hash pinned, the organizers' own test suite passes 58/58, and their
own Dockerfile guardrail and D-axis measurement both accept our submission.

## 2. Today's commits (the record)

| Commit | What it added |
|---|---|
| `a79a0d4` 17:50 | Vendored the official references; differential lab (`tooling/conformance.py`, with `--oracle`, `--diff`, `--minimize`) + measured profiler (`tooling/profile-reference.py`) + `docs/depth/92-REFERENCE-PROFILES.md`. Found and fixed **6 real fixture defects**. |
| `b038391` 18:23 | Freeze payload: **deadlines migrated 18:00 → 15:00** (13 files); official generators vendored; **`tooling/official-data.py`** — generate → verify published input hash → run the reference → verify published output hash → install → pin. **All 8 published seed-1 hashes reproduced.** Full 17-image allowlist. Harness SHA gate made scaffold-aware. |
| `859b772` 18:26 | **Ledger closed** (`No DEFERRED values remain`): reference S pinned (56/91/108/43) from `harness/reference_axes.json`; our S counter aligned to the official rule (prose/data are free) → honest 141 code lines/track. |
| `43a23b0` 18:33 | **Spec conformance corpus**: the `tiny` scale injects every documented edge case, so all four tiny corpora now sit in `verification/cases/` with the oracle's own answers; `tooling/spec-cases.py` (`--verify-corpus`, `--run`). |
| `cf00b58` 18:45 | Vendored the **organizers' own test suite** (`reference/official/`, byte-identical, free under rule 16) → **58/58 pass**; `tooling/official-tests.py` gate. |
| `3e45554` 18:53 | **Gates 21–22 run the authority's own code**: their `dockerfile_check.py` accepts all four Dockerfiles; their `dependencies.py` measures **D = 1.00** on every track; `tooling/official-axes.py`. |
| `d4790a9` 19:09 | Final audit: fixed the last 5 stale path/count references. Sweep now clean. |

## 3. Kickoff facts (memorise these)

| Fact | Value |
|---|---|
| **Kickoff** | **Oct 30, 15:00 UTC** (was 18:00) — full datasets + live board open |
| **Code freeze** | **Nov 2, 15:00 UTC** (was 18:00) — repo locked at the recorded commit |
| Live board cap | 3× the reference; final scoring 10× |
| Board dataset | **`full` scale, seed 1, for every track** (owner-confirmed; the "runs on sample" line in `b-index.md` was drift and is fixed). Final scoring uses a private **full-size** holdout seed. |
| Reference (official calibration) | A 32.047276 s / 227.98 MB · B 56.352376 / 2199.96 · C 14.444059 / 367.44 · D 72.692655 / 892.92 |
| Reference S (code lines) | A 56 · B 91 · C 108 · D 43 |
| Board caps (3×) | A 96.1 s · B 169.1 · C 43.3 · D 218.1 |
| Machine | fsn1, i5-13500, `--cpuset-cpus=0-11`, 8 GB no swap, turbo off, `/data` ro, one file to `/out`, exit 0 |
| Scoring | `K = (Tref/T)^0.50·(Mref/M)^0.20·(Sref/S)^0.15·(Dref/D)^0.15`; speed `= 50·log10(K)`; track total `= 0.65·speed + 0.35·judge`; overall = geometric mean of best three |

## 3a. CORRECTIONS AND NEW FACTS (official LLM context + owner reply, Oct 29)

Read from `speeddemon.dev/speed-demon-context.md` and the owner's answer on the two
repo drifts. These override anything older in this repo.

| # | Fact | Why it matters |
|---|---|---|
| 1 | **The board runs every track on the `full` public dataset, seed 1** — not `sample` (that line in `b-index.md` was drift, now fixed upstream). Board scores therefore use the **full-scale** references we hold: A 32.047276 s / 227.98 MB, B 56.352376 / 2199.96, C 14.444059 / 367.44, D 72.692655 / 892.92. | Our board pushes are scored on 10× the data we tested on with `--scale sample`. Generate `--scale full` (≈600 MB) for every real measurement. |
| 2 | **Final-scoring timeout = 10× T_ref** (A 320.5 s, B 563.5, C 144.4, D 726.9). The board's live cap is **3×**. | Already in `contract.toml`; the harness's `--timeout-factor` default was 3.0 and is now 10.0 upstream. |
| 3 | **Track B at `full` is 250,000 documents and 50,000 queries** (31,000 BOOL / 19,000 PHRASE), `docs.txt` 253,827,587 B, reference peak RSS 2.29 GB. | The corpus is 10× and the query set is 25× our bare sample assumption; budget index build and query time accordingly. |
| 4 | **PERF.md steps must cite live-board run IDs**, and judges check that each cited run exists and that the claimed change moved in the claimed direction. | This changes the module loop: **push each verified rung and record its run ID** in PERF.md. A rung that was never pushed cannot be cited. |
| 5 | **The write-up side quest is a separate $100 × 2 prize, closing Nov 10**, on a public blog (dev.to, Medium, Hashnode or your own site). A README or a GitHub Pages repo does **not** count. | Pure upside beyond the board; schedule a post at D3. |
| 6 | **A committed `.profdata` inside a track folder is DATA, not code** — it costs nothing in S. Tooling kept **outside** the track folders is never scored. | PGO becomes nearly free on S: train in the build, or commit the profile. |
| 7 | **"Unchanged" folder detection**: a folder is unchanged only if every file except Markdown and `.speed-demon.toml` is byte-identical to the starter. One push re-runs **all** changed folders (no per-folder detection). | Our folders always differ, so they always run — expected. Do not leave the final push to the last hour: the queue lengthens near the freeze. |
| 8 | **Authority order**: frozen specs + harness > website > clarifications > video transcript. | When two sources disagree, cite the spec. |
| 9 | **Gates are checked on every one of the five final runs.** Nondeterminism is a correctness bug. | Our determinism matrix (36 configs × 3 runs) is exactly the right guard. |
| 10 | **Specialist prizes**: $50 for the highest K on each track, no three-track requirement. | A single very fast track has its own prize. |

## 4. What we now hold (all verified locally)

1. **Official data** — `tooling/official-data.py` regenerates it and verifies
   the published hashes. Sample scale is installed in `verification/data/`.
2. **Official expected hashes, pinned** — the true SHA-gate targets:
   `ca6311bb…` (A) · `eeeec1e9…` (B) · `9035fed3…` (C) · `a4c76ed0…` (D).
   `tooling/pin-hash.sh` refuses to overwrite them (writes `.local.sha256`).
3. **The oracle** — `reference/*.py` (byte-identical). Byte-diff any candidate
   with `tooling/conformance.py --diff` (add `--minimize` to shrink a divergence).
4. **The conformance corpus** — `verification/cases/`, every documented edge
   case at `tiny` scale with the oracle's answers; `tooling/spec-cases.py --run`
   is the millisecond feedback loop.
5. **The organizers' tests** — `python tooling/official-tests.py` (fast) /
   `--full` (58 tests, ~2 min).
6. **The authority's checkers** — `tooling/official-axes.py` (D) and their
   `dockerfile_check.py` (guardrails), both wired into CI.
7. **Measured priorities** — `docs/depth/92-REFERENCE-PROFILES.md`: the
   reference's time is **A 48.7% timestamp**, **B ~49% build-side**, **C 24%
   amount + 20% aggregate (join only 3.7%)**, **D 97.1% search (parse 0.4%)**.
   Build the first rung where the measurement says.
8. **Ground blueprints** — `docs/depth/95-GROUND-BLUEPRINTS.md`: every feature
   to the byte, plus the 20-row zero-score risk register and the target-T table
   (K=10 needs A 0.321 s · B 0.564 s · C 0.144 s · D 0.727 s).

## 5. The kickoff sequence (run this at 15:00 UTC)

**Invocation note (learned the hard way on Oct 28).** Windows PowerShell has
no `sh` on PATH, so `sh tooling/ci.sh` fails with *"The term 'sh' is not
recognized"*. Use the shim `tooling/run.ps1` (it finds Git for Windows' sh for
you), or run the same commands inside Git Bash. The Python tools find the shell
by themselves (`tooling/_shell.py`: `$SD_SH` → Git for Windows' sh → PATH sh →
bash), so `python tooling/spec-cases.py --build` works from either shell.

```powershell
# 0. state
git log --oneline -1 ; git status --short           # expect clean

# 1. official data + the official expected hash, installed and verified
python tooling/official-data.py --track all --scale sample

# 2. the loop (run from PowerShell; the shim supplies the shell)
python tooling/spec-cases.py --run --build          # every documented edge case, ms
python tooling/conformance.py --track a --diff --build   # full-sample byte diff vs the oracle
.\tooling\run.ps1 bench-rung.sh a --label rung-01        # or: sh tooling/bench-rung.sh a
python tooling/k_whatif.py --scoreboard `
    --t a=1.07 b=1.88 c=0.48 d=2.42 --m a=15 b=120 c=80 d=70 --l a=150 b=260 c=180 d=220
.\tooling\run.ps1 ci                                    # or: sh tooling/ci.sh

# 3. before every green commit
.\tooling\run.ps1 determinism-check.sh all               # or: sh tooling/determinism-check.sh all
python tooling/k_whatif.py --tradeoff --t-abs 1.07 --lines 150 --add-lines 20 --t-gain 8
```

**If a command still fails:** *"The term 'sh' is not recognized"* → use
`run.ps1`; *WinError 2 / FileNotFoundError* → the tool could not find a shell,
set `$env:SD_SH = "C:\Program Files\Git\bin\sh.exe"`; *WSL session error*
→ Git Bash is being shadowed by WSL's bash, and `_shell.py` already prefers Git
for Windows' `sh.exe`.

Priority order from the measured profiles: **A** hour-key/timestamp first ·
**B** tokenizer + single-pass build · **C** amount parser + aggregate layout ·
**D** search loop (heap, labels, early exit, parallel queries). Stop a track
below ~0.4 points/hour, within ~1.5× of its floor, or when a rung needs ≥10%
more lines for <3% time.

## 6. Open items

| # | Item | Where |
|---|---|---|
| 1 | ~~Real team name~~ **DONE**: `Pal Ghevariya` (solo), `language = "Rust"`, `dependency_count = 0` | the four `track-*/.speed-demon.toml` |
| 2 | ~~GitHub repo + push to `speed-demon`~~ **DONE**: `pal-123456789/speed-demon`, private, default branch `speed-demon`, created from the official starter template, pushed at `6750a40` | github.com |
| 3 | ~~Docker smoke build~~ **DONE — and the premise was wrong.** Docker runs inside WSL Ubuntu on this host, so the whole scored path is testable locally. All four tracks verified at full scale: allowlisted digest-pinned `FROM`s, `docker build --network=none`, a 30 MB image, the container reading the board's dataset from the read-only `/data` mount, exactly one output file, exit 0, 4.4–5.6 s. The correctness hash fails only because `track-*/src/main.rs` is the rule-05 scaffold, and reconstructing that hash from `#SCAFFOLD_NOT_IMPLEMENTED` plus the line/byte count reproduces it exactly. Commands: `docs/KICKOFF-CARD.md`. | WSL Ubuntu |
| 4 | **Install the board app** on the repo (browser, ~30 s) | github.com/apps/speed-demon-board |
| 5 | **Register, and file the team form** (repo URL, all four tracks, members). No form, no score. | speeddemon.dev |

Only items 4 and 5 remain, and both are browser steps that need you.

## 7. Where everything lives

```
track-a..d/            the four submission folders (Dockerfile, build.sh, src/, .speed-demon.toml,
                       PERF.md, README.md, verification/expected/<output>.sha256 = the OFFICIAL hash)
reference/             the organizers' references + generators + the full official repo (tests, harness,
                       specs) — outside every track folder, so it costs nothing in S or D
verification/cases/    the tiny conformance corpora + the oracle's expected answers
verification/data/     the official sample data (gitignored, regenerable)
tooling/               38 tools: official-data, spec-cases, conformance, profile-reference, official-tests,
                       official-axes, ci.sh (the gate wall), bench-rung, pin-hash, determinism-check, k_whatif,
                       gen-fixtures, context-replay, check-output-names, check-track-folders, …
docs/depth/            92 profiles · 93 PGO/codegen · 94 visuals · 95 ground blueprints · 97 model digest
                       (all engines, to the byte) · 98 model prompts · 99 events-page pins
docs/WINDOW-RUNBOOK.md the hour-by-hour plan, tool map, and the submission logistics
```

**Bottom line at the freeze:** the contract is closed, the data and expected
hashes are official and verified, the organizers' own tests and checkers pass,
and the CI gate wall proves it on every commit. What remains is writing the four
implementations inside the window — and the four external items above.
