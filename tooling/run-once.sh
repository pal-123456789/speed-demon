#!/bin/sh
# Time and verify ONE run of a track, into a private scratch directory.
#   usage: sh tooling/run-once.sh <track> <data-dir> <expected-sha256> <output-name>
set -u
track="$1"; data="$2"; expect="$3"; outname="$4"
bin="./track-$track/run"
[ -x "$bin" ] || { echo "no binary at $bin"; exit 2; }

scratch=$(mktemp -d "${TMPDIR:-/tmp}/sd-once.$$.XXXXXX") || exit 2
trap 'rm -rf "$scratch"' EXIT INT TERM

start=$(date +%s.%N)
SD_DATA_DIR="$data" SD_OUT_DIR="$scratch" "$bin"
rc=$?
end=$(date +%s.%N)

secs=$(awk -v a="$start" -v b="$end" 'BEGIN{printf "%.3f", b-a}')
if [ -f "$scratch/$outname" ]; then
    got=$(sha256sum "$scratch/$outname" | cut -d' ' -f1)
    if [ "$got" = "$expect" ]; then v=MATCH; else v=WRONG; fi
    printf '  exit=%s  wall=%ss  %s  %s\n' "$rc" "$secs" "$v" "$(echo "$got" | cut -c1-20)"
else
    printf '  exit=%s  wall=%ss  NO-OUTPUT\n' "$rc" "$secs"
fi
