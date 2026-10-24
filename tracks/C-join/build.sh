#!/bin/sh
# Track C offline build.
#
# This is a thin wrapper. The single implementation of the offline build
# lives in build-one.sh at the repository root, so the per-track standalone
# images and the scored image cannot drift apart.
#
# It used to contain its own copy of the rustc invocation, and that copy
# carried three defects that only surfaced when somebody actually ran it:
# a hyphenated `--crate-name` (rustc rejects it), an rlib compiled for the
# host target while the binary linked for musl (E0461), and the unstable
# `-C linker-flavor=gnu-lld`. Wrapping removes that whole class of bug.
#
# Needs no network: rustc only, no cargo, no registry.
set -eu

TRACK_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$TRACK_DIR/../.." && pwd)"

exec sh "$ROOT/build-one.sh" C