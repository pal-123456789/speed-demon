#!/bin/sh
# Re-pin a track's LOCAL expected-hash file from the CURRENT binary's output.
#
# TWO KINDS OF PIN — do not confuse them:
#
#   * track-X/verification/expected/<output>.sha256
#       THE OFFICIAL EXPECTED HASH, published by the organizers for the
#       official sample (seed 1). It is the SHA gate the board applies and the
#       target your implementation must hit. `tooling/official-data.py` writes
#       it; THIS SCRIPT REFUSES TO OVERWRITE IT (use --force only if you know
#       the organizers republished the data).
#
#   * track-X/verification/expected/<output>.local.sha256
#       a local self-consistency pin for your own iteration, written by this
#       script. Useful while the scaffold or a half-finished rung is in place;
#       it is NOT the scoring target.
#
# Usage:  sh tooling/pin-hash.sh a|b|c|d [--force]
# Needs : the binary built (sh track-X/build.sh) and data present
#         (verification/data/track-X — `python tooling/official-data.py` or
#          `python tooling/gen-fixtures.py --all`)
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
t="${1:-}"
force="${2:-}"
t="$(echo "$t" | tr '[:upper:]' '[:lower:]')"
case "$t" in
    a) want=summary.txt  ;;
    b) want=results.txt   ;;
    c) want=aggregates.txt    ;;
    d) want=distances.txt ;;
    *) echo "usage: pin-hash.sh a|b|c|d [--force]" >&2; exit 2 ;;
esac

data="$ROOT/verification/data/track-$t"
[ -d "$data" ] || { echo "no sample data at verification/data/track-$t — run gen-fixtures" >&2; exit 2; }
[ -f "$ROOT/track-$t/run" ] || { echo "no binary at track-$t/run — run sh track-$t/build.sh" >&2; exit 2; }

out="$(mktemp -d 2>/dev/null || echo "/tmp/sd-pin-$t-$$")"
SD_DATA_DIR="$data" SD_OUT_DIR="$out" "$ROOT/track-$t/run" >/dev/null 2>&1
rc=$?
if [ "$rc" -ne 0 ]; then
    echo "binary exited $rc — the track is not producing output" >&2
    rm -rf "$out"
    exit 1
fi
files="$(find "$out" -maxdepth 1 -type f)"
n="$(printf '%s\n' "$files" | sed '/^$/d' | wc -l | tr -d ' ')"
if [ "$n" != "1" ]; then
    echo "expected exactly 1 output file, got $n" >&2
    rm -rf "$out"
    exit 1
fi
f="$(printf '%s\n' "$files" | sed '/^$/d')"
outname="$(basename "$f")"
if [ "$outname" != "$want" ]; then
    echo "output file is $outname, expected $want" >&2
    rm -rf "$out"
    exit 1
fi
sha="$(sha256sum "$f" | cut -d' ' -f1)"
official="$ROOT/track-$t/verification/expected/$want.sha256"
local_pin="$ROOT/track-$t/verification/expected/$want.local.sha256"
mkdir -p "$(dirname "$official")"

# The official pin is the scoring target: never clobber it silently.
if [ -f "$official" ] && [ "$force" != "--force" ]; then
    have="$(tr -d '[:space:]' < "$official")"
    if [ "$have" = "$sha" ]; then
        rm -rf "$out"
        echo "already SHA-green against the OFFICIAL expected hash ($sha)"
        exit 0
    fi
    printf '%s\n' "$sha" > "$local_pin"
    rm -rf "$out"
    echo "OFFICIAL pin kept: $official"
    echo "  official expects $have"
    echo "  this build gives $sha"
    echo "  wrote the local pin instead: $local_pin"
    echo "  (that mismatch is expected until the real implementation lands;"
    echo "   --force overwrites the official pin only if the organizers republished)"
    exit 0
fi

printf '%s\n' "$sha" > "$official"
rm -rf "$out"
echo "pinned $official <- $sha"
echo "next: git add $official && commit (the SHA gate now expects this hash)"