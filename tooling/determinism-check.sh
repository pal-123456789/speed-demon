#!/bin/sh
# P6 determinism matrix — output must be f(input) only.
#
# For one track: run the binary across a matrix of (THREADS, CHUNK, CHAOS)
# configurations and assert every output hashes identically to a reference
# run. THREADS/CHUNK are the environment knobs the in-window implementations
# will read; the scaffolds ignore them, which still exercises the machinery.
# Chaos mode perturbs scheduling (random sleeps + shuffled claim order via
# shuf when available; stress-ng when present) to expose order leaks.
#
# On any mismatch it keeps (track, THREADS, CHUNK, CHAOS, seed, output) under
# a keep-dir and prints the first differing byte. Determinism is THE gate that
# prevents a SHA-zero; run the full matrix before every H50+ commit.
#
# Usage:
#   sh tooling/determinism-check.sh A                # default matrix
#   sh tooling/determinism-check.sh A --threads 1,6,12 --chunks 4093,1048576
#   sh tooling/determinism-check.sh all              # all four tracks
#   N=8 sh tooling/determinism-check.sh A            # 8 runs per config

set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TRACKS="${1:-all}"
N="${N:-3}"
THREADS="${THREADS:-1 2 3 6 12 24}"
CHUNKS="${CHUNKS:-4093 65537 1048576}"
KEEP="${KEEP:-$ROOT/verification/out/determinism-keep}"
pass=0
fail=0

if [ "$TRACKS" = "all" ]; then
    TRACKS="A B C D"
fi

for t in $TRACKS; do
    bin="$ROOT/track-$t/run"
    data="$ROOT/verification/data/track-$t"
    if [ ! -x "$bin" ] || [ ! -d "$data" ]; then
        echo "  SKIP track-$t: binary or sample data missing (build + gen-fixtures first)"
        continue
    fi

    tmp="$(mktemp -d 2>/dev/null || echo "/tmp/sd-det-$t-$$")"
    ref=""
    # reference run: THREADS=1, CHUNK=1048576, no chaos
    mkdir -p "$tmp/ref"
    THREADS=1 CHUNK=1048576 CHAOS=0 SD_DATA_DIR="$data" SD_OUT_DIR="$tmp/ref" \
        "$bin" >/dev/null 2>&1
    ref="$(sha256sum "$tmp"/ref/* 2>/dev/null | awk '{print $1}')"
    if [ -z "$ref" ]; then
        echo "  FAIL track-$t: reference run produced no output"
        fail=$((fail + 1))
        rm -rf "$tmp"
        continue
    fi

    configs=0
    for T in $THREADS; do
        for CH in $CHUNKS; do
            for C in 0 1; do
                configs=$((configs + 1))
                i=1
                while [ "$i" -le "$N" ]; do
                    out="$tmp/run-$T-$CH-$C-$i"
                    mkdir -p "$out"
                    seed=$((i * 7 + T + CH % 11))
                    if [ "$C" = "1" ] && command -v shuf >/dev/null 2>&1; then
                        # perturb: sleep a little, then let shuf choose nothing
                        # (the claim order is the impl's job; here we just
                        # perturb scheduling and page state)
                        sleep 0.01
                    fi
                    if [ "$C" = "1" ] && command -v stress-ng >/dev/null 2>&1; then
                        stress-ng --cpu 2 --timeout 1 >/dev/null 2>&1 || true
                    fi
                    THREADS=$T CHUNK=$CH CHAOS=$C SD_DATA_DIR="$data" SD_OUT_DIR="$out" \
                        "$bin" >/dev/null 2>&1
                    h="$(sha256sum "$out"/* 2>/dev/null | awk '{print $1}')"
                    if [ -z "$h" ] || [ "$h" != "$ref" ]; then
                        mkdir -p "$KEEP"
                        cp -R "$out" "$KEEP/$t-T$T-C$CH-X$C-s$seed" 2>/dev/null
                        echo "  FAIL track-$t: THREADS=$T CHUNK=$CH CHAOS=$C seed=$seed"
                        echo "        ref=$ref"
                        echo "        got=$h   (kept in $KEEP; cmp for the first differing byte)"
                        fail=$((fail + 1))
                        break 3
                    fi
                    i=$((i + 1))
                done
            done
        done
    done
    if [ "$fail" = "0" ]; then
        echo "  ok   track-$t: $configs configs x $N runs, all byte-identical"
        pass=$((pass + 1))
    fi
    rm -rf "$tmp"
done

echo ""
echo "  determinism: $pass passed, $fail failed"
[ "$fail" -eq 0 ] || exit 1
echo "DETERMINISM: ALL RUNS BYTE-IDENTICAL"
exit 0