# WINDOW RUNBOOK — Oct 30 18:00 → Nov 2 18:00 UTC (72 hours)

> The single document for the window. Every tool below is committed and
> gate-verified. The goal of H0–H12 is **a SHA-green single-thread baseline on
> all funded tracks before ANY optimization** — a slow green beats a fast gate
> failure, and everything after is measured from there.
>
> Design depth: `docs/depth/` (per-track DEEP files, the model digest
> `97-MODEL-OUTPUT-DIGEST.md`, the events-page pins `99-EVENTS-PAGE-PINS.md`).
> The scoring formula is the compass; `k_whatif.py` is the calculator.

## The tool map

| When | Tool | What it gives you |
|---|---|---|
| H0 (once) | `sh tooling/h0-launch.sh` | build all → gates → fixtures → kit → pins, in one run |
| H0–H2 | `python tooling/spec_intake.py --check` | the Oct-28 pins applied; **0 blocking** = intake done |
| Every rung | `sh tooling/bench-rung.sh A --label rung-07` | build → mini determinism → SHA-check → 5-run bench → ledger |
| Every rung | `python tooling/k_whatif.py --scoreboard --t A=<s> --m A=<MB> --l A=<lines>` | K / speed / track total / best-3 geomean |
| Every ~8 h | `python tooling/k_whatif.py --waterfill --hours H --base … --doubling …` | re-solve the hour budget from measured rates |
| H50+ | `sh tooling/determinism-check.sh all` | the full P6 matrix before every green commit |
| Before freeze | `sh tooling/ci.sh` | all 15 gates, then `verification/pre-freeze.ps1 -All` / `lockdown` |
| Freeze | git push | public repo + OSI license + final commit; PERF.md cites run IDs |

## Hour-by-hour

### H0–H2 — intake (no optimization)
1. `sh tooling/h0-launch.sh` — state, builds, gates, fixtures, kit, pins.
2. Read the four frozen specs AND the reference implementations. Write down,
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

## The immutable rules (recited at every decision point)

1. SHA-green before speed — determinism is correctness; the SHA gate is binary.
2. D = 1 forever — one crate costs K × 0.90.
3. S is per track folder — terse, documented, tight.
4. Scratchless — no /data writes, no cached outputs, one output file.
5. 12 threads max (`--cpuset-cpus=0-11`); oversubscription hurts at ~2.5 GHz.
6. Every PERF.md number is KNOWN (measured) or EXPECTED, with run IDs.
7. Tune to the distribution, never to sample bytes — the holdout seed has
   never been seen.