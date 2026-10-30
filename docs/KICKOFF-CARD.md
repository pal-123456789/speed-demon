# KICKOFF CARD — Friday Oct 30, 15:00 UTC = **20:30 IST**

Everything is prepared. This page is the whole procedure: do section 0 in a
browser, then run section 1 in order. Nothing else is required to start.

The 72-hour window runs **Fri 20:30 IST → Mon Nov 2 20:30 IST**.

---

## 0 · Before the clock starts — three browser steps, ~5 minutes

Only you can do these. Nothing in the window works without step 3.

| # | Where | What |
|---|---|---|
| 1 | <https://tally.so/r/q4redG> | **Register.** Every team member, yourself included. Solo means once. |
| 2 | <https://github.com/apps/speed-demon-board> | **Install the board app** on `pal-123456789/speed-demon`. It gets read-only access to the repos you pick. Install it now and leave it installed through the freeze — that is how the final commit is recorded. |
| 3 | <https://tally.so/r/Ar7g9z> | **Team submission form**: repo URL, tracks entered (all four), members. **No form = no score.** Resubmit to change anything; the latest response is the one that counts. |

The repository is already live and pushed:

```
https://github.com/pal-123456789/speed-demon     private, branch speed-demon
```

It is **private** deliberately, and stays private until the freeze (section 3).
Install the board app at kickoff rather than now, so that the run history begins
with a real baseline instead of a scaffold push.

---

## 1 · 20:30 IST — kickoff, in order

### 1.1 Confirm nothing broke overnight

```powershell
cd D:\SpeedDemon-Campaign
.\tooling\run.ps1 ci
```

Expect the closing banner to report skipped checks and **none failed**. Each
skip is explained inline (four need Linux ELF binaries, one is the freeze-day
lockdown, one reports absent generated corpora — this tree has them). A failure
here is real; fix it before writing code.

### 1.2 The board's dataset is already on disk

`verification/data-full/` holds the **exact bytes the live board runs on**:
full scale, seed 1, 765 MB, every hash checked against the organizers'
published values.

```
verification/data-full/track-a   requests.log   238,836,234 B   3,100,000 lines
verification/data-full/track-b   docs.txt       253,827,587 B   + queries.txt
verification/data-full/track-c   records.csv    201,935,059 B   + regions.csv
verification/data-full/track-d   graph.txt       91,950,247 B   + queries.txt
```

If it is ever missing, regenerate and re-verify in about **9 minutes measured**:

```powershell
python tooling\official-data.py --track all --scale full --dest verification\data-full
```

### 1.3 Smoke-test the scored path on all four tracks

Docker lives inside **WSL Ubuntu** on this machine, so the complete scored path
is testable locally — offline build, allowlisted digest-pinned `FROM`s,
sandboxed run, output hash.

```powershell
$D = '/mnt/d/SpeedDemon-Campaign/verification/data-full'
foreach ($t in 'a','b','c','d') {
  wsl -d Ubuntu -- bash -lc "cd /mnt/d/SpeedDemon-Campaign && python3 check.py $t --data $D/track-$t 2>&1 | tail -5"
}
```

Expect, for each track: `every FROM is allowlisted`, `image is 30 MB`,
one output file, and a sha256. **The hash will not match the published one** —
`track-*/src/main.rs` is still the rule-05 scaffold, which reads the whole
dataset and then emits `#SCAFFOLD_NOT_IMPLEMENTED` with the line and byte count.
That is the correct state at H0. Verified working before kickoff.

### 1.3b Measure the M axis — it needs Linux, and you have it

`check.py` reports the time but **not** the memory, and M is 20% of K. Peak
memory is the container's cgroup v2 `memory.peak`, which no Windows process can
read for a container it did not create. The Rust harness does it properly, and
this host has a Linux environment with Docker on cgroup v2, so the real protocol
runs here:

```powershell
cd D:\SpeedDemon-Campaign
.\tooling\wsl-harness.ps1 -Track a -Label baseline
```

It builds the harness as a **Linux** binary inside WSL (into its own target
directory, so the Windows artefacts the PowerShell gates use are untouched),
builds the track image the way the board does, then runs the board's exact
`docker run` flags. Output looks like this:

