#!/bin/sh
# Offline build. The scoring harness runs `docker build --network=none`, so
# NOTHING may be fetched here: no apt, no cargo, no pip, no curl.
#
# Strategy (in order):
#   1. Vendored toolchain in the build context (vendor/toolchain/bin/rustc).
#      Preferred: fully offline, hermetic, pinned.
#   2. rustc already on PATH (local dev, or a base image that ships one).
#
# We call rustc DIRECTLY (no cargo): cargo adds registry resolution we do not
# need for a std-only crate, and it is one more thing that can try to touch
# the network. Each track is a single crate plus one local path dependency
# (core), compiled with --extern.
set -eu

APP_DIR="$(cd "$(dirname "$0")" && pwd)"
OUT_DIR="$APP_DIR/bin"
mkdir -p "$OUT_DIR"

# ---- locate rustc --------------------------------------------------------
if [ -x "$APP_DIR/vendor/toolchain/bin/rustc" ]; then
    RUSTC="$APP_DIR/vendor/toolchain/bin/rustc"
    RUSTUP_HOME="$APP_DIR/vendor/toolchain"
    export RUSTUP_HOME
    PATH="$APP_DIR/vendor/toolchain/bin:$PATH"
    export PATH
    echo "[build] using vendored toolchain: $RUSTC"
elif command -v rustc >/dev/null 2>&1; then
    RUSTC="$(command -v rustc)"
    echo "[build] using rustc from PATH: $RUSTC"
else
    echo "[build] FATAL: no rustc found. Vendor one with tooling/vendor-toolchain.sh" >&2
    exit 1
fi

# ---- target selection ----------------------------------------------------
# Prefer a fully static musl binary when the vendored toolchain carries the
# musl target: the runtime image is debian:13-slim (glibc), but a static
# binary removes any libc-version mismatch between builder and runtime.
TARGET="x86_64-unknown-linux-gnu"
LINK_ARGS=""
if "$RUSTC" --print target-list 2>/dev/null | grep -q '^x86_64-unknown-linux-musl$'; then
    if "$RUSTC" --print target-libdir --target x86_64-unknown-linux-musl >/dev/null 2>&1; then
        TARGET="x86_64-unknown-linux-musl"
        LINK_ARGS="-C target-feature=+crt-static -C linker=rust-lld -C linker-flavor=gnu-lld"
        echo "[build] static musl target selected"
    fi
fi

RUSTFLAGS="-C opt-level=3 -C codegen-units=1 -C lto=thin -C panic=abort -C strip=symbols $LINK_ARGS"
export RUSTFLAGS

# ---- compile core --------------------------------------------------------
echo "[build] compiling core (std-only, D=1)"
"$RUSTC" --edition 2021 --crate-type rlib --crate-name speeddemon_core \
    -C opt-level=3 -C codegen-units=1 -C panic=abort \
    -o "$OUT_DIR/libspeeddemon_core.rlib" "$APP_DIR/core/src/lib.rs"

# ---- compile this track --------------------------------------------------
echo "[build] compiling speeddemon-d"
"$RUSTC" --edition 2021 --crate-name speeddemon-d \
    --extern speeddemon_core="$OUT_DIR/libspeeddemon_core.rlib" \
    -L "$OUT_DIR" \
    $RUSTFLAGS \
    --target "$TARGET" \
    -o "$OUT_DIR/speeddemon-d" "$APP_DIR/tracks/D-traverse/src/main.rs"

echo "[build] ok: $OUT_DIR/speeddemon-d"
