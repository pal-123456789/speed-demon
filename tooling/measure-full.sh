#!/bin/sh
# Measure a track binary against a PUBLISHED full-scale hash, five times.
#
# Deliberately does not use `sd-harness verify`: its expectation comes from the
# committed pin, which is sample-scale, so a full-scale run would compare
# full-scale output against a sample-scale hash and print FAIL for a correct
# answer. See verification/EXPECTED-HASHES.md.
#
#   usage: sh tooling/measure-full.sh <track> <data-dir> <expected-sha256> <output-name>
set -u

track="$1"
data="$2"
expect="$3"
outname="$4"
bin="./track-$track/run"

[ -x "$bin" ] || { echo "no binary at $bin"; exit 2; }

i=1
while [ "$i" -le 5 ]; do
    rm -rf /tmp/sd-measure
    mkdir -p /tmp/sd-measure
    start=$(date +%s.%N)
    SD_DATA_DIR="$data" SD_OUT_DIR=/tmp/sd-measure "$bin"
    rc=$?
    end=$(date +%s.%N)
    got=$(sha256sum "/tmp/sd-measure/$outname" 2>/dev/null | cut -d' ' -f1)
    if [ "$got" = "$expect" ]; then verdict="MATCH"; else verdict="WRONG"; fi
    secs=$(awk -v a="$start" -v b="$end" 'BEGIN{printf "%.3f", b-a}')
    printf '  run %d: %8ss  exit=%s  %s  %s\n' "$i" "$secs" "$rc" "$verdict" \
        "$(echo "$got" | cut -c1-16)"
    i=$((i + 1))
done