```
run 2:    2.700s  peak 179.9 MiB  sha 753bf0feb3ce <- used
run 3:    2.812s  peak 227.1 MiB  sha 753bf0feb3ce <- used
T (trimmed mean) : 3.0321 s
peak memory      : 268.8 MiB (max of the runs)
M ratio          : 0.85x
```

That `peak memory` line is the number that goes in `PERF.md`, and it is the one
the K formula multiplies by `^0.20`. Every other local tool reports
`peak memory : n/a`.

### 1.4 Settle the three oracle-deferred behaviours

Resolve these against the vendored reference **before** writing the parser, and
record the answers in the track's `README.md`. All three are places where the
spec text can be read two ways:

1. Track C — an unterminated quote, and bytes after a closing quote.
2. Track B — an empty phrase query (`k = 0`).
3. Track C — `-0.00` as a group minimum.

### 1.5 Then the baselines

H0–H12 is a **SHA-green single-thread baseline on all four tracks before any
optimisation**. A slow green beats a fast gate failure. See
[WINDOW-RUNBOOK.md](WINDOW-RUNBOOK.md) for the hour-by-hour plan and
[MODULE-PLAN.md](MODULE-PLAN.md) for the module order.

---

## 2 · The loop, for every change in the window

1. Build the module (multi-agent), then **verify** it against the oracle and
   the published full-scale hash.
2. `sh tooling/ci.sh` green.
3. Commit and push to `speed-demon`.
4. **Record the board run ID** in the track's `PERF.md`, section
   `## 1b. Board runs`, one row per push, in step order. That table is yours —
   it sits *outside* the marked region, and the generator preserves it byte for
   byte across regenerations. Anything between the
   `BEGIN GENERATED measured` / `END GENERATED measured` markers is rewritten
   from `verification/ledger.jsonl` on every run of `tooling/gen_docs.py`, so
   change the ledger or that tool's `TRACKS` config, never the marked text.
   Judges check that every cited run exists and that the numbers moved the way
   the log claims — and that no run ID is invented.

```powershell
cd D:\SpeedDemon-Campaign
git add -A
git commit -m "track-a: <what changed and why>"
git push
```

The board allows one run per team per 15 minutes, and a newer push replaces the
one waiting in the queue. Near the freeze the queue gets long — **do not leave a
push to the last hour.**

---

## 3 · Freeze — Monday Nov 2, 20:30 IST

Stop pushing by roughly **20:00 IST** so the recorded commit is the one you
want. Then, in the hour after the freeze:

```powershell
gh repo edit pal-123456789/speed-demon --visibility public --accept-visibility-change-consequences
```

- [ ] repo is **public**, under the MIT `LICENSE` already in the tree
- [ ] the Speed Demon Board app is **still installed** (it records the commit)
- [ ] `git log -1` is the commit you intend to be scored — nothing pushed after
- [ ] the team form is submitted and current

Publishing is the one deadline with a hard penalty attached: a repo still
private an hour after the freeze cannot be scored.

---

## 4 · After the freeze — the write-up side quest, by November 10

A separate $200 pot: two prizes of $100, judged on insight, not audience size.
The post must live on a **public blog** (dev.to, Medium, Hashnode or your own
site) — a README or a GitHub Pages repo does not count — must mention Hackathon
Raptors, and must link the repository.

The raw material is already banked in
[WRITEUP-EVIDENCE.md](WRITEUP-EVIDENCE.md): twelve real findings from this
repository, each with the measurement that proves it and a note on why it stayed
invisible. Add to that file as the window produces more, then write the post
from it. The deadline is after the freeze, so this costs no window time.

---

## The three facts that decide everything

- **D = 1.00 forever.** One dependency multiplies K by 0.90. The tracks carry no
  manifest at all, which is what keeps it there.
- **T carries half the weight and has no cap.** A faster program always earns
  more. M is capped at 50×, and S and D can only ever cost you.
- **Correct or zero.** The gate is a SHA-256 of exactly one file, checked on all
  five final runs. Nondeterminism is a correctness bug.
