#!/bin/sh
# Build ALL FIVE track binaries into /app/bin.
#
# The core crate is compiled once as an rlib and linked into each track
# binary with --extern, so we pay the core's compile cost once, not five
# times. No cargo, no network, no registry.
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
    echo "[build-all] using vendored toolchain: $RUSTC"
elif command -v rustc >/dev/null 2>&1; then
    RUSTC="$(command -v rustc)"
    echo "[build-all] using rustc from PATH: $RUSTC"
else
    echo "[build-all] FATAL: no rustc found. Vendor one with tooling/vendor-toolchain.sh" >&2
    exit 1
fi

# ---- target selection ----------------------------------------------------
TARGET="x86_64-unknown-linux-gnu"
LINK_ARGS=""
if "$RUSTC" --print target-list 2>/dev/null | grep -q '^x86_64-unknown-linux-musl$'; then
    if "$RUSTC" --print target-libdir --target x86_64-unknown-linux-musl >/dev/null 2>&1; then
        TARGET="x86_64-unknown-linux-musl"
        LINK_ARGS="-C target-feature=+crt-static -C linker=rust-lld -C linker-flavor=gnu-lld"
        echo "[build-all] static musl target selected"
    fi
fi

# ---- compile core once ---------------------------------------------------
echo "[build-all] compiling core (std-only, D=1)"
"$RUSTC" --edition 2021 --crate-type rlib --crate-name speeddemon_core \
    -C opt-level=3 -C codegen-units=1 -C panic=abort \
    -o "$OUT_DIR/libspeeddemon_core.rlib" "$APP_DIR/core/src/lib.rs"

# ---- compile each track --------------------------------------------------
build_track() {
    TRACK_DIR="$1"
    BIN="$2"
    echo "[build-all] compiling $BIN"
    "$RUSTC" --edition 2021 --crate-name "$BIN" \
        --extern speeddemon_core="$OUT_DIR/libspeeddemon_core.rlib" \
        -L "$OUT_DIR" \
        -C opt-level=3 -C codegen-units=1 -C lto=thin -C panic=abort \
        -C strip=symbols \
        $LINK_ARGS \
        --target "$TARGET" \
        -o "$OUT_DIR/$BIN" "$APP_DIR/tracks/$TRACK_DIR/src/main.rs"
}

build_track "A-ingest"   "speeddemon-a"
build_track "B-index"    "speeddemon-b"
build_track "C-join"     "speeddemon-c"
build_track "D-traverse" "speeddemon-d"
build_track "E-compress" "speeddemon-e"

echo "[build-all] ok:"
ls -l "$OUT_DIR"
