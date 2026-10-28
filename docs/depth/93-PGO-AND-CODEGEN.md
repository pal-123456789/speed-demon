# PGO & CODEGEN — the free build-time wins (change #23, #24)

> **Design/infrastructure, no solution code (rules 04/05).** Build time is not
> scored: "if you want to spend six of your ten minutes on profile-guided
> optimisation, that is free." This file is the recipe, the cost model, and
> the risk controls — decide with `k_whatif.py --tradeoff`, not by feel.

---

## 1. What the rules now allow

| Fact | Source |
|---|---|
| **PGO is allowed** on generator output; `/data` is not mounted at build time; a byte-identical copy of the organizers' generator in your folder is **free** in source size | change #16 |
| **`halloumee/speed-demon-rust:1.99.1-trixie-llvm-tools@sha256:13e4ee7135d8eac6c56476c7d1aee201f36479f4745fd5d0e6b4529f4e13251a`** is on the allowlist — `rust:1-trixie` **with the llvm-tools component**, so `llvm-profdata` matches `rustc` | change #23 |
| **Native CPU builds are supported**: a track that asks for `-C target-cpu=native` (or `-march=native`) is **built on the scoring box itself**, so native means the i5-13500 (AVX2, no AVX-512) | change #24 |
| Build capped at **10 minutes**, image at **2 GB** | change #14 |
| PGO/AOT/native-image may train on generator output inside the build | change #16 |

`tooling/base-image.txt` carries all three digests (runtime, standard rust
build stage, llvm-tools build stage) and `tooling/check_base_pinned.py`
enforces them.

## 2. The codegen baseline (already in every `build.sh`)

```
-C opt-level=3 -C codegen-units=1 -C panic=abort -C strip=symbols
-C target-cpu=x86-64-v3          # AVX2 + BMI2 + FMA, exactly the scoring CPU's level
```

`x86-64-v3` is the **risk-free** choice: it emits AVX2/FMA/BMI2 (what the
i5-13500 has) and still runs on any modern x86-64 host, including this dev
machine. `-C target-cpu=native` is now also supported by the organizers
(change #24) and can buy a little more (e.g. tuning for the specific cache
sizes); treat it as an **H24+ experiment**, because it depends on their
build-on-the-scoring-box path being applied to your track.

**Verified:** switching to `-C target-cpu=x86-64-v3` builds, runs, and
reproduces every committed pin byte-for-byte — codegen flags are
behaviour-neutral, so they cannot break the SHA gate.

## 3. The PGO recipe (offline, inside the Docker build)

`track-<t>/Dockerfile` (only for a track doing PGO):

```dockerfile
FROM halloumee/speed-demon-rust:1.99.1-trixie-llvm-tools@sha256:13e4ee7135d8eac6c56476c7d1aee201f36479f4745fd5d0e6b4529f4e13251a AS build
COPY . /src
WORKDIR /src
RUN ./build-pgo.sh

FROM debian:13-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
COPY --from=build /src/run /app/run
ENTRYPOINT ["/app/run"]
```

`track-<t>/build-pgo.sh` (sketch — write it in-window):

```sh
set -eu; cd "$(dirname "$0")"
# 1. instrumented build
rustc --edition 2021 -C opt-level=3 -C codegen-units=1 -C panic=abort \
      -C target-cpu=x86-64-v3 \
      -C profile-generate=/tmp/pgo -o run-gen src/main.rs
# 2. train on GENERATOR OUTPUT (never /data: it is not mounted at build time)
python3 gen/<track>_gen.py --seed 1 --scale sample --out /tmp/train
SD_DATA_DIR=/tmp/train SD_OUT_DIR=/tmp/out ./run-gen
# 3. merge the raw profiles with the matching llvm-profdata
llvm-profdata merge -o /tmp/pgo/merged.profdata /tmp/pgo
# 4. optimized build using the profile
rustc --edition 2021 -C opt-level=3 -C codegen-units=1 -C panic=abort \
      -C target-cpu=x86-64-v3 \
      -C profile-use=/tmp/pgo/merged.profdata \
      -C llvm-args=-pgo-warn-missing-function \
      -o run src/main.rs
```

- **Step 2 is the one that must be right.** The profile has to be
  representative: a 10% sample at seed 1 is the obvious choice. Training on
  `/data` is impossible (not mounted) and on sample *bytes* is pointless
  (holdout), so train on generator output generated inside the build.
- **`-pgo-warn-missing-function`** turns a mismatched/bogus profile from a
  silent no-op into a visible warning. Do not omit it.
- **Cost:** the generator copy is free in S (change #16); the `build-pgo.sh`
  script itself is ~15–25 lines and **does** count in S. Price it:
  `python tooling/k_whatif.py --tradeoff --t-abs <T> --m-abs <M> --lines <L> --add-lines 20 --t-gain 8`.

## 4. The decision rule (do not guess)

```
K(before) = (Tref/T)^0.50 · (Mref/M)^0.20 · (Sref/S)^0.15 · 1
K(after)  = (Tref/(T·(1-g)))^0.50 · (Mref/M')^0.20 · (Sref/(S+dl))^0.15 · 1
```

- T gain `g` from PGO: expect **5–15%** on a parser/aggregator-shaped hot loop
  (measure it; `bench-rung.sh` gives the 5-run middle-3).
- S cost `dl`: the script length; the S exponent is 0.15 and capped at 1.0, so
  with S already ≫ Sref each extra line multiplies K by roughly
  `(1 + dl/S)^-0.15`. For S = 150 and dl = 20 that is ≈ 0.98 — i.e. **a 2%
  K cost for a 5–15% T gain** (T's exponent is 0.50, so 8% T is ≈ +2.0 speed
  points). PGO wins comfortably **if** the measured gain is real.
- Also check the M side: PGO changes code layout and can shift peak memory
  slightly (usually down). Re-measure M in the same run.
- **Adopt only after** `bench-rung.sh` shows the gain and the pin is
  unchanged; otherwise revert (that failed experiment belongs in PERF.md).

## 5. Risk controls

| Risk | Control |
|---|---|
| Profile mismatched → silently no optimization | `-C llvm-args=-pgo-warn-missing-function`; compare `perf stat` instruction counts before/after |
| Training reads `/data` → build fails (not mounted) | generate training data **inside** the build with the committed generator |
| Build exceeds 10 min | PGO on a ≤300-line program takes seconds; keep training at `sample` scale |
| PGO changes the output | it must not (it only guides optimization) — the pin check proves it; if the pin moves, revert and investigate |
| Image > 2 GB | runtime stage is unchanged (`debian:13-slim` + one binary) |
| `target-cpu=native` on a mismatched build host → SIGILL | use `x86-64-v3` for the safe path; native only via the organizers' scoring-box build (change #24) |
| Generator copy inflates S | byte-identical copies of published generators are explicitly free (change #16) — keep them byte-identical, do not edit |

## 6. In-window order of operations

1. H2–H12: get SHA-green with the plain `build.sh` (v3 already on).
2. H12–H24: measure the baseline with `bench-rung.sh`; record T and M.
3. H24+: **only if** T is dominated by one hot loop, try PGO per §3, measure,
   and price it with `--tradeoff`. Keep it only if the points are real.
4. Record the attempt either way in PERF.md (a reverted PGO experiment is
   exactly the kind of entry the judges reward).
