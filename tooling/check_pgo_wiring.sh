#!/bin/sh
# check_pgo_wiring.sh — prove PGO can actually reach the scored build.
#
# The bug this guards against is invisible in every log and every exit code:
#
#   build-one.sh passed `-C profile-use=<directory>`. rustc answers
#     warning: <dir>: is a directory
#   and EXITS 0, compiling with no profile at all. The build printed
#   "[build-one] PGO profile found" and shipped an unprofiled binary.
#
#   Even with the file passed correctly, the profile lived in target/pgo/prof,
#   and .dockerignore excludes both `target/` and `*.profdata`, so the profile
#   was never in the build context the organizer builds from.
#
# Both halves are checked here, statically, so neither can regress silently.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
fail=0
ok()   { printf '  ok    %s\n' "$1"; }
bad()  { printf '  FAIL  %s\n' "$1"; fail=$((fail + 1)); }

echo "check_pgo_wiring: does the profile reach the scored build?"

BUILD_ONE="$ROOT/build-one.sh"
DOCKERIGNORE="$ROOT/.dockerignore"
PGO_BUILD="$ROOT/tooling/pgo-build.sh"

# ── 1. `-C profile-use` must name a file, never a directory ───────────────
# Every occurrence in the build path, not just the first: a second bad call
# site is exactly how this would come back.
bad_use=0
for f in "$BUILD_ONE" "$PGO_BUILD"; do
    [ -f "$f" ] || continue
    hits=$(grep -v '^[[:space:]]*#' "$f" \
        | grep -o 'profile-use=[^ "'"'"']*' || true)
    for h in $hits; do
        val=${h#profile-use=}
        case "$val" in
            *merged.profdata)
                : ;;
            \$*)
                # A variable is acceptable only if the same file proves it
                # holds a regular file before use: that is the invariant the
                # directory bug violated. Fixed-string matching, because a
                # regex over shell quoting is its own source of bugs.
                var=${val#\$}
                var=$(printf '%s' "$var" | tr -d '{}')
                if grep -qF -- "-f \"\$$var\"" "$f"; then
                    ok "$(basename "$f"): \$$var is verified to be a file (-f) before use"
                else
                    bad "$(basename "$f"): \$$var is not verified with -f; a directory or a stale file would silently disable PGO"
                    bad_use=1
                fi
                ;;
            *)
                bad "$(basename "$f"): $h does not end in merged.profdata"
                bad_use=1
                ;;
        esac
    done
done
[ "$bad_use" -eq 0 ] && ok "-C profile-use resolves to a .profdata file"

# ── 2. a profile rustc REFUSES must fail the build ────────────────────────
# rustc downgrades an unreadable/corrupt profile to a warning, so the exit
# code is not enough evidence that PGO was applied.
if grep -q 'grep -qi "profile"' "$BUILD_ONE"; then
    ok "a rejected profile is fatal (rustc warns and exits 0 otherwise)"
else
    bad "build-one.sh does not check rustc's stderr for a refused profile"
fi

# ── 3. the profile must be inside the build context ───────────────────────
# Later .dockerignore patterns win, so an explicit !pgo/ re-inclusion is what
# makes the profile shippable.
if grep -Eq '^!pgo/\*?\.?p?r?o?f?d?a?t?a?$|^!pgo/$|^!pgo/\*\.profdata$' "$DOCKERIGNORE"; then
    ok ".dockerignore re-includes the shipped profile"
else
    bad ".dockerignore excludes *.profdata and never re-includes pgo/"
fi

# ── 4. pgo-build.sh must publish to that shippable path ───────────────────
if grep -q 'APP_DIR/pgo/merged.profdata' "$PGO_BUILD"; then
    ok "pgo-build.sh publishes pgo/merged.profdata"
else
    bad "pgo-build.sh only writes target/pgo/prof, which cannot enter the image"
fi

# ── 5. build-one.sh must prefer the shippable copy ────────────────────────
if grep -q 'APP_DIR/pgo/merged.profdata' "$BUILD_ONE"; then
    ok "build-one.sh looks for the shippable copy first"
else
    bad "build-one.sh does not look in pgo/"
fi

echo
if [ "$fail" -ne 0 ]; then
    printf 'check_pgo_wiring: %d FAILED\n' "$fail"
    exit 1
fi
printf 'check_pgo_wiring: all checks passed\n'