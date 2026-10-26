#!/bin/sh
# Build ONE track into ./bin — core compiled once, then a single binary.
#
# This is the single implementation of the offline build. `build-all.sh`
# loops over it for the scored image; the per-track Dockerfiles call it
# directly for the standalone single-track images. One implementation means
# the two paths cannot drift — and they HAVE drifted before, which is how a
# per-track Dockerfile ended up referring to files that were never copied
# into its image.
#
# THE LINKER IS THE WHOLE PROBLEM
#
#   The builder stage is debian:13-slim, which ships NO C compiler. The
#   scoring build runs with --network=none, so `apt-get install gcc` is not
#   an option either. A stock `x86_64-unknown-linux-gnu` link needs `cc`,
#   so it would fail on the scoring box and score ZERO on all four tracks
#   (the image is shared).
#
#   The way out is the musl target with a fully static link:
#       --target x86_64-unknown-linux-musl
#       -C target-feature=+crt-static
#       -C linker=<sysroot>/lib/rustlib/<host>/bin/rust-lld
#       -C linker-flavor=ld.lld
#   rust-lld ships inside the Rust toolchain, so nothing has to be
#   downloaded at build time, and the resulting binary needs no libc at
#   all — it runs on debian:13-slim regardless of its glibc version.
#
#   Note the flavor: `ld.lld` is STABLE. `gnu-lld` is not — using it makes
#   rustc demand `-Z unstable-options`, which is a hard error on the
#   pinned stable toolchain, and the failure looks like a mysterious
#   "unstable options" message rather than a missing linker.
#
# Usage:
#   TRACK=A sh build-one.sh          # build track A
#   sh build-one.sh B                # build track B
#
# Environment:
#   TRACK            which track to build (A..D). Overridden by $1.
#   SD_REUSE_CORE=1  reuse an existing bin/libspeeddemon_core.rlib instead
#                    of recompiling it (used by build-all.sh).
set -eu

APP_DIR="$(cd "$(dirname "$0")" && pwd)"
OUT_DIR="$APP_DIR/bin"
mkdir -p "$OUT_DIR"

TRACK="${1:-${TRACK:-A}}"
TRACK="$(printf '%s' "$TRACK" | tr '[:lower:]' '[:upper:]' | tr -cd 'A-Z')"
[ -n "$TRACK" ] || TRACK="A"

case "$TRACK" in
    A) TRACK_DIR="A-ingest";   BIN="speeddemon-a" ;;
    B) TRACK_DIR="B-index";    BIN="speeddemon-b" ;;
    C) TRACK_DIR="C-join";     BIN="speeddemon-c" ;;
    D) TRACK_DIR="D-traverse"; BIN="speeddemon-d" ;;
    *)
        echo "[build-one] FATAL: unknown track '$TRACK' (expected A..D)" >&2
        exit 1
        ;;
esac

# ---- locate rustc --------------------------------------------------------
# The scoring build runs with --network=none, so the toolchain must already
# be in the build context. `tooling/vendor-toolchain.sh` puts it there.
if [ -x "$APP_DIR/vendor/toolchain/bin/rustc" ]; then
    RUSTC="$APP_DIR/vendor/toolchain/bin/rustc"
    RUSTUP_HOME="$APP_DIR/vendor/toolchain"
    export RUSTUP_HOME
    PATH="$APP_DIR/vendor/toolchain/bin:$PATH"
    export PATH
    echo "[build-one] using vendored toolchain: $RUSTC"
elif command -v rustc >/dev/null 2>&1; then
    RUSTC="$(command -v rustc)"
    echo "[build-one] using rustc from PATH: $RUSTC"
else
    echo "[build-one] FATAL: no rustc found." >&2
    echo "  Run tooling/vendor-toolchain.sh on a networked machine BEFORE the hack." >&2
    exit 1
fi

