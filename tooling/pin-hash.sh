#!/bin/sh
# Re-pin a track's expected-hash file from the CURRENT binary's deterministic
# output on the sample data.
#
# After H0 the SHA gate pins track-X/verification/expected/<output>.sha256 and
# every ladder rung that stays SHA-green must re-pin it. This is that one
# command — run the binary once, hash the single output file, rewrite the pin.
#
# Usage:  sh tooling/pin-hash.sh A|B|C|D
# Needs : the binary built (sh track-X/build.sh) and sample data present
#         (verification/data/track-X — run `python tooling/gen-fixtures.py --all`)
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
t="${1:-}"
case "$t" in
    A) want=ingest.txt  ;;
    B) want=index.txt   ;;
    C) want=join.txt    ;;
    D) want=traverse.txt ;;
    *) echo "usage: pin-hash.sh A|B|C|D" >&2; exit 2 ;;
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
pin="$ROOT/track-$t/verification/expected/$want.sha256"
mkdir -p "$(dirname "$pin")"
printf '%s\n' "$sha" > "$pin"
rm -rf "$out"
echo "pinned $pin <- $sha"
echo "next: git add $pin && commit (the SHA gate now expects this hash)"