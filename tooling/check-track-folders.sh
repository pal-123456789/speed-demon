#!/bin/sh
# Audit every track folder against the board's expectations.
#
# The board uses each track-* folder as the build context AND as the S axis
# ("everything the build can see"). Two failure classes this gate catches:
#
#  1. REQUIRED files missing  -> the board/build fails or the folder is
#     indistinguishable from the reference.
#  2. STRAY files present     -> they enter the image and inflate S. Worst
#     cases are data files (could be mistaken for cached outputs -> the image
#     "may not ship cached outputs"), built binaries, or large junk.
#
# Required per track: Dockerfile, .speed-demon.toml, PERF.md, README.md,
# build.sh, src/ (main.rs), verification/expected/*.sha256.
# Always allowed: .gitkeep under verification/expected.
#
# Usage:  sh tooling/check-track-folders.sh
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
fail=0

# exact filenames that must never be in a track folder (data/output/build junk)
FORBIDDEN="run run.exe run.pdb requests.log docs.txt queries.txt records.csv regions.csv graph.txt summary.txt results.txt aggregates.txt distances.txt *.log *.tmp"

for t in a b c d; do
    dir="$ROOT/track-$t"
    [ -d "$dir" ] || { echo "  FAIL track-$t: folder missing"; fail=$((fail + 1)); continue; }

    missing=""
    for f in Dockerfile .speed-demon.toml PERF.md README.md build.sh src/main.rs; do
        [ -f "$dir/$f" ] || missing="$missing $f"
    done
    if [ -n "$missing" ]; then
        echo "  FAIL track-$t: missing required file(s):$missing"
        fail=$((fail + 1))
    fi

    # every TRACKED file under the folder (what the board's clone sees and
    # what S counts) minus the always-fine bits
    for p in $(git ls-files "track-$t" | sed "s|^track-$t/||" | sort); do
        case "$p" in
            Dockerfile|.speed-demon.toml|PERF.md|README.md|build.sh) continue ;;
            src/main.rs|src/io.rs|src/*.rs) continue ;;
            verification/expected/*.sha256|verification/expected/.gitkeep) continue ;;
            *) # a stray file: is it one of the forbidden categories?
               base="$(basename "$p")"
               case "$base" in
                   run|run.exe|run.pdb|requests.log|docs.txt|queries.txt|records.csv|regions.csv|graph.txt|summary.txt|results.txt|aggregates.txt|distances.txt)
                       echo "  FAIL track-$t: forbidden file in the build context/S axis: $p"
                       fail=$((fail + 1)) ;;
                   *.log|*.tmp|*.csv|*.txt|*.bin|*.data|*.out|*.pdb|*.exe)
                       echo "  FAIL track-$t: stray data/output file would enter image + S: $p"
                       fail=$((fail + 1)) ;;
                   *)
                       echo "  WARN track-$t: unexpected file (check it is wanted): $p" ;;
               esac ;;
        esac
    done
done

echo ""
if [ "$fail" -eq 0 ]; then
    echo "track folders: required files present, no stray/forbidden files"
    exit 0
fi
echo "$fail check(s) failed — fix the track folders before pushing"
exit 1