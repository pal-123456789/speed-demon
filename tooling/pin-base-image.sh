#!/bin/sh
# Re-pin every base image to its current digest.
#
# The rules announced before the Oct 28 freeze require the final image to
# "build from a published, digest-pinned toolchain image, so code can't hide
# in a custom base image" (docs/RULE-CHANGES-oct28.md §3). A tag like
# debian:13-slim is mutable, so the digest has to be written into the
# Dockerfile itself.
#
# This script resolves the digest once and rewrites ALL six Dockerfiles with
# it, so the root image and the five per-track images cannot drift apart.
#
# Usage:  sh tooling/pin-base-image.sh [image:tag]      (default debian:13-slim)
# Requires: docker (or a network + curl to the registry API)

set -eu

IMAGE="${1:-debian:13-slim}"
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

if ! command -v docker >/dev/null 2>&1; then
  echo "FATAL: docker is required to resolve the digest of $IMAGE" >&2
  echo "       with 'docker buildx imagetools inspect $IMAGE'" >&2
  exit 1
fi

echo "resolving $IMAGE ..."
# The manifest list digest is the portable pin: it resolves to the right
# per-architecture manifest on whichever builder consumes it.
DIGEST=$(docker buildx imagetools inspect "$IMAGE" --format '{{.Manifest.Digest}}' 2>/dev/null || true)

if [ -z "$DIGEST" ]; then
  DIGEST=$(docker buildx imagetools inspect "$IMAGE" 2>/dev/null |
    sed -n 's/^Digest:[[:space:]]*//p' | head -n 1)
fi

case "$DIGEST" in
  sha256:*) ;;
  *)
    echo "FATAL: could not resolve a digest for $IMAGE (got: '${DIGEST}')" >&2
    exit 1
    ;;
esac

PINNED="${IMAGE}@${DIGEST}"
echo "pinning to $PINNED"

FILES="$ROOT/Dockerfile"
for t in A B C D; do
  FILES="$FILES $ROOT/track-$t/Dockerfile"
done

changed=0
for f in $FILES; do
  [ -f "$f" ] || { echo "FATAL: missing $f" >&2; exit 1; }
  # Replace any existing pin (or a bare tag) on a FROM line.
  tmp="$f.pin.tmp"
  awk -v img="$IMAGE" -v pinned="$PINNED" '
    /^FROM[ \t]+/ {
      line = $0
      sub(img "@sha256:[0-9a-f]+", pinned, line)
      if (line == $0) sub(img "[ \t]", pinned " ", line)
      print line
      next
    }
    { print }
  ' "$f" > "$tmp"
  if ! cmp -s "$f" "$tmp"; then
    mv "$tmp" "$f"
    echo "  updated ${f#$ROOT/}"
    changed=$((changed + 1))
  else
    rm -f "$tmp"
    echo "  unchanged ${f#$ROOT/}"
  fi
done

echo "$changed file(s) updated"
echo
echo "verifying:"
python3 "$ROOT/tooling/check_base_pinned.py" || python "$ROOT/tooling/check_base_pinned.py"
