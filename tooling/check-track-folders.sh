#!/bin/sh
# Audit every track folder against the board's expectations.
#
# The board uses each track-* folder as the build context AND as the S axis
# ("everything the build can see"). Three failure classes this gate catches:
#
#  1. REQUIRED files missing  -> the board/build fails, or the folder becomes
#     indistinguishable from the reference.
#  2. TRACKED stray files     -> they enter the image and inflate S.
#  3. UNTRACKED forbidden files that still reach `COPY . /src`. This is the
#     subtle one: `git` ignoring a file does NOT keep it out of `docker build`,
#     because the FOLDER is the context, not the repository. A local
#     `sh build.sh` leaves `run` behind and rustc on Windows also writes
#     `run.pdb`, so 1.5 MB of foreign binaries sat in every build context while
#     every tracked-file check stayed green. If an in-image compile ever failed
#     to replace `run`, `COPY --from=build /src/run /app/run` would ship that
#     stale host binary. Each folder therefore carries a `.dockerignore`, and
#     this gate fails if a forbidden file is on disk without a matching line.
#
# Required per track: Dockerfile, .dockerignore, .speed-demon.toml, PERF.md,
# README.md, build.sh, src/ (main.rs), verification/expected/*.sha256.
# Always allowed: .gitkeep under verification/expected.
#
# Usage:  sh tooling/check-track-folders.sh
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
fail=0

# Does the folder's .dockerignore exclude this basename, exactly or as *.ext?
dockerignored() {
    di="$1/.dockerignore"
    [ -f "$di" ] || return 1
    grep -qxF "$2" "$di" && return 0
    ext="${2##*.}"
    [ "$ext" != "$2" ] && grep -qxF "*.$ext" "$di" && return 0
    return 1
}

for t in a b c d; do
    dir="$ROOT/track-$t"
    [ -d "$dir" ] || { echo "  FAIL track-$t: folder missing"; fail=$((fail + 1)); continue; }

    missing=""
    for f in Dockerfile .dockerignore .speed-demon.toml PERF.md README.md build.sh src/main.rs; do
        [ -f "$dir/$f" ] || missing="$missing $f"
    done
    if [ -n "$missing" ]; then
        echo "  FAIL track-$t: missing required file(s):$missing"
        fail=$((fail + 1))
    fi

    # ── every TRACKED file (what the board's clone sees and what S counts) ──
    for p in $(git ls-files "track-$t" | sed "s|^track-$t/||" | sort); do
        case "$p" in
            Dockerfile|.dockerignore|.speed-demon.toml|PERF.md|README.md|build.sh) continue ;;
            src/main.rs|src/io.rs|src/*.rs) continue ;;
            verification/expected/*.sha256|verification/expected/.gitkeep) continue ;;
            *) # a stray tracked file: is it one of the forbidden categories?
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

    # ── UNTRACKED files that would still enter `COPY . /src` ────────────────
    # Enumerated from disk, one test per file, so nothing is double-counted.
    for f in "$dir"/* "$dir"/src/*; do
        [ -f "$f" ] || continue
        base="$(basename "$f")"
        case "$base" in
            run|run.exe|run.pdb|*.pdb|*.exe|*.o|*.rlib|*.log|*.tmp) ;;
            *) continue ;;
        esac
        if dockerignored "$dir" "$base"; then
            echo "  ok   track-$t: $base is dockerignored, so it cannot enter the context"
        else
            echo "  FAIL track-$t: $base is on disk and NOT dockerignored — it would enter the scored image"
            fail=$((fail + 1))
        fi
    done
done

echo ""
if [ "$fail" -eq 0 ]; then
    echo "track folders: required files present, no stray/forbidden files, build context clean"
    exit 0
fi
echo "$fail check(s) failed — fix the track folders before pushing"
exit 1
