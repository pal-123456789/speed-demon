# WINDOW RUNBOOK — Oct 30 15:00 → Nov 2 15:00 UTC (72 hours)
> **Schedule:** defers to the canonical H-table at the top of
> [`docs/KICKOFF-CARD.md`](KICKOFF-CARD.md). If this file disagrees with
> it about *when* something happens, that table is right.


> The single document for the window. Every tool below is committed and
> gate-verified. The goal of H0–H12 is **a SHA-green single-thread baseline on
> all funded tracks before ANY optimization** — a slow green beats a fast gate
> failure, and everything after is measured from there.
>
> Design depth: `docs/depth/` (per-track DEEP files, the model digest
> `docs/depth/97-MODEL-OUTPUT-DIGEST.md`, the events-page pins
> `docs/depth/99-EVENTS-PAGE-PINS.md`).
> The scoring formula is the compass; `tooling/k_whatif.py` is the calculator.

## The tool map

| When | Tool | What it gives you |
|---|---|---|
| H0 (once) | `sh tooling/h0-launch.sh` | build all → gates → fixtures → kit → pins, in one run |
| H0–H2 | `python tooling/spec_intake.py --check` | the Oct-28 pins applied; **0 blocking** = intake done |
| Any time | `python check.py a --data verification/data/track-a --expected ca6311bb...` | the OFFICIAL board checker (from the starter): builds with `--network=none`, runs under the board's sandbox, prints the output hash — the closest local proxy for a board run |
| Every rung | `sh tooling/bench-rung.sh a --label rung-07` | build → mini determinism → SHA-check → 5-run bench → ledger |
| Every rung | `python tooling/k_whatif.py --scoreboard --t a=1.07 b=1.88 c=0.48 d=2.42 --m a=15 b=120 c=80 d=70 --l a=150 b=260 c=180 d=220` | K / speed / track total / best-3 geomean |
| Every ~8 h | `python tooling/k_whatif.py --waterfill --hours 60 --base A=47 B=43 C=39 D=35 --doubling A=6 B=5 C=9 D=8` | re-solve the hour budget from measured rates |
| H50+ | `sh tooling/determinism-check.sh all` | the full P6 matrix before every green commit |
| Before freeze | `sh tooling/ci.sh` | the full gate wall, then `verification/pre-freeze.ps1 -All` / `lockdown` |
| Freeze | git push | public repo + OSI license + final commit; PERF.md cites run IDs |

> Board contract reminder: the live board caps runs at **3× the reference**
> (final scoring: 10×), so a track must be under 3× to appear on the board.

## Hour-by-hour

### H0–H2 — intake (no optimization)
1. `sh tooling/h0-launch.sh` — state, builds, gates, fixtures, kit, pins.
2. **Oct 30 06:00: the organizers publish the generators and the sample
   hashes.** Fetch the official-shaped data with `sh tooling/gen-official-data.sh`
   (seed 1, `sample` scale = 10%; `SCALE=full` is the board dataset). Then
   `python check.py a --data verification/data/track-a --expected ca6311bb...` is the official
   local check. Our own `gen-fixtures.py` remains the anti-overfit layer.
3. Read the four frozen specs AND the reference implementations. Write down,
   per track, the EXACT output format, rounding/tie/order rules, malformed-
   input policy, unreachable/empty spellings.
3. Apply the pins to `verification/contract.toml`; `spec_intake.py --check`
   must report **0 blocking**. Add the B/C/D M/S refs to `k_whatif.py` REFS.
4. Pick your three funded tracks (the waterfill prior says exactly one gets
   zero hours — keep the fourth as a verified baseline).

### H2–H12 — baselines (correctness first)
1. For each funded track: a single-thread, obviously-correct implementation
   from the L1 rung of the deep dives. The `prep/` kit is already in
   `track-*/src` (copied by h0-launch).
2. Gate each: `bench-rung.sh` → **SHA-green** → re-pin deliberately
   (`pin-hash.sh`) → commit, tag `baseline-green`.
   **Pin reproducibility rule:** a pin is only meaningful for a known data
   set. The canonical set is `python tooling/gen-fixtures.py --all` (writes
   `verification/data/`); the hostile corpora go to
   `verification/data-adversarial/` and never touch the pins. A fresh clone
   reproduces every committed pin with exactly:
   `gen-fixtures --all` → `sh track-<t>/build.sh` → run → hash. Never pin
   while an `--adversarial` corpus is in `verification/data/`. When the
   official sample ships (Oct 30), re-generate with
   `tooling/gen-official-data.sh` and re-pin against **that**.
3. `difftest.py` against the official reference on the sample + adversarial
   fixtures. If Track C is not oracle-clean by ~H8, decide early whether to
   drop it — a hash-gate zero at H68 is the worst failure mode.

### H12–H50 — the rung loop
1. Pick the next rung from the deep-dive ladders (A: parallel → compact →
   AVX2 delimiters → interning → ts-cache → SWAR; B: query-aware → parallel
   build → fast tokenizer → tiered postings → prefix memo → phrase merge;
   C: parity prepass → skip-only fields → SWAR money → join; D: parallel
   parse → CSR → compact layout → radix heap → parallel queries).
