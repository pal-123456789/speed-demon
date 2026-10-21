# Speed Demon — Solo Campaign Monorepo (5 Tracks, Pre-Kickoff Build)

> **Status: PRE-KICKOFF SCAFFOLD — no project code committed before Oct 30 18:00 UTC.**
> All source under `tracks/*/src/` is a **correctness-green stub** that compiles offline
> via `rustc` (no cargo fetch). Real optimization replaces stubs at H0 inside the 72h window.
> Rules 04/05 explicitly permit reading, designing, and preparing the environment beforehand.

## Map

```
D:\SpeedDemon-Campaign\
├── Cargo.toml                  # workspace root (core + all tracks, offline)
├── rust-toolchain.toml         # pinned channel (1.86 stable — deterministic builds)
├── Makefile                    # `make verify`, `make bench`, `make freeze-check`
├── LICENSE                     # MIT (OSI-approved, public at freeze)
├── .gitignore
├── core/                       # zero-dependency shared library (arena, buf-reader, det-writer, pool, simd)
├── tracks/
│   ├── A-ingest/               # Track A — Ingest (312 MB, 3.1M lines, ref 41.2s)
│   ├── B-index/                # Track B — Index  (250 MB corpus, 50k queries, ref 58.7s)
│   ├── C-join/                 # Track C — Join   (204 MB hostile CSV, ref 36.9s) — DQ minefield
│   ├── D-traverse/             # Track D — Traverse (5M edges, 2k queries, ref 72.4s)
│   └── E-compress/             # Track E — Compress (100 MiB, ref 29.8s, M=size)
├── verification/               # local HEL1 harness replica + SHA gate + determinism
├── tooling/                    # K-calculator, S-measure, D-measure
└── docs/                       # frozen plan + per-track deep dives (reference only)
```

## Quick start (Windows dev machine — Docker not required for scaffold check)

```powershell
# 1. Verify the scaffold compiles offline (no network)
.\verification\verify.ps1 -Track A

# 2. Full 5-track offline build + determinism check
.\verification\pre-freeze.ps1

# 3. K-coefficient calculator
python tooling\k_calc.py --help
```

## On HEL1 (Debian 13 + Docker, scoring replica)

```bash
docker build --network=none -t speeddemon/test:A -f tracks/A-ingest/Dockerfile .
docker run --rm --cpuset-cpus="0-11" --cpuset-mems="0" --memory=8g --memory-swap=8g \
  --pids-limit=512 --network=none --read-only --tmpfs /tmp:rw,size=1g,exec \
  --ulimit nofile=8192:8192 -v /srv/speeddemon/data/track-A:/data:ro \
  -v $PWD/out:/out:rw -e TRACK=A speeddemon/test:A
cat /sys/fs/cgroup/system.slice/docker-*.scope/memory.peak  # cgroup v2
sha256sum out/*
```

## Doctrine (enforced by hooks)

1. **SHA-green before speed.** Every optimization re-runs the SHA gate.
2. **Deterministic output** — no HashMap iteration, no float reduction, fixed merge order.
3. **D=1** — std-only Rust, no non-stdlib deps (free +10–17% on K, lean prize).
4. **Buffered reads, not mmap** — mmap may inflate `memory.peak`.
5. **Honest claims** — every number in PERF.md is `KNOWN` (measured) or `EXPECTED`.

## Timeline

| When (UTC) | What |
|---|---|
| Oct 28 18:00 | Specs + refs + harness frozen — pin all deferred values |
| Oct 30 06:00 | 10% sample + expected SHA — load into `tracks/*/verification/expected/` |
| Oct 30 18:00 | **KICKOFF — H0** — replace stubs with real baselines, first SHA-green |
| ~H17 | **All 5 SHA-green + deterministic** — tag `baseline-green` |
| Nov 02 18:00 | **FREEZE** — public repo, OSI license, final commit |

See [docs/MASTER-PLAN.md](docs/MASTER-PLAN.md) for the full 72h schedule and
per-track deep dives.
