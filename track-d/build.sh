#!/bin/sh
# Offline build for THIS track folder. rustc only, no cargo, no registry, no network.
#
# -C target-cpu=x86-64-v3: the scoring CPU is a Core i5-13500 (AVX2, BMI2, FMA; no AVX-512).
#
# PGO. The contract allows build-time training on generator output, and gen_d.py beside this script
# is a byte-identical copy of the organizers' Track D generator, so it is free in S. This script
# previously priced PGO at "~5-15% on T" and then never ran it, because the build stage had no
# llvm-profdata; the Dockerfile now uses the allowlisted llvm-tools image, the same one Track A and
# Track C build against. Every step is checked and any failure drops to a plain build: a scored
# build that FAILS is a rejected track, worse than a slow one, so PGO must never break the build.
set -eu
cd "$(dirname "$0")"
COMMON="--edition 2021 -C opt-level=3 -C codegen-units=1 -C panic=abort -C target-cpu=x86-64-v3"
plain() { rustc $COMMON -C strip=symbols -o run src/main.rs; }
pgo() {
    command -v llvm-profdata >/dev/null 2>&1 || return 1
    command -v python3 >/dev/null 2>&1 || return 1
    [ -f gen_d.py ] || return 1
    rm -rf /tmp/pgo /tmp/tr /tmp/to; mkdir -p /tmp/pgo /tmp/to
    python3 gen_d.py --seed 1 --scale sample --out /tmp/tr >/dev/null 2>&1 || return 1
    rustc $COMMON -C profile-generate=/tmp/pgo -o run-gen src/main.rs || return 1
    SD_DATA_DIR=/tmp/tr SD_OUT_DIR=/tmp/to ./run-gen >/dev/null 2>&1 || return 1
    llvm-profdata merge -o /tmp/pgo/merged.profdata /tmp/pgo >/dev/null 2>&1 || return 1
    rustc $COMMON -C profile-use=/tmp/pgo/merged.profdata \
          -C llvm-args=-pgo-warn-missing-function -C strip=symbols -o run src/main.rs || return 1
    rm -f run-gen
}
if pgo; then echo "[build] profile-guided"; else echo "[build] plain (PGO unavailable)"; plain; fi
