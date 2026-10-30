#!/bin/sh
# Check a track binary against both committed corpora: the tiny hostile oracle
# corpus and the official 25 MB sample.
#
#   usage: sh tooling/check-corpora.sh <track> <output-name> <sample-pin>
set -u
track="$1"; outname="$2"; pin="$3"
root=/d/SpeedDemon-Campaign
bin="$root/track-$track/run"

check() {
    label="$1"; datadir="$2"; expect="$3"
    rm -rf /tmp/sd-cc; mkdir -p /tmp/sd-cc
    SD_DATA_DIR="$datadir" SD_OUT_DIR=/tmp/sd-cc "$bin" >/dev/null 2>&1
    got=$(sha256sum "/tmp/sd-cc/$outname" 2>/dev/null | cut -d' ' -f1)
    if [ "$got" = "$expect" ]; then r=MATCH; else r=WRONG; fi
    printf '  %-22s %s  %s\n' "$label" "$r" "$(echo "$got" | cut -c1-20)"
}

echo "track $track"
check "tiny oracle corpus" "$root/verification/cases/track-$track" \
      "$(sha256sum "$root/verification/cases/track-$track/expected-$outname" | cut -d' ' -f1)"
check "official sample" "$root/verification/data/track-$track" "$pin"