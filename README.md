# Speed Demon — Solo Campaign Monorepo (4 Tracks, Pre-Kickoff Build)

> **Status: PRE-KICKOFF SCAFFOLD — no project code committed before Oct 30 18:00 UTC.**
> All source under `track-*/src/` is a **correctness-green stub** that compiles offline
> via `rustc` (no cargo fetch). Real optimization replaces stubs at H0 inside the 72h window.
> Rules 04/05 explicitly permit reading, designing, and preparing the environment beforehand.
>
> **Start here: [`docs/CAMPAIGN-STATUS.md`](docs/CAMPAIGN-STATUS.md)** — the operational
> dashboard, the contract of deferred values, and the hour-by-hour runbook.

## Map

```
D:\SpeedDemon-Campaign\
├── Makefile                    # `make verify`, `make bench`, `make freeze-check`, `make ci`
├── LICENSE                     # MIT (OSI-approved, public at freeze)
├── .gitignore                  # excludes track-*/run, run.pdb, .exe build artifacts
├── track-a/                    # Track A — Ingest  (ref 32.05 s, timeout 321 s, S_ref 56 lines)
├── track-b/                    # Track B — Index   (ref 56.35 s, timeout 564 s)
├── track-c/                    # Track C — Join    (ref 14.44 s, timeout 144 s) — DQ minefield
├── track-d/                    # Track D — Traverse (ref 72.69 s, timeout 727 s)
│   (each: Dockerfile, build.sh, README.md, PERF.md, .speed-demon.toml, src/, verification/expected/)
├── verification/               # fsn1 harness replica, 3 gates, bench, lockdown, contract.toml, sample data
├── tooling/                    # k_calc, measure_axes, gen-fixtures, gen_docs, ci.sh, hooks, base-image.txt
└── docs/                       # MASTER-PLAN, per-track deep dives, CAMPAIGN-STATUS, RULE-CHANGES-oct28
```

Each track is a **self-contained build context**: `docker build --network=none
-t speeddemon/$TEAM-$TRACK:$COMMIT track-$TRACK` runs `COPY . /src` +
`RUN ./build.sh`, which compiles `src/main.rs` with `rustc` directly — no
cargo, no registry, no network.

## Quick start (Windows dev machine — Docker not required for scaffold check)

```powershell
# 1. Run the whole gate suite (sh from Git Bash / WSL / Linux)
sh tooling\ci.sh

# 2. Run the three gates on one track (correctness / timeout / sandbox)
powershell -ExecutionPolicy Bypass -File verification\verify.ps1 -Track A

# 3. The full pre-freeze lockdown (freeze-day gate; meaningful after Oct 30)
powershell -ExecutionPolicy Bypass -File verification\pre-freeze.ps1 -All

# 4. Price a candidate with the K calculator
python tooling\k_calc.py --t 30 --m 15 --s 0.62 --d 1

# 5. See where the S and D axes stand
python tooling\measure_axes.py --all

# 6. Generate anti-overfit fixtures
python tooling\gen-fixtures.py --all
```

> **Note on `pwsh` vs `powershell`.** These scripts run under Windows
> PowerShell 5.1 as well as PowerShell 7, and the `.ps1` files carry a UTF-8
> BOM so 5.1 parses the non-ASCII characters correctly. If you edit a `.ps1`
> file, re-run `python tooling/fix_ps1_bom.py`. Shell scripts (`tooling/*.sh`,
> `track-*/build.sh`) must stay **BOM-free** — a BOM before the shebang breaks
> direct execution on Linux.

## On the scoring box (Falkenstein fsn1, Debian 13 + Docker)

The build is **per track**: one image per track, built from the track folder as
the build context. Each image's entrypoint **is** that track's binary — there
is no dispatcher. The organizer's run command still passes `-e TRACK=$TRACK`,
which a per-track binary simply ignores.

```bash
# offline build — no apt, no cargo, no pip, no curl
docker build --network=none -t speeddemon/$TEAM-$TRACK:$COMMIT track-$TRACK

# the scored run, verbatim from the events page
docker run --rm --cpuset-cpus="0-11" --cpuset-mems="0" \
  --memory=8g --memory-swap=8g --pids-limit=512 --network=none --read-only \
  --tmpfs /tmp:rw,size=1g,exec --ulimit nofile=8192:8192 \
  -v /srv/speeddemon/data/track-$TRACK:/data:ro -v /srv/speeddemon/out/$RUN:/out:rw \
  -e TRACK=$TRACK speeddemon/$TEAM-$TRACK:$COMMIT

# the M axis, from the cgroup — never from `ps` or `time`
cat /sys/fs/cgroup/system.slice/docker-*.scope/memory.peak

# the correctness gate
sha256sum /out/*
```

