#!/bin/sh
# Check a track binary against both committed corpora: the tiny hostile oracle
# corpus and the official 25 MB sample.
#
#   usage: sh tooling/check-corpora.sh <track> <output-name> <sample-pin>
#
# WHY THIS IS DEFENSIVE. An earlier version ran the binary with stderr sent to
# /dev/null and then compared a possibly-empty hash, printing "WRONG" when no file
# had been written at all. That conflates two completely different things and sends
# you hunting a correctness bug that does not exist -- which is exactly what it did:
# it reported the official sample as WRONG with a blank hash for a binary that
# produces the correct hash when run by hand.
#
# A verification tool that misreports its own verdict is worse than no tool. This
# session has already had one of those (the A/B script announcing the loser as the
# winner), so here every failure mode gets its own outcome: missing input, non-zero
# exit, absent output, no expectation supplied, and genuinely different bytes are
# five different results, and only the last one is a correctness failure.
set -u
track="$1"; outname="$2"; pin="$3"
root=/d/SpeedDemon-Campaign
bin="$root/track-$track/run"

if [ ! -x "$bin" ]; then
    echo "  ERROR: no executable at $bin  (run: sh track-$track/build.sh)"
    exit 2
fi

fail=0
check() {
    label="$1"; datadir="$2"; expect="$3"

    if [ ! -d "$datadir" ]; then
        printf '  %-22s SKIP       no such data directory\n' "$label"
        fail=1
        return
    fi

    rm -rf /tmp/sd-cc
    mkdir -p /tmp/sd-cc

    SD_DATA_DIR="$datadir" SD_OUT_DIR=/tmp/sd-cc "$bin" >/tmp/sd-cc.err 2>&1
    rc=$?

    out="/tmp/sd-cc/$outname"
    if [ ! -f "$out" ]; then
        printf '  %-22s NO-OUTPUT  exit %s, no %s written\n' "$label" "$rc" "$outname"
        sed -n '1,3p' /tmp/sd-cc.err | sed 's/^/        /'
        fail=1
        return
    fi
    if [ "$rc" -ne 0 ]; then
        printf '  %-22s BAD-EXIT   wrote %s but exited %s\n' "$label" "$outname" "$rc"
        sed -n '1,3p' /tmp/sd-cc.err | sed 's/^/        /'
        fail=1
        return
    fi

    got=$(sha256sum "$out" | cut -d' ' -f1)
    if [ -z "$expect" ]; then
        printf '  %-22s NO-PIN     produced %s\n' "$label" "$(echo "$got" | cut -c1-20)"
        return
    fi
    if [ "$got" = "$expect" ]; then
        printf '  %-22s MATCH      %s\n' "$label" "$(echo "$got" | cut -c1-20)"
    else
        printf '  %-22s WRONG      %s\n' "$label" "$(echo "$got" | cut -c1-20)"
        printf '        expected              %s\n' "$(echo "$expect" | cut -c1-20)"
        fail=1
    fi
}

echo "track $track  (binary $bin)"
check "tiny oracle corpus" "$root/verification/cases/track-$track" \
      "$(sha256sum "$root/verification/cases/track-$track/expected-$outname" 2>/dev/null | cut -d' ' -f1)"
check "official sample" "$root/verification/data/track-$track" "$pin"
rm -f /tmp/sd-cc.err
exit $fail
