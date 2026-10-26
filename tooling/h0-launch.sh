#!/bin/sh
# h0-launch.sh — the kickoff GO sequence. Run at Oct 30 18:00 UTC.
#
# Verifies the machine state, builds everything, runs the gates, regenerates
# fixtures, re-pins the expected hashes (the first real implementation CHANGES
# the output — re-pinning is deliberate), and points at the runbook. The goal:
# from kickoff to a SHA-green first baseline on all tracks in the least wall
# time possible, because everything else in the 72 hours is measured from
# there.
#
# Usage:  sh tooling/h0-launch.sh
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "══════════════════════════════════════════════════════════════"
echo " H0 LAUNCH — $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
echo "══════════════════════════════════════════════════════════════"

# 1. repo state
echo "== 1. repo state =="
git status --porcelain | head -5
echo "  head: $(git rev-parse --short HEAD 2>/dev/null)"

# 2. build all four tracks
echo "== 2. build all tracks (rustc, offline) =="
for t in a b c d; do
  sh "track-$t/build.sh" >/tmp/sd-h0-build-$t.log 2>&1 \
    && echo "  ok track-$t" || { echo "  FAIL track-$t"; tail -5 /tmp/sd-h0-build-$t.log; exit 1; }
done

# 3. gates (fast suite — the full suite runs after the first baselines)
echo "== 3. gate suite (--fast) =="
sh tooling/ci.sh --fast || { echo "FAIL: a gate is red — fix before reading specs"; exit 1; }

# 4. fixtures (published bytes + adversarial corpora; the real sample lands
#    at 06:00 UTC, the generators are the anti-overfit layer)
echo "== 4. fixtures =="
python tooling/gen-fixtures.py --all >/dev/null 2>&1 && echo "  ok generators ran"

# 5. copy the generic infrastructure in (rule-05-safe: generic primitives)
echo "== 5. generic kit =="
for t in a b c d; do
  cp prep/bump.rs prep/lines.rs prep/parse_u32.rs prep/itoa.rs "track-$t/src/" 2>/dev/null
done
echo "  copied prep/{bump,lines,parse_u32,itoa}.rs into track-*/src (S now counts them)"

# 6. re-pin expected hashes from the CURRENT binaries (scaffolds are
#    deterministic; the first real implementation re-pins again deliberately)
echo "== 6. hash pins =="
for t in a b c d; do sh tooling/pin-hash.sh "$t" >/dev/null 2>&1 && echo "  ok track-$t"; done

# 7. pointer to the runbook
echo ""
echo "══════════════════════════════════════════════════════════════"
echo " GO. Next: read the frozen specs + reference (docs/WINDOW-RUNBOOK.md)"
echo " then the Oct 28 intake: python tooling/spec_intake.py --check"
echo "══════════════════════════════════════════════════════════════"