# Speed Demon — Solo Campaign Monorepo (4 Tracks, Pre-Kickoff Build)

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
├── Dockerfile                  # THE SCORED IMAGE: one image, all four tracks
├── build-all.sh                # offline build of core + 4 binaries (rustc only)
├── run                         # runtime dispatcher: reads $TRACK, execs the binary
├── LICENSE                     # MIT (OSI-approved, public at freeze)
├── .gitignore
├── core/                       # zero-dependency shared library (arena, buf-reader, det-writer, pool, simd)
├── tracks/
│   ├── A-ingest/               # Track A — Ingest (239 MB, 3.1M lines, ref 32.05s)
│   ├── B-index/                # Track B — Index  (250 MB corpus, 50k queries, ref 56.35s)
│   ├── C-join/                 # Track C — Join   (204 MB hostile CSV, ref 14.44s) — DQ minefield
│   └── D-traverse/             # Track D — Traverse (5M edges, 2k queries, ref 72.69s)
│   (each: Dockerfile, build.sh, PERF.md, README.md, .speed-demon.toml, src/)
├── verification/               # fsn1 harness replica, 3 gates, benchmark, lockdown, contract.toml
├── tooling/                    # k_calc, measure_axes, gen-fixtures, gen_docs, vendor-toolchain, hooks
└── docs/                       # MASTER-PLAN, per-track deep dives, CAMPAIGN-STATUS
```

## Quick start (Windows dev machine — Docker not required for scaffold check)

```powershell
# 1. Build everything offline and run the core unit tests
cargo build --release --offline
cargo test --offline

# 2. Run the three gates on one track (correctness / timeout / sandbox)
powershell -ExecutionPolicy Bypass -File verification\verify.ps1 -Track A

# 3. The full pre-freeze lockdown
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
> file, re-run `python tooling/fix_ps1_bom.py`.

## On the scoring box (Falkenstein fsn1, Debian 13 + Docker)

The scored image carries **all four tracks** and dispatches on `$TRACK` at run
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

Every `FROM` must name an image from `speeddemon.dev/allowlist.json`, pinned by
digest, and nothing else may enter the image: no `COPY --from` of another
image, no `ADD` of a URL. The published example pins
`debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad`.
Builds stay offline and are capped at 10 minutes; images are capped at 2 GB.

## Doctrine (enforced by hooks)

1. **SHA-green before speed.** Every optimization re-runs the SHA gate.
2. **Deterministic output** — no HashMap iteration, no float reduction, fixed merge order.
3. **D=1** — std-only Rust, no non-stdlib runtime dependencies (each one would
   make D = 2 and cost 10% of K).
4. **Buffered reads, not mmap** — mmap may inflate `memory.peak`.
5. **Honest claims** — every number in PERF.md is `KNOWN` (measured) or `EXPECTED`.
6. **One image, four tracks** — the entrypoint dispatches on `$TRACK`.

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
  across the whole repo minus registry packages first published before
  Oct 1, 2026 by people outside the team. It can cost you but never pay.
- **D** dependencies (0.15) — `D = 1 + the number of direct non-stdlib runtime
  dependencies in your manifest`. One dependency ⇒ D = 2 ⇒ contributes
  2⁻⁰·¹⁵ = 0.90 to K. It can cost you but never pay.

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
| Oct 30 06:00 | Sample datasets + expected hashes — load into `tracks/*/verification/expected/` |
| Oct 30 18:00 | **KICKOFF — H0** — replace stubs with real baselines, first SHA-green |
| ~H17 | **All 4 SHA-green + deterministic** — tag `baseline-green` |
| Nov 02 18:00 | **CODE FREEZE** — public repo, OSI license, final commit; the live board closes |
| Nov 02–04 | Scoring (holdout seed) — gate pass/fail published by Nov 04 |
| Nov 03–12 | Judging; Nov 10 write-up deadline; Nov 13 winners announced |

See [docs/CAMPAIGN-STATUS.md](docs/CAMPAIGN-STATUS.md) for the runbook, and
[docs/MASTER-PLAN.md](docs/MASTER-PLAN.md) for the full 72h schedule and
per-track deep dives.
