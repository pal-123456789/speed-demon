#!/bin/sh
# Produce the official data for every track — OFFLINE, hash-verified.
#
# This used to clone the organizers' repo from GitHub. It no longer does: the
# generators are vendored byte-identical under reference/generators/ (rule 16
# makes published copies free), and tooling/official-data.py generates the data
# and checks every published SHA-256. So this script is now a thin, compatible
# wrapper — same SEED/SCALE/DEST interface, no network, nothing to clone.
#
# Usage:
#   sh tooling/gen-official-data.sh              # seed 1, sample (the board's live scale)
#   SCALE=tiny sh tooling/gen-official-data.sh   # the edge-case scale
#   SCALE=full sh tooling/gen-official-data.sh   # the full dataset (~600 MB)
#
# For the tiny conformance corpus with the reference's answers stored beside it:
#   python tooling/official-data.py --track all --scale tiny \
#       --dest verification/cases --save-expected --no-pin
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SEED="${SEED:-1}"
SCALE="${SCALE:-sample}"
DEST="${DEST:-$ROOT/verification/data}"

PY="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
if [ -z "$PY" ]; then
    echo "no python3/python on PATH" >&2
    exit 2
fi

if [ "$SEED" != "1" ]; then
    echo "note: only seed 1 has published hashes; generating seed $SEED with the" >&2
    echo "      vendored generator directly (no hash verification possible)." >&2
    for t in a b c d; do
        case "$t" in
            a) g=a_ingest.py ;; b) g=b_index.py ;; c) g=c_join.py ;; d) g=d_traverse.py ;;
        esac
        "$PY" "$ROOT/reference/generators/$g" --seed "$SEED" --scale "$SCALE" \
              --out "$DEST/track-$t" || exit 1
    done
    exit 0
fi

echo "gen-official-data: delegating to tooling/official-data.py (vendored generators, offline, hash-verified)"
exec "$PY" "$ROOT/tooling/official-data.py" --track all --scale "$SCALE" --dest "$DEST"
