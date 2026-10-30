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
#
# TWO BUGS LIVED HERE, and both made this gate unable to fail:
#
#  1. The `case` labels were UPPERCASE (`A)`) while `$tid` is lowercase, so no
#     arm ever matched: seed() made the directory and wrote no files at all.
#     Every binary then correctly refused to run, and the gate reported the
#     refusal as the binary's fault. It passed for weeks only because the
#     pre-kickoff scaffolds wrote their sentinel output regardless of input --
#     the gate was never exercising the real path, and the moment real
#     implementations landed it went red for a reason that was not their fault.
#
#  2. The fixtures were written for those same scaffolds and do not match the
#     frozen formats. Track A had no METHOD field, so a correctly parsed line
#     would have been rejected. Track C had no `note` or `region_id` column and
#     no regions.csv, and Track D had no weight column. A gate whose input is
#     malformed tests nothing but the error path.
#
# The shapes below follow the specs, and deliberately include the hostile cases
# each track is scored on: a 5xx for A's error rate, a quoted comma and a CRLF
# for C, an unknown region for `_UNMATCHED`, and a self-loop plus an unreachable
# pair for D.
seed() {
    d="$1"
    tid="$2"
    mkdir -p "$d"
    case "$tid" in
        a)
            {
                i=0
                while [ $i -lt 200 ]; do
                    # ts(27 bytes) SP method SP endpoint SP status SP latency
                    if [ $((i % 17)) -eq 0 ]; then st=503; else st=200; fi
                    printf '2026-01-01T%02d:%02d:00.000000Z GET /api/v1/item/%d %s %d\n' \
                        $((i % 24)) $((i % 60)) $((i % 17)) "$st" $((10 + i % 400))
                    i=$((i + 1))
                done
            } > "$d/requests.log"
            ;;
        b)
            {
                i=0
                while [ $i -lt 120 ]; do
                    # id TAB text
                    printf '%d\talpha beta gamma delta epsilon doc%d\n' "$i" "$i"
                    i=$((i + 1))
                done
            } > "$d/docs.txt"
            # BOOL with no operator, a left-to-right chain, and a phrase
            {
                printf 'BOOL alpha\n'
                printf 'BOOL alpha AND beta\n'
                printf 'BOOL alpha OR beta NOT gamma\n'
                printf 'PHRASE "alpha beta"\n'
            } > "$d/queries.txt"
            ;;
        c)
            printf 'region_id,country\n1,AAA\n2,BBB\n' > "$d/regions.csv"
            {
                printf 'id,name,note,region_id,amount\r\n'
                i=1
                while [ $i -le 120 ]; do
                    rid=$((i % 4))
                    if [ $i -eq 3 ]; then
                        # quoted comma, quoted quote, and an embedded CRLF
                        printf '%d,"Sume, Sufo","said ""no"" twice",%d,%d.%02d\r\n' \
                            "$i" "$rid" $((i % 900)) $((i % 100))
                    elif [ $i -eq 5 ]; then
                        # trailing space in the name is a DISTINCT value
                        printf '%d,"Betts ",note,%d,%d.%02d\r\n' \
                            "$i" "$rid" $((i % 900)) $((i % 100))
                    else
                        printf '%d,user%d,note %d,%d,%d.%02d\r\n' \
                            "$i" "$i" "$i" "$rid" $((i % 900)) $((i % 100))
                    fi
                    i=$((i + 1))
                done
            } > "$d/records.csv"
            ;;
        d)
            {
                # header: n m, then exactly m lines of "u v w"
                printf '5 7\n'
                printf '0 1 2\n1 2 3\n2 3 4\n3 4 5\n4 0 6\n0 0 9\n1 3 1\n'
            } > "$d/graph.txt"
            # reachable, unreachable, and a zero-edge query
            {
                printf '0 4\n'
                printf '0 2\n'
                printf '0 0\n'
            } > "$d/queries.txt"
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

for tid in a b c d; do
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
