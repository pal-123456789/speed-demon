#!/usr/bin/env bash
# ci.sh — one command, every gate.
#
#   sh tooling/ci.sh            # everything
#   sh tooling/ci.sh --fast    # skip the container builds
#   sh tooling/ci.sh --tracks A,C
#
# The order matters. Cheap and fatal checks come first, so a broken tree
# fails in seconds rather than after a five-minute Docker build:
#
#   0  rule 05 compliance     (pre-kickoff: no solution code in the tree)
#   1  format drift           (rustfmt --check)
#   2  lints                  (clippy -D warnings)
#   3  unit tests             (core + harness)
#   4  offline build          (--offline, and the vendored-toolchain path)
#   5  D axis                 (no non-stdlib dependencies, anywhere)
#   6  S axis                 (source size, per track)
#   7  per-track gates        (verify: determinism, sandbox, timeout)
#   8  scaffold sentinels     (a stub must never reach the freeze)
#   9  the pre-freeze lockdown
set -u

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO" || exit 1

FAST=0
TRACKS="A,B,C,D,E"
while [ $# -gt 0 ]; do
  case "$1" in
    --fast) FAST=1 ;;
    --tracks) TRACKS="$2"; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

FAILED=0
STEP=0

step() {
  STEP=$((STEP + 1))
  printf '\n\033[1m[%d] %s\033[0m\n' "$STEP" "$1"
}

fail() {
  printf '  \033[31mFAIL\033[0m %s\n' "$1"
  FAILED=$((FAILED + 1))
}

ok() {
  printf '  \033[32mPASS\033[0m %s\n' "$1"
}

# ── 0. rule 05 compliance ─────────────────────────────────────────────────
step "RULE 05 COMPLIANCE (pre-kickoff guard)"
if python3 tooling/rule05_guard.py >/tmp/sd-rule05.txt 2>&1; then
  ok "no track-solution code in the tree"
else
  cat /tmp/sd-rule05.txt
  fail "rule 05 guard blocked the tree (see above)"
fi

# ── 1. format ─────────────────────────────────────────────────────────────
step "FORMAT (rustfmt --check)"
if cargo fmt --all -- --check >/tmp/sd-fmt.txt 2>&1; then
  ok "no formatting drift"
else
  cat /tmp/sd-fmt.txt
  fail "rustfmt drift — run: cargo fmt --all"
fi

# ── 2. lints ──────────────────────────────────────────────────────────────
step "LINTS (clippy -D warnings)"
if cargo clippy --all-targets --offline -- -D warnings >/tmp/sd-clippy.txt 2>&1; then
  ok "no clippy warnings"
else
  cat /tmp/sd-clippy.txt
  fail "clippy warnings (see above)"
fi

# ── 3. tests ──────────────────────────────────────────────────────────────
step "UNIT TESTS"
if cargo test --offline >/tmp/sd-test.txt 2>&1; then
  ok "$(grep -c 'test result: ok' /tmp/sd-test.txt) test binaries passed"
else
  tail -40 /tmp/sd-test.txt
  fail "unit tests failed"
fi

# ── 4. offline build ──────────────────────────────────────────────────────
step "OFFLINE BUILD (--offline, no network)"
if cargo build --release --offline >/tmp/sd-build.txt 2>&1; then
  ok "release build succeeded offline"
else
  tail -40 /tmp/sd-build.txt
  fail "offline build failed"
fi

# ── 5. D axis ─────────────────────────────────────────────────────────────
step "D AXIS (dependency honesty)"
if python3 tooling/measure_axes.py --all >/tmp/sd-d.txt 2>&1; then
  grep -E "Track [A-E]: D" /tmp/sd-d.txt | sed 's/^/  /'
  ok "D = 1.0000 on every track"
else
  cat /tmp/sd-d.txt
  fail "D axis is not 1.0 — a non-stdlib dependency costs ~10-17% of K"
fi

# ── 6. S axis ─────────────────────────────────────────────────────────────
step "S AXIS (source size per track)"
python3 tooling/measure_axes.py --s-only 2>/dev/null | sed 's/^/  /' || true

# ── 7. per-track gates ────────────────────────────────────────────────────
step "TRACK GATES (determinism, sandbox, timeout)"
HARNESS="target/release/sd-harness"
if [ ! -x "$HARNESS" ]; then
  cargo build --release --offline -p sd-harness >/dev/null 2>&1 || true
fi
for t in $(echo "$TRACKS" | tr ',' ' '); do
  tt="$(echo "$t" | tr '[:lower:]' '[:upper:]')"
  if [ "$FAST" = "1" ]; then
    ok "track $tt skipped (--fast)"
  else
    # The Rust harness is the canonical gate runner: the same code runs on
    # the scoring box, where PowerShell does not exist.
    if "$HARNESS" verify --track "$tt" --no-docker >/tmp/sd-track-$tt.txt 2>&1; then
      ok "track $tt gates passed"
    else
      cat /tmp/sd-track-$tt.txt
      fail "track $tt gates failed"
    fi
  fi
done

# ── 8. sentinels ──────────────────────────────────────────────────────────
step "SCAFFOLD SENTINELS (a stub must never ship)"
SENT=$(grep -rl "SCAFFOLD_NOT_IMPLEMENTED" tracks/*/src 2>/dev/null | wc -l | tr -d ' ')
if [ "$SENT" = "0" ]; then
  ok "no scaffold sentinel in the tree"
else
  fail "$SENT track(s) still contain a scaffold sentinel"
fi

# ── 9. lockdown ───────────────────────────────────────────────────────────
step "PRE-FREEZE LOCKDOWN"
if [ "$FAST" = "1" ]; then
  ok "lockdown skipped (--fast)"
else
  if "$HARNESS" lockdown >/tmp/sd-freeze.txt 2>&1; then
    ok "lockdown passed"
  else
    grep -E "FAIL|LOCKDOWN" /tmp/sd-freeze.txt | head -30
    fail "lockdown failed"
  fi
fi

printf '\n%s\n' "============================================================"
if [ "$FAILED" = "0" ]; then
  printf '\033[32mCI: ALL GATES GREEN\033[0m\n'
  exit 0
else
  printf '\033[31mCI: %d GATE(S) FAILED\033[0m\n' "$FAILED"
  exit 1
fi
