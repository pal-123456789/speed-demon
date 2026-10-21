# Speed Demon — Solo Campaign Monorepo (5 Tracks, Pre-Kickoff Build)

> **Status: PRE-KICKOFF SCAFFOLD — no project code committed before Oct 30 18:00 UTC.**
> All source under `tracks/*/src/` is a **correctness-green stub** that compiles offline
> via `rustc` (no cargo fetch). Real optimization replaces stubs at H0 inside the 72h window.
> Rules 04/05 explicitly permit reading, designing, and preparing the environment beforehand.
>
> **Start here: [`docs/CAMPAIGN-STATUS.md`](docs/CAMPAIGN-STATUS.md)** — the operational
> dashboard, the contract of deferred values, and the hour-by-hour runbook.

## Map

```
D:\SpeedDemon-Campaign\
├── Cargo.toml                  # workspace root (core + all tracks, offline)
├── rust-toolchain.toml         # pinned channel (1.86 — deterministic builds)
├── Makefile                    # `make verify`, `make bench`, `make freeze-check`
├── Dockerfile                  # THE SCORED IMAGE: one image, all five tracks
├── build-all.sh                # offline build of core + 5 binaries (rustc only)
├── run                         # runtime dispatcher: reads $TRACK, execs the binary
├── LICENSE                     # MIT (OSI-approved, public at freeze)
├── .gitignore
├── core/                       # zero-dependency shared library (arena, buf-reader, det-writer, pool, simd)
├── tracks/
│   ├── A-ingest/               # Track A — Ingest (312 MB, 3.1M lines, ref 41.2s)
│   ├── B-index/                # Track B — Index  (250 MB corpus, 50k queries, ref 58.7s)
│   ├── C-join/                 # Track C — Join   (204 MB hostile CSV, ref 36.9s) — DQ minefield
│   ├── D-traverse/             # Track D — Traverse (5M edges, 2k queries, ref 72.4s)
│   └── E-compress/             # Track E — Compress (100 MiB, ref 29.8s, M=size)
│   (each: Dockerfile, build.sh, PERF.md, README.md, .speed-demon.toml, src/)
├── verification/               # HEL1 harness replica, 4 gates, benchmark, lockdown, contract.toml
├── tooling/                    # k_calc, measure_axes, gen-fixtures, gen_docs, vendor-toolchain, hooks
└── docs/                       # MASTER-PLAN, per-track deep dives, CAMPAIGN-STATUS
```

## Quick start (Windows dev machine — Docker not required for scaffold check)

```powershell
# 1. Build everything offline and run the core unit tests
cargo build --release --offline
cargo test --offline

# 2. Run the four gates on one track (correctness / speed floor / timeout / sandbox)
powershell -ExecutionPolicy Bypass -File verification\verify.ps1 -Track A

# 3. The full 81-check pre-freeze lockdown
powershell -ExecutionPolicy Bypass -File verification\pre-freeze.ps1 -All

# 4. Price a candidate with the K calculator
python tooling\k_calc.py --t 30 --m 15 --s 0.62 --d 1 --x 1.05

# 5. See where the S and D axes stand
python tooling\measure_axes.py --all

# 6. Generate anti-overfit fixtures
python tooling\gen-fixtures.py --all
```

> **Note on `pwsh` vs `powershell`.** These scripts run under Windows
> PowerShell 5.1 as well as PowerShell 7, and the `.ps1` files carry a UTF-8
> BOM so 5.1 parses the non-ASCII characters correctly. If you edit a `.ps1`
> file, re-run `python tooling/fix_ps1_bom.py`.

## On HEL1 (Debian 13 + Docker, scoring replica)

The scored image carries **all five tracks** and dispatches on `$TRACK` at run
time, because the harness builds one image and passes `-e TRACK=$TRACK`.

```bash
# offline build — no apt, no cargo, no pip, no curl
docker build --network=none -t speeddemon/solo:final .

# the scored run, verbatim from the harness
docker run --rm --cpuset-cpus="0-11" --cpuset-mems="0" \
  --memory=8g --memory-swap=8g --pids-limit=512 --network=none --read-only \
  --tmpfs /tmp:rw,size=1g,exec --ulimit nofile=8192:8192 \
  -v /srv/speeddemon/data/track-A:/data:ro -v $PWD/out:/out:rw \
  -e TRACK=A speeddemon/solo:final

# the M axis, from the cgroup — never from `ps` or `time`
cat /sys/fs/cgroup/system.slice/docker-*.scope/memory.peak

# the correctness gate
sha256sum out/*
```

**Before the hack, on a networked machine:**

```bash
sh tooling/vendor-toolchain.sh        # vendors rustc into vendor/toolchain/
```

`debian:13-slim` ships no rustc, and the scoring build runs with
`--network=none`, so a Dockerfile that fetches anything fails the build — and
a failed build scores zero on **every** track, because the image is shared.

## Doctrine (enforced by hooks)

1. **SHA-green before speed.** Every optimization re-runs the SHA gate.
2. **Deterministic output** — no HashMap iteration, no float reduction, fixed merge order.
3. **D=1** — std-only Rust, no non-stdlib deps (free +10–17% on K, lean prize).
4. **Buffered reads, not mmap** — mmap may inflate `memory.peak`.
5. **Honest claims** — every number in PERF.md is `KNOWN` (measured) or `EXPECTED`.
6. **One image, five tracks** — the entrypoint dispatches on `$TRACK`.

The pre-commit hook enforces 1, 2, 5, and the offline build:

```bash
sh tooling/install-hooks.sh
```

## Timeline

| When (UTC) | What |
|---|---|
| Oct 28 18:00 | Specs + refs + harness frozen — pin all deferred values in `verification/contract.toml` |
| Oct 30 06:00 | 10% sample + expected SHA — load into `tracks/*/verification/expected/` |
| Oct 30 18:00 | **KICKOFF — H0** — replace stubs with real baselines, first SHA-green |
| ~H17 | **All 5 SHA-green + deterministic** — tag `baseline-green` |
| Nov 02 18:00 | **FREEZE** — public repo, OSI license, final commit |

See [docs/CAMPAIGN-STATUS.md](docs/CAMPAIGN-STATUS.md) for the runbook, and
[docs/MASTER-PLAN.md](docs/MASTER-PLAN.md) for the full 72h schedule and
per-track deep dives.
