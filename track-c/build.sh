#!/bin/sh
# Offline build for THIS track folder. rustc only, no cargo, no registry,
# no network: the scored build runs with --network=none inside the official
# allowlisted build stage, which already ships rustc.
#
# -C target-cpu=x86-64-v3: the scoring CPU is a Core i5-13500 (AVX2, BMI2,
# FMA; AVX-512 fused off), so v3 is exactly right and safe on every host this
# runs on.
#
# PGO. The contract allows build-time training on generator output --
#   "Build-time training (PGO, a JVM AOT cache, native-image) is allowed on
#    generator output. /data is not mounted at build time, and a byte-identical
#    copy of our generator in your repo costs nothing in source size."
# -- and `gen_c.py` beside this script is a byte-identical copy of the
# organizers' Track A generator, so it is free in S. Training runs on SAMPLE
# scale output generated here, at seed 1, because /data is not mounted.
#
# THE FALLBACK IS THE POINT. Every step is checked and any failure drops to a
# plain build. A scored build that FAILS is a rejected track -- worse than a
# slow one -- so PGO must never be able to break the build. If the profiler or
# python3 is missing, or the profile cannot be merged, we still produce `run`.
#
# -pgo-warn-missing-function turns a mismatched or bogus profile from a silent
# no-op into a visible warning, so a broken profile cannot quietly do nothing.
set -eu
cd "$(dirname "$0")"

COMMON="--edition 2021 -C opt-level=3 -C codegen-units=1 -C panic=abort
        -C target-cpu=x86-64-v3"

plain() {
    # shellcheck disable=SC2086
    rustc $COMMON -C strip=symbols -o run src/main.rs
}

pgo() {
    command -v llvm-profdata >/dev/null 2>&1 || return 1
    command -v python3 >/dev/null 2>&1 || return 1
    [ -f gen_c.py ] || return 1

    rm -rf /tmp/pgo /tmp/tr /tmp/to
    mkdir -p /tmp/pgo /tmp/to
    python3 gen_c.py --seed 1 --scale sample --out /tmp/tr >/dev/null 2>&1 || return 1

    # 1. instrumented build
    # shellcheck disable=SC2086
    rustc $COMMON -C profile-generate=/tmp/pgo -o run-gen src/main.rs || return 1
    # 2. train on generator output
    SD_DATA_DIR=/tmp/tr SD_OUT_DIR=/tmp/to ./run-gen >/dev/null 2>&1 || return 1
    # 3. merge raw profiles with the MATCHING llvm-profdata
    llvm-profdata merge -o /tmp/pgo/merged.profdata /tmp/pgo >/dev/null 2>&1 || return 1
    # 4. build again, using the profile
    # shellcheck disable=SC2086
    rustc $COMMON -C profile-use=/tmp/pgo/merged.profdata \
          -C llvm-args=-pgo-warn-missing-function \
          -C strip=symbols -o run src/main.rs || return 1

    rm -f run-gen
    return 0
}

if pgo; then
    echo "[build] profile-guided"
else
    echo "[build] plain (PGO unavailable)"
    plain
fi
