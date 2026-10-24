#!/bin/sh
# Runtime entrypoint. The scoring harness runs ONE image for every track and
# selects the track at RUN time via `-e TRACK=$TRACK`:
#
#   docker run ... -v /srv/speeddemon/data/track-$TRACK:/data:ro \
#                  -v /srv/speeddemon/out/$RUN:/out:rw -e TRACK=$TRACK IMAGE
#
# So this dispatcher MUST read $TRACK and exec the matching binary.
#
# Contract:
#   reads  /data/...   (read-only mount)
#   writes /out/<one file>  (exactly one file — anything else is flagged by
#                            the filesystem diff)
#   exits  0
set -eu

APP_DIR="$(cd "$(dirname "$0")" && pwd)"
TRACK="${TRACK:-A}"
TRACK="$(printf '%s' "$TRACK" | tr '[:lower:]' '[:upper:]' | tr -cd 'A-Z')"
[ -n "$TRACK" ] || TRACK="A"

case "$TRACK" in
    A) BIN="speeddemon-a" ;;
    B) BIN="speeddemon-b" ;;
    C) BIN="speeddemon-c" ;;
    D) BIN="speeddemon-d" ;;
    E) BIN="speeddemon-e" ;;
    *)
        echo "[run] FATAL: unknown TRACK='$TRACK' (expected A..E)" >&2
        exit 1
        ;;
esac

BINPATH="$APP_DIR/bin/$BIN"
if [ ! -x "$BINPATH" ]; then
    echo "[run] FATAL: missing binary $BINPATH" >&2
    exit 1
fi

# exec so the binary's exit code is the container's exit code and signals
# propagate to the harness directly.
exec "$BINPATH" "$@"
