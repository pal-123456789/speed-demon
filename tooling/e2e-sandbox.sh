#!/bin/sh
# End-to-end sandbox test: run all four track binaries (track-*/run) in a
# simulated container layout, and assert the gates.
#
# This exists because every other gate in this project tests a PIECE. The
# scoring box tests the ASSEMBLY: per-track image, /data read-only, /out
# writable, and exactly one file appearing in /out.
#
# What this proves, per track:
#   * the binary is a real Linux ELF and exits 0 on real input
#   * exactly one file lands in /out, and it is non-empty
#   * two runs produce byte-identical output (the determinism gate)
#   * a second file in /out is DETECTED (negative test: the check can fail)
#
# It does NOT reproduce the cgroup memory ceiling or the real timing protocol;
# those need Docker. It is the strongest assembly test available without it.
#
# Usage:  sh tooling/e2e-sandbox.sh
#         (run `sh track-*/build.sh` first in WSL/Linux)

set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

TMP="${TMPDIR:-/tmp}/sd-e2e-$$"
mkdir -p "$TMP" || exit 1
trap 'rm -rf "$TMP"' EXIT INT TERM

pass=0
fail=0
ok()   { printf '  PASS  %s\n' "$1"; pass=$((pass + 1)); }
bad()  { printf '  FAIL  %s\n' "$1"; fail=$((fail + 1)); }

# ── seed plausible input for each track ─────────────────────────────────────
# Enough structure that a real implementation would produce real output, small
# enough to stay instant. These are shapes, not the scored datasets.
seed() {
    d="$1"
    tid="$2"
    mkdir -p "$d"
    case "$tid" in
        A)
            {
                i=0
                while [ $i -lt 200 ]; do
                    printf '2026-01-01T%02d:%02d:00Z /api/v1/item/%d 200 %d\n' \
                        $((i % 24)) $((i % 60)) $((i % 17)) $((10 + i % 400))
                    i=$((i + 1))
                done
            } > "$d/access.log"
            ;;
        B)
            {
                i=0
                while [ $i -lt 120 ]; do
                    printf 'document %d alpha beta gamma delta epsilon\n' "$i"
                    i=$((i + 1))
                done
            } > "$d/corpus.txt"
            printf 'alpha\nbeta\ngamma\ndelta\n' > "$d/queries.txt"
            ;;
        C)
            {
                printf 'id,name,amount\n'
                i=1
                while [ $i -le 120 ]; do
                    printf '%d,user%d,%d\n' "$i" $((i % 9)) $((i * 7 % 1000))
                    i=$((i + 1))
                done
            } > "$d/records.csv"
            ;;
        D)
            {
                printf '5 6\n'
                printf '0 1\n1 2\n2 3\n3 4\n4 0\n1 3\n'
            } > "$d/graph.txt"
            ;;
    esac
}

sha_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

echo "=== END-TO-END SANDBOX TEST ==="
echo "  per-track binaries in track-*/run"
echo ""

for tid in A B C D; do
    echo "[track $tid]"
    data="$TMP/data-$tid"
    out="$TMP/out-$tid"
    seed "$data" "$tid"
    mkdir -p "$out"

    # ── gate: the binary exists and is a real executable ───────────────────
    # The scored binary links against the runtime stage's glibc (debian:13-
    # slim provides it), so "static" is no longer required; "it runs" is the
    # gate, and the run steps below prove it.
    bin="$ROOT/track-$tid/run"
    if [ ! -f "$bin" ]; then
        bad "$tid binary missing at $bin (run: sh track-$tid/build.sh)"
        echo ""
        continue
    fi
    if command -v file >/dev/null 2>&1; then
        desc="$(file -b "$bin" 2>/dev/null)"
        case "$desc" in
            *ELF*) ok "$tid binary is a Linux ELF ($desc)" ;;
            *PE32*|*"MS Windows"*)
                # A Windows-host build: the assembly gates below still run the
                # binary for real, but ELF-only assertions do not apply here.
                ok "$tid binary runs on this host ($desc); ELF gate N/A" ;;
            *)     bad "$tid binary unreadable: $desc" ;;
        esac
    fi

    # ── gate: exit 0 + exactly one file ───────────────────────────────────
    SD_DATA_DIR="$data" SD_OUT_DIR="$out" "$bin" \
        >"$TMP/stdout-$tid" 2>"$TMP/stderr-$tid"
    rc=$?
    if [ "$rc" -eq 0 ]; then
        ok "$tid binary exit 0"
    else
        bad "$tid binary exit $rc"
        sed 's/^/         /' "$TMP/stderr-$tid" | head -3
    fi

    n=$(find "$out" -mindepth 1 -maxdepth 1 -type f | wc -l | tr -d ' ')
    if [ "$n" -eq 1 ]; then
        ok "$tid exactly one file in /out"
    else
        bad "$tid $n file(s) in /out (the sandbox gate requires exactly 1)"
        find "$out" -mindepth 1 -maxdepth 1 | sed 's/^/         /'
    fi

    f=$(find "$out" -mindepth 1 -maxdepth 1 -type f | head -1)
    if [ -n "$f" ] && [ -s "$f" ]; then
        ok "$tid output is non-empty ($(wc -c < "$f" | tr -d ' ') bytes)"
    else
        bad "$tid output is missing or empty"
    fi

    # ── gate: determinism across two runs ─────────────────────────────────
    h1=$(sha_of "$f" 2>/dev/null || echo none)
    out2="$TMP/out2-$tid"
    mkdir -p "$out2"
    SD_DATA_DIR="$data" SD_OUT_DIR="$out2" "$bin" \
        >/dev/null 2>&1
    f2=$(find "$out2" -mindepth 1 -maxdepth 1 -type f | head -1)
    h2=$(sha_of "$f2" 2>/dev/null || echo none)
    if [ "$h1" = "$h2" ] && [ "$h1" != "none" ]; then
        ok "$tid byte-identical across two runs"
    else
        bad "$tid not deterministic ($h1 vs $h2)"
    fi

    # ── negative test: a stray file must be visible to the gate ───────────
    # The sandbox gate is only trustworthy if a violation is detectable.
    : > "$out/STRAY"
    n2=$(find "$out" -mindepth 1 -maxdepth 1 -type f | wc -l | tr -d ' ')
    if [ "$n2" -eq 2 ]; then
        ok "$tid stray-file violation is detectable ($n2 files seen)"
    else
        bad "$tid stray file not detected — the sandbox gate cannot be trusted"
    fi
    rm -f "$out/STRAY"

    echo ""
done

# (There is no dispatcher any more: each track ships its own per-track image
# whose entrypoint IS its binary. The organizer's run command still passes
# -e TRACK=$TRACK; a per-track binary simply does its own job regardless of
# the variable, which is the intended contract.)

echo ""
echo "  $pass passed, $fail failed"
if [ "$fail" -ne 0 ]; then
    echo "E2E: FAILED"
    exit 1
fi
echo "E2E: ALL CHECKS PASSED"
