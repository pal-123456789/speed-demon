# SESSION LOG — start here when resuming

A single resume point. Read this first, then
[KICKOFF-CARD.md](KICKOFF-CARD.md) for the procedure and
[WRITEUP-EVIDENCE.md](WRITEUP-EVIDENCE.md) for the write-up material.

**Last updated:** Oct 29, 2026, on the tip of `speed-demon`.

---

## 1. State, in one screen

| | |
|---|---|
| Repository | `pal-123456789/speed-demon` — **private**, default branch `speed-demon` |
| Local checkout | `D:\SpeedDemon-Campaign`, branch `speed-demon`, working tree clean |
| Local vs remote | **byte-identical** — `git show --format=%T -s HEAD` gives the same tree hash on both sides |
| Rule 05 | `CLEAR` — no solution code anywhere, as required before kickoff |
| CI | the full suite passes, no failures; the skips are all explained inline |
| Consistency | 177 cross-file assertions, all green |

No commit hash or file count is quoted above on purpose: both change with every
commit, and a document that pins them is a document that is wrong one commit
later. Ask git instead — `git log -1` and `git ls-files | Measure-Object -Line`.

Nothing is uncommitted and nothing is unpushed. `git status` is clean and
`git log origin/speed-demon..HEAD` is empty.

### Event times, in local time (this machine is IST, UTC+5:30)

| Event | UTC | Local |
|---|---|---|
| Kickoff | Oct 30, 15:00 | **Oct 30, 20:30** |
| Code freeze | Nov 2, 15:00 | **Nov 2, 20:30** |
| Write-up deadline | Nov 10 | Nov 10 |

### Still outstanding — both are browser steps only a human can do

