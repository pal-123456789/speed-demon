#!/bin/sh
# check_pgo_wiring.sh — fail if a scored build could silently run unprofiled.
#
# WHY THIS EXISTS
#
# `docs/CAMPAIGN-STATUS.md` records this as a defect that already bit us once,
# on the abandoned build path, and it is the nastiest kind: it produces no
# error, no non-zero exit and no gap in the output.
#
#   rustc -C profile-use=<a DIRECTORY>   prints
#       warning: <dir>: is a directory
#   and still EXITS 0, compiling with no profile at all, while the build log
#   says "PGO profile found".
#
# So a typo in the profile path (a directory instead of a file, or a file that
# was never merged) costs the whole PGO win and reports success. Worse, a
# profile that is not inside the TRACK FOLDER never reaches the scored image,
# because the track folder is the whole build context:
#
#     docker build --network=none ... track-$TRACK      # context == track-$TRACK
#
# A `.profdata` this script cannot see from inside the track folder is a
# profile rustc will never read, however good the build.log looks.
#
# WHAT IT CHECKS (on the CURRENT build path: track-*/build.sh)
#
#   1. `-C profile-use=` is never given a directory (must end in .profdata).
#   2. If any track compiles with -C profile-use, that profile must exist as a
#      file INSIDE that track's folder, so it is actually in the build context.
#   3. The profile-use flag is paired with a merged profile, not a raw .profraw
#      (rustc cannot read a raw profile).
#   4. A track that ships a `.profdata` states so in its PERF.md, so the
#      optimisation log can cite the run that measured it.
#
# Stays quiet and exits 0 when no PGO is wired, which is the pre-kickoff state:
# the profiles are generated in-window from real code, never committed blind.
#
# Usage:  sh tooling/check_pgo_wiring.sh      (exit 1 on any finding)

set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

FAILED=0
NOTE() { printf '%s\n' "$*"; }
FAIL() { FAILED=1; printf 'FAIL  %s\n' "$*"; }

TRACKS=""
for d in track-*; do
    [ -d "$d" ] || continue
    TRACKS="$TRACKS $d"
done

if [ -z "$TRACKS" ]; then
    FAIL "no track-* folders found (run from the repository root)"
fi

for t in $TRACKS; do
    BUILD="$t/build.sh"
    [ -f "$BUILD" ] || { FAIL "$t has no build.sh"; continue; }

    # Every -C profile-use=<arg> in this track's build script.
    # shellcheck disable=SC2013
    for arg in $(sed -n 's/.*profile-use=\([^ "]*\).*/\1/p' "$BUILD"); do
        case "$arg" in
            *.profdata) : ;;
            *)
                FAIL "$t: profile-use=$arg is not a .profdata file; rustc warns and exits 0 with NO profile applied"
                continue
                ;;
        esac

        # It must be reachable from inside the track folder, because that
        # folder is the entire build context.
        case "$arg" in
            /*) FAIL "$t: profile-use=$arg is an absolute path; a scored build gets only $t/ as its context" ;;
            *)
                if [ ! -f "$t/$arg" ]; then
                    FAIL "$t: profile-use=$arg does not exist inside $t/, so it cannot reach the scored image"
                fi
                ;;
        esac
    done

    # A raw profile is not readable by rustc; it must have been merged.
    for arg in $(sed -n 's/.*profile-use=\([^ "]*\).*/\1/p' "$BUILD"); do
        case "$arg" in
            *.profraw) FAIL "$t: profile-use=$arg is a raw profile; merge it with llvm-profdata first" ;;
        esac
    done

    # Every .profdata shipped in the folder must be explained by PERF.md.
    for prof in "$t"/*.profdata "$t"/pgo/*.profdata; do
        [ -f "$prof" ] || continue
        if ! grep -qi 'profdata\|PGO' "$t/PERF.md" 2>/dev/null; then
            FAIL "$t ships $(basename "$prof") but PERF.md never mentions the PGO step"
        fi
    done
done

if [ "$FAILED" -ne 0 ]; then
    printf '\nPGO wiring is unsound: the build would report success and run unprofiled.\n' >&2
    exit 1
fi

NOTE "check_pgo_wiring: no unprofiled-PGO hazard on the current build path"
