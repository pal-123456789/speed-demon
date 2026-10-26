#!/bin/sh
# Replay the Dockerfile's exact build-context contract WITHOUT docker.
#
# The scored build is:  docker build --network=none -t ... track-$TRACK
# with the track folder as context, i.e. `COPY . /src` then `RUN ./build.sh`.
# This script copies the track folder into a CLEAN temp dir (a faithful proxy
# for the context the builder sends), runs build.sh inside it, and verifies
# the produced binary executes and writes exactly one output file.
#
# Why this gate exists: a build that "works here" because it reads a file that
# lives OUTSIDE the track folder (or is gitignored/untracked) fails on the
# scoring box, scoring zero for that track. Replaying from a clean copy makes
# that contract a gate instead of a surprise at hour 68.
#
# Usage:  sh tooling/context-replay.sh [TRACK]    (default: A B C D)
# Needs:  sh, cp, rm, a seed data dir (verification/data/track-X or a stub)

set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TRACKS="${1:-a b c d}"
TRACKS="$(echo "$TRACKS" | tr '[:upper:]' '[:lower:]')"
pass=0
fail=0

for t in $TRACKS; do
    src="$ROOT/track-$t"
    if [ ! -f "$src/build.sh" ] || [ ! -d "$src/src" ]; then
        echo "  FAIL track-$t: folder is missing build.sh/src"
        fail=$((fail + 1))
        continue
    fi
    tmp="$(mktemp -d 2>/dev/null || echo "/tmp/sd-replay-$t-$$")"
    mkdir -p "$tmp/src"
    # COPY . /src  — the file list the Docker context would carry. Copying
    # with cp -R also drops gitignored artifacts, same as a fresh clone+checkout.
    if ! cp -R "$src/." "$tmp/src/" 2>/dev/null; then
        echo "  FAIL track-$t: could not stage the context"
        fail=$((fail + 1))
        continue
    fi
    if ! (cd "$tmp/src" && sh build.sh >"$tmp/build.log" 2>&1); then
        echo "  FAIL track-$t: build.sh failed inside a clean context (see $tmp/build.log)"
        fail=$((fail + 1))
        continue
    fi
    bin="$tmp/src/run"
    if [ ! -f "$bin" ]; then
        echo "  FAIL track-$t: build.sh produced no ./run in the clean context"
        fail=$((fail + 1))
        continue
    fi
    # Exec it against the real sample if present, else a trivial stub, and
    # require exactly one output file — the sandbox gate's shape.
    out="$tmp/out"
    mkdir -p "$out"
    data="$ROOT/verification/data/track-$t"
    if [ ! -d "$data" ]; then
        data="$tmp/data"
        mkdir -p "$data"
        case "$t" in
            A) printf '2026-10-30T18:07:18.569411Z GET /api/v2/products 200 8177\n' >"$data/requests.log" ;;
            B) printf 'in the matter of trixclar\n' >"$data/docs.txt" ;;
            C) printf 'id,name,note,region_id,amount\r\n1,"Kaphar, Titus","said ""no"" twice",7,1420.00\r\n' >"$data/records.csv" ;;
            D) printf '4 3\n0 1 10\n1 2 20\n2 3 30\n' >"$data/graph.txt" ;;
        esac
    fi
    SD_DATA_DIR="$data" SD_OUT_DIR="$out" "$bin" >"$tmp/stdout.log" 2>"$tmp/stderr.log"
    rc=$?
    nfiles="$(find "$out" -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' ')"
    if [ "$rc" -eq 0 ] && [ "$nfiles" = "1" ]; then
        echo "  ok   track-$t: clean-context build + run (1 output file)"
        pass=$((pass + 1))
    else
        echo "  FAIL track-$t: rc=$rc files=$nfiles in the clean context"
        fail=$((fail + 1))
    fi
    rm -rf "$tmp"
done

echo ""
echo "  context replay: $pass passed, $fail failed"
[ "$fail" -eq 0 ] || exit 1
echo "CONTEXT REPLAY: ALL CHECKED OUT"
exit 0