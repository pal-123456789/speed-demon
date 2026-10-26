#!/bin/sh
# Fetch the organizers' PUBLIC generators and produce the official-shaped
# data for every track.
#
# The event docs (KICKSTART §04) prescribe exactly this flow:
#     git clone https://github.com/hackathon-raptors/speed-demon-2026
#     python3.13 speed-demon-2026/generators/a_ingest.py --seed 1 --scale sample --out data/a
#
# Seed 1 at `full` scale is the board's dataset; `sample` is 10% of it. The
# repo and the sample hashes are published on Oct 30 06:00 UTC — until then
# the clone fails (private), and this script says so instead of pretending.
#
# The generators are the organizers' own tooling and byte-identical copies are
# explicitly free, so keeping a checkout locally costs nothing on any axis
# (nothing here is inside a track folder, so it never enters S).
#
# Usage:
#   sh tooling/gen-official-data.sh              # seed 1, sample (10%)
#   SCALE=full sh tooling/gen-official-data.sh   # the full board dataset
#   SEED=7 sh tooling/gen-official-data.sh       # a different seed (fuzzing)
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SEED="${SEED:-1}"
SCALE="${SCALE:-sample}"
DEST="${DEST:-$ROOT/verification/data}"
GEN_DIR="${GEN_DIR:-$ROOT/.official-generators}"
REPO="https://github.com/hackathon-raptors/speed-demon-2026"

PY="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
[ -n "$PY" ] || { echo "no python3/python on PATH" >&2; exit 2; }

if [ ! -d "$GEN_DIR/generators" ]; then
    echo "cloning the organizers' generators into $GEN_DIR ..."
    if ! git clone --depth 1 "$REPO" "$GEN_DIR" 2>/tmp/sd-genclone.log; then
        echo "CLONE FAILED — the repo is private until Oct 30 06:00 UTC." >&2
        echo "Re-run after the samples publish; see /tmp/sd-genclone.log" >&2
        exit 1
    fi
fi

mkdir -p "$DEST"
rc=0
for t in a b c d; do
    case "$t" in
        a) gen=a_ingest.py ;;
        b) gen=b_index.py ;;
        c) gen=c_join.py ;;
        d) gen=d_traverse.py ;;
    esac
    src="$GEN_DIR/generators/$gen"
    if [ ! -f "$src" ]; then
        echo "  SKIP track-$t: $src not found (check the repo layout)" >&2
        rc=1
        continue
    fi
    mkdir -p "$DEST/track-$t"
    if "$PY" "$src" --seed "$SEED" --scale "$SCALE" --out "$DEST/track-$t"; then
        echo "  ok   track-$t  (seed $SEED, scale $SCALE)"
    else
        echo "  FAIL track-$t" >&2
        rc=1
    fi
done

echo ""
echo "Official-shaped data written under $DEST (seed $SEED, scale $SCALE)."
echo "Next: sh track-*/build.sh, then python check.py <track> --data verification/data/track-<t>"
exit "$rc"