2. `bench-rung.sh <T> --label rung-NN`: red → revert; green → re-pin →
   commit. Never climb on a red SHA.
3. After every green: update the scoreboard and re-derive the 1/total
   weights; re-solve the waterfill every ~8 h.
4. **Stop a track** when the next rung is < ~0.4 track points/hour, when
   it's within ~1.5× of its floor, or when a rung needs ≥ 10% more lines
   for < 3% time — move the hour to the next-best rung or the write-up.
5. T carries 0.50 weight: the parser/IO floor is where winners live. M under
   ~15–25 MB is the cheap second lever. S discipline: terse, documented.

### H50–H60 — hardening (features frozen)
1. Full `determinism-check.sh all` (threads 1..32, odd chunks, chaos, N ≥ 8).
2. P7 memory calibration under the exact `--memory=8g` container: baseline,
   pread floor, page-cache charging, hysteresis.
3. Kill any path that depends on thread count, chunk size, or scheduling;
   no `-C target-cpu=native` unless the build host IS the scoring machine.

### H60–H72 — write-up and final
1. PERF.md per track: the ladder as a measurements table with live-board run
   IDs, plus the "experiments that failed / were reverted" table (judges
   reward it) — `python tooling/gen_docs.py` regenerates from the ledger.
2. Final timing runs (5, middle-3) and the last scoreboard; two sleeps of
   ≥ 6 h were already banked before H48.
3. Freeze-day: `sh tooling/ci.sh` green, `pre-freeze.ps1 -All` (Windows) /
   `lockdown` (Linux) green, public repo pushed, real team name set in
   `.speed-demon.toml`.

## Submission logistics (the five official steps)

These happen outside the repo — **no gate can check them, and missing one
means no score.**

1. **Repo from the template.** The submission repo must be created from
   `hackathon-raptors/speed-demon-starter` ("Use this template"); it may stay
   **private until code freeze**. The board reads only the `track-*` folders,
   so this repo's `track-a..track-d`, `LICENSE`, `check.py` and
   `.speed-demon.toml` files are the content that repo needs; the
   `tooling/`/`verification/`/`docs/` folders are optional there.
2. **Install the Speed Demon Board app** (`github.com/apps/speed-demon-board`)
   on that repo — read-only; it builds every `track-*` folder on push.
3. **Register the team** through the submission form linked from
   speeddemon.dev — one form per team, with the repo URL and the tracks
   entered. **No registration = no score.**
4. **Push to the `speed-demon` branch** (the template's default branch). The
   board builds and runs within minutes; one run per team per 15 minutes;
   the board caps a run at 3× the reference.
5. **Make the repo public under an OSI licence at code freeze**
   (Nov 2, 15:00 UTC). This repo's LICENSE is MIT and already in place.

**Pre-flight checklist before the first push**

- [x] `team` set to the real name in all four `.speed-demon.toml`
      (`Pal Ghevariya`, plus `language = "Rust"` and `dependency_count = 0`,
      which is what makes D = 1.00)
- [x] all four tracks entered — nothing to delete. All four folders differ from
      the starter (Rust, not the reference Python), so the board runs all four.
- [x] **the organizers' `check.py` passes on all four tracks.** The note that
      used to sit here — "the one thing this Windows host cannot run" — was
      wrong: Docker runs inside WSL Ubuntu on this machine, so the complete
      scored path is testable locally. Verified at full scale:
      allowlisted digest-pinned `FROM`s, `docker build --network=none`
      succeeded, 30 MB image (cap 2 GB), the container read the whole dataset
      from the read-only `/data` mount, wrote exactly one file to `/out` and
      exited 0, in 4.4–5.6 s. The correctness hash still fails, and that is
      correct: `track-*/src/main.rs` is the rule-05 scaffold, which emits
      `#SCAFFOLD_NOT_IMPLEMENTED` plus the line and byte count it read.
      Exact commands: `docs/KICKOFF-CARD.md`.
- [x] the PERF.md board-run table is ready to record the first run IDs
      (`## 1b. Board runs`, one row per live-board push, in step order)
- [x] `sh tooling/ci.sh` green and the working tree committed
- [x] the repo exists and is pushed: `pal-123456789/speed-demon`, **private**,
      default branch `speed-demon`, created from the official starter template
- [ ] **install the Speed Demon Board app on it** — human, in a browser, ~30 s
- [ ] **register and file the team form** — human, in a browser. No form, no
      score, and every team member must be registered separately.

## The immutable rules (recited at every decision point)

1. SHA-green before speed — determinism is correctness; the SHA gate is binary.
2. D = 1 forever — one crate costs K × 0.90.
3. S is per track folder — terse, documented, tight.
4. Scratchless — no /data writes, no cached outputs, one output file.
5. 12 threads max (`--cpuset-cpus=0-11`); oversubscription hurts at ~2.5 GHz.
6. Every PERF.md number is KNOWN (measured) or EXPECTED, with run IDs.
7. Tune to the distribution, never to sample bytes — the holdout seed has
   never been seen.