# ---- target and linker ----------------------------------------------------
# The musl standard library must actually be INSTALLED, not merely known to
# the compiler. `rustc --print target-libdir` succeeds for an uninstalled
# target, so it cannot be used as the test; the presence of the standard
# library archive is the real test.
TARGET="x86_64-unknown-linux-musl"
SYSROOT="$("$RUSTC" --print sysroot)"
HOST_TARGET="$("$RUSTC" -vV | sed -n 's/^host: //p')"
MUSL_LIBDIR="$SYSROOT/lib/rustlib/$TARGET/lib"
LLD="$SYSROOT/lib/rustlib/$HOST_TARGET/bin/rust-lld"

if [ ! -d "$MUSL_LIBDIR" ] || ! ls "$MUSL_LIBDIR"/libstd-*.rlib >/dev/null 2>&1; then
    echo "[build-one] FATAL: the musl target is not installed in this toolchain." >&2
    echo "  The builder image (debian:13-slim) has no C compiler and the" >&2
    echo "  build has no network, so the only possible link is a static" >&2
    echo "  musl link through the toolchain's own rust-lld." >&2
    echo "" >&2
    echo "  Fix: rustup target add x86_64-unknown-linux-musl" >&2
    echo "  or re-run: sh tooling/vendor-toolchain.sh" >&2
    exit 1
fi

if [ ! -x "$LLD" ]; then
    echo "[build-one] FATAL: rust-lld not found at $LLD" >&2
    echo "  Without it there is no linker, and the image will not build." >&2
    exit 1
fi

LINK_ARGS="-C target-feature=+crt-static -C linker=$LLD -C linker-flavor=ld.lld"
echo "[build-one] target $TARGET, linker $LLD (static, no libc needed)"

# ---- profile-guided optimisation ----------------------------------------
# PGO is a free T-axis win, but only if it reaches the SCORED build. The
# profile lives in tooling/pgo-build.sh's output directory; if it exists we
# compile with it, and if it does not we compile the same way minus that one
# flag. That check is here, on the scored path, so a PGO profile cannot be
# generated and then silently left behind.
#
# `-C profile-use` takes the MERGED .profdata FILE, not the directory that
# contains it. Passing the directory is not an error: rustc prints
#   warning: /path/to/prof: is a directory
# emits one warning, EXITS 0, and compiles without profile data. So the old
# form silently produced an unprofiled binary on every single build while
# printing "[build-one] PGO profile found" — the worst possible failure shape,
# because the log says PGO was used and nothing downstream can tell it was not.
#
# TWO LOCATIONS, because the profile has to survive into the image:
#   $APP_DIR/pgo/merged.profdata          shippable (NOT excluded by .dockerignore)
#   $APP_DIR/target/pgo/prof/merged.profdata   build artefact (target/ IS excluded)
# The shippable copy wins, because that is the only one present when the
# organizer builds with `docker build --network=none .`.
#
# The profile must never change the ANSWER — tooling/pgo-build.sh verifies
# byte-identical output against a non-PGO build before the profile is kept.
PGO_FLAGS=""
PROFILE_FILE=""
for cand in \
    "${SD_PGO_PROFILE:-}" \
    "$APP_DIR/pgo/merged.profdata" \
    "$APP_DIR/target/pgo/prof/merged.profdata"
do
    [ -n "$cand" ] || continue
    if [ -f "$cand" ] && [ -s "$cand" ]; then
        PROFILE_FILE="$cand"
        break
    fi
done
if [ -n "$PROFILE_FILE" ]; then
    # Belt and braces: re-assert the invariant at the point of use. Anything
    # that is not a readable, non-empty regular file must not be handed to
    # -C profile-use, because rustc accepts a bad profile with a warning.
    if [ ! -f "$PROFILE_FILE" ] || [ ! -s "$PROFILE_FILE" ]; then
        echo "[build-one] FATAL: PGO profile is not a readable non-empty file: $PROFILE_FILE" >&2
        exit 1
    fi
    PGO_FLAGS="-C profile-use=$PROFILE_FILE -C llvm-args=-inline-threshold=275"
    echo "[build-one] PGO profile found: $PROFILE_FILE (optimising for it)"
