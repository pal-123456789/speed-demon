#!/bin/sh
# Vendor the Rust toolchain into the build context so the Docker build can
# run with `--network=none`.
#
# WHY THIS EXISTS
#   The scoring harness runs:  docker build --network=none -t speeddemon/$TEAM:$COMMIT .
#   A Dockerfile that fetches anything (apt, cargo, pip, curl) fails the
#   build, and the build failing means the submission scores ZERO.
#   debian:13-slim ships no rustc AND no C compiler, so the toolchain — and
#   the linker — must come from the build context.
#
# THE TARGET MATTERS MORE THAN IT LOOKS
#   The build vendors the MUSL target, not the gnu one. A gnu link needs
#   `cc` at link time, and the builder image has no C compiler and no
#   network to install one. The musl target with the toolchain's own
#   rust-lld produces a fully static binary that needs no libc at all.
#   Vendoring gnu here while build-one.sh links musl is exactly how an image
#   build fails on the scoring box with no useful error.
#
# HOW THE TOOLCHAIN DIRECTORY IS FOUND
#   rustup names the toolchain directory after the HOST triple
#   (1.86.0-x86_64-unknown-linux-gnu on a Linux host,
#    1.86.0-x86_64-pc-windows-msvc on a Windows host), NOT after the target.
#   Guessing that directory from the target happens to work on Linux and is
#   wrong everywhere else, so the directory is discovered from rustup itself.
#
# WHEN TO RUN IT
#   On a NETWORKED machine, BEFORE the hack starts. Re-run it whenever
#   rust-toolchain.toml changes.
#
#   vendor/ is gitignored (~1 GB). The Docker build context includes it
#   (.dockerignore deliberately does not exclude vendor/), so the toolchain
#   has to be present in the working tree when the image is built. Record
#   how it reaches the scoring box in verification/contract.toml before the
#   freeze, and verify that path — never after.
#
# USAGE
#   sh tooling/vendor-toolchain.sh
#
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/vendor/toolchain"
MUSL_TARGET="x86_64-unknown-linux-musl"

echo "[vendor] dest   : $DEST"

# ---- read the pinned channel --------------------------------------------
CHANNEL="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml")"
if [ -z "$CHANNEL" ]; then
    echo "[vendor] FATAL: could not read the channel from rust-toolchain.toml" >&2
    exit 1
fi
echo "[vendor] channel: $CHANNEL"
echo "[vendor] target : $MUSL_TARGET (static — the builder image has no C compiler)"

# ---- host prerequisites --------------------------------------------------
for tool in curl sed; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "[vendor] FATAL: '$tool' is required and not on PATH" >&2
        exit 1
    fi
done

if [ -d "$DEST" ]; then
    echo "[vendor] existing toolchain found — removing"
    rm -rf "$DEST"
fi
mkdir -p "$DEST"

# ---- install --------------------------------------------------------------
export RUSTUP_HOME="$DEST"
export CARGO_HOME="$DEST/cargo"
mkdir -p "$CARGO_HOME"

echo "[vendor] downloading rustup + the pinned toolchain (this needs network)"
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none

"$CARGO_HOME/bin/rustup" toolchain install "$CHANNEL" \
    --profile minimal --target "$MUSL_TARGET" --no-self-update

# ---- discover the toolchain directory (host triple, not the target) ------
# rustup names it after the host. Asking rustup which toolchains exist and
# matching on the channel is the only portable way.
TOOLCHAIN_DIR=""
for d in "$DEST"/toolchains/*/; do
    [ -d "$d" ] || continue
    case "$(basename "$d")" in
        "$CHANNEL"-*) TOOLCHAIN_DIR="$d" ;;
    esac
done
if [ -z "$TOOLCHAIN_DIR" ] || [ ! -x "$TOOLCHAIN_DIR/bin/rustc" ]; then
    echo "[vendor] FATAL: could not locate the installed toolchain under $DEST/toolchains" >&2
    ls -la "$DEST/toolchains" 2>/dev/null || true
    exit 1
fi
echo "[vendor] toolchain dir: $TOOLCHAIN_DIR"

# ---- stage the entry points where build-one.sh expects them --------------
mkdir -p "$DEST/bin"
for f in rustc cargo rustup; do
    if [ -f "$TOOLCHAIN_DIR/bin/$f" ]; then
        ln -sf "$TOOLCHAIN_DIR/bin/$f" "$DEST/bin/$f"
    fi
done

# ---- VERIFY: the three things the offline build actually needs -----------
FAIL=0
SYSROOT="$("$DEST/bin/rustc" --print sysroot)"

if ! ls "$SYSROOT/lib/rustlib/$MUSL_TARGET/lib/"libstd-*.rlib >/dev/null 2>&1; then
    echo "[vendor] FAIL: the musl standard library is missing" >&2
    FAIL=1
else
    echo "[vendor] ok: musl std present"
fi

HOST_TARGET="$("$DEST/bin/rustc" -vV | sed -n 's/^host: //p')"
LLD="$SYSROOT/lib/rustlib/$HOST_TARGET/bin/rust-lld"
if [ -x "$LLD" ]; then
    echo "[vendor] ok: rust-lld present ($LLD)"
else
    echo "[vendor] FAIL: rust-lld missing at $LLD — there is no linker without it" >&2
    FAIL=1
fi

if [ "$FAIL" != "0" ]; then
    echo "[vendor] the vendored toolchain cannot build the submission. Do NOT proceed." >&2
    exit 1
fi

echo "[vendor] done:"
"$DEST/bin/rustc" --version || true
du -sh "$DEST" || true

cat <<'EOF'

NEXT STEPS
  1. Prove the offline build works before the hack, not during it:
       docker build --network=none -t speeddemon/local:test .
  2. Record in verification/contract.toml how vendor/ reaches the scoring
     box (release asset vs a directory copied ahead of time), and verify
     that path on the box itself.
  3. Re-run this script whenever rust-toolchain.toml changes.
EOF
