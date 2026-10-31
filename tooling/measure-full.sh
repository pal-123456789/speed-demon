#!/bin/sh
# Measure a track binary against a PUBLISHED full-scale hash, five times.
#
# Deliberately does not use `sd-harness verify`: its expectation comes from the
# committed pin, which is sample-scale, so a full-scale run would compare
# full-scale output against a sample-scale hash and print FAIL for a correct
# answer. See verification/EXPECTED-HASHES.md.
#
#   usage: sh tooling/measure-full.sh <track> <data-dir> <expected-sha256> <output-name>
#
# WHY THE SCRATCH DIRECTORY IS PER-INVOCATION. This script used to hardcode
# /tmp/sd-measure, and so did every concurrent workstream on this box. They then
# deleted each other's output mid-run, and the script reported WRONG with an
# EMPTY hash for binaries that are green. That false verdict cost real work: it
# is what made me revert two tracks whose changes were correct. A shared scratch
# path turns a measurement tool into a random number generator, and the failure
# is silent because "WRONG" looks exactly like a real correctness failure.
#
# Two further hazards, learned the same way:
#   * Never measure the live `track-<t>/run` path while a build may be running --
#     build.sh truncates it mid-run. Copy it somewhere private first.
#   * Wall time on this box is unusable under load (it has ranged 1.6 s to 27 s
#     for runs whose CPU time is ~10 s). The hash is exact and load-independent;
#     timing is not. For a load-robust comparison use child CPU time, which the
#     workstreams measure with GetProcessTimes.
set -u

track="$1"
data="$2"
expect="$3"
outname="$4"
bin="./track-$track/run"

[ -x "$bin" ] || { echo "no binary at $bin"; exit 2; }

scratch=$(mktemp -d "${TMPDIR:-/tmp}/sd-measure.$$.XXXXXX") || exit 2
trap 'rm -rf "$scratch"' EXIT INT TERM

i=1
while [ "$i" -le 5 ]; do
    rm -f "$scratch/$outname"
    start=$(date +%s.%N)
    SD_DATA_DIR="$data" SD_OUT_DIR="$scratch" "$bin"
    rc=$?
    end=$(date +%s.%N)
    got=$(sha256sum "$scratch/$outname" 2>/dev/null | cut -d' ' -f1)
    if [ "$got" = "$expect" ]; then verdict="MATCH"; else verdict="WRONG"; fi
    secs=$(awk -v a="$start" -v b="$end" 'BEGIN{printf "%.3f", b-a}')
    printf '  run %d: %8ss  exit=%s  %s  %s\n' "$i" "$secs" "$rc" "$verdict" \
        "$(echo "$got" | cut -c1-16)"
    i=$((i + 1))
done
