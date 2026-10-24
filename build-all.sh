#!/bin/sh
# Build ALL FIVE track binaries into ./bin.
#
# This is the build the SCORED image uses. It is a loop over `build-one.sh`,
# which is the single implementation of the offline build — so the per-track
# standalone images and the scored image cannot drift apart.
#
# The core crate is compiled once as an rlib and linked into each track
# binary with --extern, so we pay the core's compile cost once, not five
# times. No cargo, no network, no registry.
set -eu

APP_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$APP_DIR"

echo "[build-all] building all five tracks"

# The first track compiles the core; the rest reuse it.
SD_REUSE_CORE=0
for TRACK in A B C D E; do
    SD_REUSE_CORE="$SD_REUSE_CORE" sh "$APP_DIR/build-one.sh" "$TRACK"
    SD_REUSE_CORE=1
done

echo "[build-all] ok:"
ls -l "$APP_DIR/bin"
