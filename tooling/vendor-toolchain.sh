#!/bin/sh
# Vendor the Rust toolchain into the build context so the Docker build can
# run with `--network=none`.
#
# WHY THIS EXISTS
#   The scoring harness runs:  docker build --network=none -t speeddemon/$TEAM:$COMMIT .
#   A Dockerfile that fetches anything (apt, cargo, pip, curl) fails the
#   build, and the build failing means the submission scores ZERO.
#   debian:13-slim ships no rustc, so the toolchain must come from the build
#   context.
#
# WHEN TO RUN IT
#   On a NETWORKED machine, BEFORE the hack starts (and re-run it if you
#   change rust-toolchain.toml). The vendored tree goes in vendor/toolchain/,
#   which .gitignore excludes from the repo — but the build context still
#   needs it, so either commit it as a release asset or add it to the Docker
#   build context when you tag the submission.
#
#   NOTE: vendor/ is gitignored because it is ~1-2 GB. Before the freeze you
#   must either (a) publish it as a release asset and have the build context
#   include it, or (b) confirm the scoring box already has a rust image
#   cached and switch the Dockerfile to a multi-stage `FROM rust:...` build.
#   Option (b) is only safe if the image is present in the box's cache —
#   verify it before the freeze, never after.
#
# USAGE
#   sh tooling/vendor-toolchain.sh [target-triple]
#
set -eu

TARGET="${1:-x86_64-unknown-linux-gnu}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/vendor/toolchain"

echo "[vendor] target : $TARGET"
echo "[vendor] dest   : $DEST"

if [ -d "$DEST" ]; then
    echo "[vendor] existing toolchain found — removing"
    rm -rf "$DEST"
fi
mkdir -p "$DEST"

# rustup with a self-contained toolchain
export RUSTUP_HOME="$DEST"
export CARGO_HOME="$DEST/cargo"
mkdir -p "$CARGO_HOME"

echo "[vendor] downloading rustup + the pinned toolchain (this needs network)"
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none

# The channel must match rust-toolchain.toml exactly.
CHANNEL="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml")"
if [ -z "$CHANNEL" ]; then
    echo "[vendor] FATAL: could not read the channel from rust-toolchain.toml" >&2
    exit 1
fi
echo "[vendor] installing toolchain $CHANNEL with target $TARGET"

"$CARGO_HOME/bin/rustup" toolchain install "$CHANNEL" \
    --profile minimal --target "$TARGET" --no-self-update

# Stage the toolchain where build.sh expects it: vendor/toolchain/bin/rustc
mkdir -p "$DEST/bin"
for f in rustc rustdoc cargo rustfmt; do
    if [ -f "$DEST/toolchains/$CHANNEL-$TARGET/bin/$f" ]; then
        ln -sf "$DEST/toolchains/$CHANNEL-$TARGET/bin/$f" "$DEST/bin/$f"
    fi
done
# cargo/rustup shims are needed for `rustup` to find the toolchain
for f in rustup cargo rustc rustdoc; do
    if [ -f "$DEST/toolchains/$CHANNEL-$TARGET/bin/$f" ]; then
        ln -sf "$DEST/toolchains/$CHANNEL-$TARGET/bin/$f" "$DEST/bin/$f"
    fi
done

echo "[vendor] done:"
"$DEST/bin/rustc" --version || true
du -sh "$DEST" || true

cat <<'EOF'

NEXT STEPS
  1. Verify the offline build works:
       docker build --network=none -t speeddemon/local:test .
  2. Decide how the toolchain reaches the scoring box (release asset vs a
     cached rust image) and record that decision in docs/CONTRACT.md.
  3. Re-run this script whenever rust-toolchain.toml changes.
EOF