else
    echo "[build-one] no PGO profile — plain release build"
    if [ -d "$APP_DIR/target/pgo/prof" ]; then
        echo "[build-one]   note: target/pgo/prof exists but holds no merged.profdata;" >&2
        echo "[build-one]   run: sh tooling/pgo-build.sh --track $TRACK" >&2
    fi
fi

# ---- compile core (std-only, D=1) ---------------------------------------
# The core MUST be compiled for the same --target as the track. Without
# --target here, rustc emits an rlib for the HOST triple and the track link
# fails with E0461 ("couldn't find crate with expected target triple") — a
# confusing failure that looks like a missing file rather than a target
# mismatch.
CORE_RLIB="$OUT_DIR/libspeeddemon_core.rlib"
if [ "${SD_REUSE_CORE:-0}" = "1" ] && [ -f "$CORE_RLIB" ]; then
    echo "[build-one] reusing core: $CORE_RLIB"
else
    echo "[build-one] compiling core (std-only, D=1, target $TARGET)"
    "$RUSTC" --edition 2021 --crate-type rlib --crate-name speeddemon_core \
        --target "$TARGET" \
        -C opt-level=3 -C codegen-units=1 -C panic=abort \
        -C target-feature=+crt-static \
        -o "$CORE_RLIB" "$APP_DIR/core/src/lib.rs"
fi

# ---- compile the track ---------------------------------------------------
# NOTE THE CRATE NAME: rustc rejects hyphens in `--crate-name`, so the
# internal name uses an underscore while the FILE keeps the hyphen. The
# original build-all.sh passed `speeddemon-a` here, which made every Docker
# build fail with "invalid character `'-'` in crate name" — a bug found only
# by actually running this script.
echo "[build-one] compiling $BIN (track $TRACK, source tracks/$TRACK_DIR)"
# Capture stderr so a profile that rustc REFUSED cannot pass unnoticed. rustc
# treats an unreadable profile as a warning and still exits 0, so the exit code
# alone cannot distinguish "profiled" from "silently unprofiled" — which is
# exactly how the directory bug survived. Any complaint about the profile is
# now fatal on the scored path.
COMPILE_LOG="$(mktemp 2>/dev/null || echo "/tmp/build-one.$$.log")"
if ! "$RUSTC" --edition 2021 --crate-name "speeddemon_${TRACK}" \
    --extern speeddemon_core="$CORE_RLIB" \
    -L "$OUT_DIR" \
    -C opt-level=3 -C codegen-units=1 -C lto=thin -C panic=abort \
    -C strip=symbols \
    $PGO_FLAGS \
    $LINK_ARGS \
    --target "$TARGET" \
    -o "$OUT_DIR/$BIN" "$APP_DIR/tracks/$TRACK_DIR/src/main.rs" 2>"$COMPILE_LOG"
then
    cat "$COMPILE_LOG" >&2
    echo "[build-one] FATAL: rustc failed for track $TRACK" >&2
    exit 1
fi
cat "$COMPILE_LOG" >&2
if [ -n "$PROFILE_FILE" ] && grep -qi "profile" "$COMPILE_LOG"; then
    echo "[build-one] FATAL: PGO was requested but rustc did not accept the profile:" >&2
    grep -i "profile" "$COMPILE_LOG" >&2
    echo "[build-one] Refusing to ship an unprofiled binary that claims to be profiled." >&2
    echo "[build-one] Re-generate it: sh tooling/pgo-build.sh --track $TRACK --clean" >&2
    exit 1
fi
rm -f "$COMPILE_LOG"

echo "[build-one] ok: $OUT_DIR/$BIN"
