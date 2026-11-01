#!/bin/sh
# Offline build for THIS track folder. rustc only, no cargo, no registry,
# no network: the scored build runs with --network=none inside the official
# rust:1-slim-trixie build stage, which already ships rustc. Nothing needs
# downloading, so there is nothing to vendor.
#
# -C target-cpu=native: the scoring CPU is a Core i5-13500 (AVX2, BMI2,
# FMA; no AVX-512), so v3 is exactly right and safe on every host this runs
# on. Change #24 confirms the organizers build native-CPU tracks ON THE
# SCORING BOX, so -C target-cpu=native is supported too; v3 is the
# risk-free choice that still emits AVX2 for the SIMD paths.
#
# PGO (optional, build time is free): see docs/depth/93-PGO-AND-CODEGEN.md.
# It needs the llvm-tools build stage (change #23) and buys ~5-15% on T.
set -eu
cd "$(dirname "$0")"
rustc --edition 2021 \
      -C opt-level=3 -C codegen-units=1 \
      -C panic=abort -C strip=symbols \
      -C target-cpu=native \
      -o run src/main.rs