1. **Install the Speed Demon Board app** on `pal-123456789/speed-demon`
   (<https://github.com/apps/speed-demon-board>). It is required for board runs
   *and* it is how the final commit is recorded at freeze.
2. **Register, and file the team form** (<https://tally.so/r/q4redG>,
   <https://tally.so/r/Ar7g9z>). No form, no score.

---

## 2. What is verified, and how

Every claim below was produced by running something, not by reading code.

| Claim | How it was proved |
|---|---|
| All four tracks build and run on the scored path | the organizers' `check.py`, via Docker in WSL Ubuntu |
| The build is genuinely offline | `docker build --network=none` succeeded |
| Base images are allowlisted and digest-pinned | their own `dockerfile_check.py` accepts all four; the digest pulled clean |
| The dataset the board uses reproduces | `official-data.py --scale full` reproduced **all twelve** published hashes |
| The sandbox contract holds | the scaffold read all 238,836,234 bytes of Track A from the read-only `/data` mount and wrote exactly one file, exit 0 |
| D = 1.00 | their `dependencies.py`, plus the declared `dependency_count = 0` |
| A fresh clone is safe | cloned from GitHub with `core.autocrlf=true` forced — CI green, no CR in any build input |
| Every documented command works | all seven README commands were executed this session |

Generated data is deliberately **not** in the repository — it is large and
reproducible:

| Directory | What | Restore command |
|---|---|---|
| `verification/data-full/` | the board's exact dataset, 765 MB, full scale seed 1 | `python tooling/official-data.py --track all --scale full --dest verification/data-full` (~9 min) |
| `verification/data/` | development corpus, sample scale, needed by the gate scripts | `python tooling/official-data.py --track all --scale sample` |
| `verification/data-fixtures/` | synthetic anti-overfit corpora | `python tooling/gen-fixtures.py --all` |

---

## 3. The bug ledger — every defect found, and the class it belongs to

This is the part worth carrying forward. The recurring class is **not** wrong
algorithms. It is **checks that cannot fail correctly**: something reports
success, or reports the wrong failure, in a way nothing notices.

### Committed before the session opened the repo
| Defect | Fix |
|---|---|
| Fixture defects the oracle rejected: a NUL in a C `region_id`, three 4-column records, a D graph header whose `n` did not cover its ids | fixed; both corpora are oracle-valid |
| `k_whatif.REFS` held pre-freeze placeholders, so B/C/D ratios silently dropped out of K | official values; 4-track projection 54.51 → 57.80 |

### This session
| # | Defect | Class |
|---|---|---|
| 1 | **No `.gitattributes`.** Git chooses line endings at *checkout* from the cloner's config, and Git for Windows defaults `core.autocrlf=true`. A clone rewrote `track-*/build.sh` to CRLF → `bad interpreter` → zero. It worked locally only because of an uncommitted local override. Proved with a control repo: **3 CR bytes in the shebang**. | Invisible until someone else clones |
| 2 | **`build-one.sh.bak`** — 166 lines of a build script for an abandoned five-track design, tracked at the repository root, contradicting the live architecture | Stale artifact |
| 3 | **`.dockerignore`** described that same abandoned layout; not one path in it existed | Doc describing nothing |
| 4 | **`.gitignore` blocked the PGO profile.** The plan depends on a committed `.profdata` inside a track folder being free *data*; `git add` refused it, so the build would run unprofiled while the log claimed PGO applied | Silent degradation |
| 5 | **`tooling/check_pgo_wiring.sh` was claimed but did not exist.** A status doc asserted it in the present tense | Prose vs filesystem |
| 6 | **`conformance.py` conflated "absent" with "rejected",** so a clean checkout ran red announcing *"the generators are broken"* when the gitignored data simply was not generated | Wrong diagnosis |
| 7 | **1.49 MB of foreign binaries in every build context.** `run` and `run.pdb` were correctly gitignored — and still entered `COPY . /src`, because the *folder* is the context, not the repo. `git` ignore is not `docker` ignore | Context vs commit |
| 8 | **Python wrote CRLF into ten committed files**, via `Path.write_text()` inheriting `newline=None`. Then the staleness check could not see it, because `read_text()` folds CRLF back to LF — *a checker that normalises the property it checks always reports success* | Two bugs compounding |
| 9 | **Regenerating `PERF.md` would have deleted every board run ID.** It is part-generated; the run-ID table is hand-transcribed and no tool can produce it | Silent data loss |
| 10 | **Three PowerShell gates could not run the binary at all.** `build.sh` emits `run` without an extension; Windows will not execute an extensionless PE. `verify.ps1` also discarded the exit code and then read `$LASTEXITCODE` — and `$null -ne 0` is TRUE in PowerShell, so **every run looked like a failed exit and it could never pass** | A gate that can never pass |
| 11 | **`pre-freeze.ps1` checked a manifest key that exists nowhere** (`declared_dependencies`). The official parser reads `dependency_count`, which our manifests already used — so the freeze-day D-axis gate failed a *correct* manifest and its docstring invited breaking it | A gate that can never pass |
| 12 | **A documented README command destroyed the official dataset.** `gen-fixtures.py --all` defaulted its synthetic output into `verification/data`; `track-c/regions.csv` went 2,177,777 B → 226 B. Quiet, because correctness checks still ran — they compared a fixture against an official pin and reported *wrong answer* instead of *dataset gone* | Silent data loss |
| 13 | **The fixer broke the file it was fixing.** `add_utf8_stdio.py` matched only the first line of a parenthesised import and spliced code inside it, making `k_whatif.py` unparseable. It had also never been fully applied: three tools still lacked the snippet | A tool that edits source |
| 14 | **`check_doc_k.py` existed but was wired into nothing** — a real gate that had never run | Orphaned gate |
| 15 | **My fix for #10 passed locally and failed on a fresh clone**: `$ErrorActionPreference = "Stop"` turns native stderr into a fatal error | Caught by the gate added for #10 |

### Second session — the measurement and gate infrastructure was inert

Found by executing the scripts that run **at** kickoff, which had never been run.
The theme sharpens: these are not wrong algorithms, they are **machinery that
reports success while doing nothing**.

| # | Defect | Class |
|---|---|---|
| 16 | **The Rust harness's SHA correctness gate never ran.** `expected_hash()` looked for the pin under `root/tracks/<dir>/` — the abandoned five-track layout, which exists nowhere here. It returned `None` on every call, so `bench` printed *"no expected hash pinned — the correctness gate did not run"* and `RESULT: FAIL` on a repository whose pin was present and correct. The harness's central job was dead, and `lockdown.rs` had the correct path all along — two twins in one crate disagreeing. | A gate that can never pass |
| 17 | **`repo_root()` required a `tracks/` directory** that exists nowhere, so the probe never matched and it silently fell back to the working directory. Its own docstring promised "run it from anywhere"; run from `verification/` it reported ~15 missing files. | A promise that was false |
| 18 | **Track paths were built as `track-A`** by upper-casing the letter, while the repository uses `track-a`. Windows hides this (case-insensitive); fsn1 does not, and the harness is specifically built to run there. | Invisible on the dev host, fatal on the scoring host |
| 19 | **CI skipped the three scoring gates on this host** — correctness, timeout and determinism, exactly the gates that decide whether a track scores — behind a `uname -s = Linux` guard, on the theory that the harness needed Linux ELF binaries. It does not: it spawns the binary directly and probes the Windows `run.exe` name itself. The e2e suite credited with covering them never checked output against the published hash. | Capability denied by inference |
| 20 | **`bench-rung.sh` had the same `uname` guard**, so every rung measured during H1–H21 would have used crude millisecond timing and written **nothing** to `verification/ledger.jsonl`. The measured ladder is generated from that ledger and `PERF.md` is 8% of the track total — it could never have filled in. | Silent absence of evidence |

Fixing #16 also repaired the ledger: `expected_sha256` was `null` in every record
and is now the official pin, so a ledger row finally links a measurement to the
answer it was measured against.

### What each one became
Nothing was fixed by hand alone. Each is now a mechanical gate, and each gate has
been **deliberately broken** to confirm it bites:

- `.gitattributes` presence and content; no CR in **any** tracked first-party
  text file (not just code extensions — that hole is how the `.sha256` pins hid)
- every text writer must force an explicit `newline=` (an AST check on the cause,
  not the symptom)
- the generated region of `PERF.md` is marker-delimited, so regeneration cannot
  delete hand-written rows; a gate asserts the markers exist
- every tracked first-party text writer must force UTF-8 stdio
- every `tooling/*` script named in prose must exist
- no path from the abandoned architecture may be referenced as current
- every track folder's build context must be clean: a forbidden file on disk must
  be docker-ignored or the gate fails
- a `.profdata` inside a track folder must remain committable
- **whatever is in `verification/data` must be identifiable as a published
  dataset** — this is the gate that catches #12 immediately
- the declared-dependency key is **discovered from the organizers' own parser**
  and asserted in every manifest, so it cannot drift
- the K arithmetic quoted in the docs is recomputed
- every documented PowerShell gate must actually run the binary and reach the
  hashing stage
- no `join("tracks")` path and no track directory rebuilt from the uppercase
  letter anywhere in the harness, plus a harness unit test asserting
  `expected_hash` finds the real pin **in this repository's layout** — the
  regression test for #16, which is the one that had no test at all
- the harness is now the CI gate runner on **every** host; the four track gates
  run instead of being skipped, so the skips went from five to one (the
  freeze-day lockdown, which is false by design before kickoff)

---

## 4. How to resume — one command

```powershell
cd D:\SpeedDemon-Campaign
.\tooling\run.ps1 ci
```

Green means the tree is in the state this log describes. If `verification/data`
is missing (a clean clone), the data gates skip with the command that fixes it,
which is expected and not a failure.

Then, in order: [KICKOFF-CARD.md](KICKOFF-CARD.md) for the kickoff procedure,
[WINDOW-RUNBOOK.md](WINDOW-RUNBOOK.md) for the hour-by-hour plan,
[MODULE-PLAN.md](MODULE-PLAN.md) for the module order.

---

## 5. The method that found these, so it does not get lost

Auditing **by angle** — docs, then line endings, then Docker, then generators —
always finds one more thing, because each angle is blind to the others. What
worked was:

1. **Enumerate** every tracked file and every claim made about it.
2. For each claim, **find the one command that would prove it.**
3. **Run that command** — especially the ones that have never been run. Every
   defect numbered 10–15 above lived in a code path that had never executed.
4. **Then break it on purpose** and confirm the checker notices.

That is why the gates exist rather than the fixes alone, and why each one has a
negative control. A fix without a gate is a fix that will be undone.