Every `FROM` must name an image from `speeddemon.dev/allowlist.json`, pinned by
digest, and nothing else may enter the image: no `COPY --from` of another
image, no `ADD` of a URL. This repository pins both stages — the toolchain
build stage and the runtime stage — and `tooling/check_base_pinned.py` verifies
every Dockerfile against `tooling/base-image.txt`:

- build:   `rust:1-slim-trixie@sha256:1a939748a7cbab172de91a014e3804a97af1394a8c844faa67fce9401c4e40fb`
- runtime: `debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad`

Builds stay offline under `--network=none`, are capped at 10 minutes, and
images at 2 GB. The rust image ships the toolchain, so nothing is vendored and
no build-time network is needed.

## Doctrine (enforced by hooks)

1. **SHA-green before speed.** Every optimization re-runs the SHA gate.
2. **Deterministic output** — no HashMap iteration, no float reduction, fixed merge order.
3. **D=1** — std-only Rust, no non-stdlib runtime dependencies. The Oct 28
   formula is `D = 1 + direct deps`; one dependency makes D = 2 and multiplies
   K by `(1/2)^0.15 ≈ 0.90` — a ~10% cut for every crate.
4. **Buffered reads, not mmap** — mmap may inflate `memory.peak`.
5. **Honest claims** — every number in PERF.md is `KNOWN` (measured) or `EXPECTED`.
6. **One image per track** — each entrypoint is its track's binary; each track
   folder is a self-contained context, so a broken build costs one track, not four.

The pre-commit hook enforces 1, 2, 5, and the offline build:

```bash
sh tooling/install-hooks.sh
```

## Scoring model

Four axes, weights summing to 1.0:

```
K = (T_ref/T)^0.50 × (M_ref/M)^0.20 × (S_ref/S)^0.15 × (D_ref/D)^0.15
```

- **T** wall time (0.50) — no cap.
- **M** peak memory (0.20) — capped at 50×.
- **S** source size (0.15) — code lines counted by `scc` after your formatter,
  **across each track's folder** (which is all the build can see), minus
  registry packages first published before Oct 1, 2026 by people outside the
  team. It can cost you but never pay (factor capped at 1.0).
- **D** dependencies (0.15) — `D = 1 + direct non-stdlib runtime dependencies`
  declared in the manifest. One dependency ⇒ D = 2 ⇒ K × 0.90. It can cost you
  but never pay.

Speed score = `50 × log10(K)`, clamped to 0–100: K = 10 is half marks and
K = 100 is full marks.

**Gates (three):** correctness (SHA-256 match), the timeout (10× the reference
time), and the sandbox (no network, read-only dataset, one output file).
The speed floor gate is gone.

**Overall title:** the geometric mean of your best three track totals, from
three or more tracks entered. Ties on the final total are broken by raw K, then
by wall time, then by earliest submission commit.

**Live board:** install the Speed Demon Board GitHub App on your repo; one run
per team per 15 minutes; the board closes at code freeze. Final scoring is on a
holdout seed. PERF.md steps must cite live-board run IDs.

## Timeline

| When (UTC) | What |
|---|---|
| Oct 28 18:00 | Specs + refs + harness frozen — pin all deferred values in `verification/contract.toml` |
| Oct 30 06:00 | Sample datasets + expected hashes — load into `track-*/verification/expected/` |
| Oct 30 18:00 | **KICKOFF — H0** — replace stubs with real baselines, first SHA-green |
| ~H17 | **All 4 SHA-green + deterministic** — tag `baseline-green` |
| Nov 02 18:00 | **CODE FREEZE** — public repo, OSI license, final commit; the live board closes |
| Nov 02–04 | Scoring (holdout seed) — gate pass/fail published by Nov 04 |
| Nov 03–12 | Judging; Nov 10 write-up deadline; Nov 13 winners announced |

See [docs/CAMPAIGN-STATUS.md](docs/CAMPAIGN-STATUS.md) for the runbook, and
[docs/MASTER-PLAN.md](docs/MASTER-PLAN.md) for the full 72h schedule and
per-track deep dives. The digested Oct 26 rule changes live in
[docs/RULE-CHANGES-oct28.md](docs/RULE-CHANGES-oct28.md).