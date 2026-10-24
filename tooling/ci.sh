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

# Some hosts ship `python3`, others only `python` (Windows, slim containers).
# Hardcoding either name makes every gate fail on the other host, and a gate
# that fails with "command not found" trains you to look away from red gates.
PY=""
for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1; then
        PY="$candidate"
        break
    fi
done
if [ -z "$PY" ]; then
    echo "FATAL: no python interpreter found (need python3 or python)" >&2
    exit 1
fi

# Force UTF-8 on the interpreter's stdio. On Windows the default code page
# mangles every non-ASCII character the tools print, which corrupts CI logs
# and makes real mojibake indistinguishable from log noise.
export PYTHONIOENCODING="utf-8"
export PYTHONUTF8="1"

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
if "$PY" tooling/rule05_guard.py >/tmp/sd-rule05.txt 2>&1; then
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

# ── 5. offline build ──────────────────────────────────────────────────────
step "OFFLINE BUILD (--offline, no network)"
if cargo build --release --offline >/tmp/sd-build.txt 2>&1; then
  ok "release build succeeded offline"
else
  tail -40 /tmp/sd-build.txt
  fail "offline build failed"
fi

# The SCORED image does NOT use cargo: it runs build-all.sh, which calls
# rustc directly and statically links through the toolchain's own rust-lld.
# That path has failed three separate ways (hyphenated crate name, an
# uninstalled target, an unstable linker flavor) and cargo never touches it,
# so it gets its own gate.
step "SCORED BUILD PATH (build-all.sh: rustc direct, static musl)"
if sh ./build-all.sh >/tmp/sd-scorebuild.txt 2>&1; then
  BINS="$(ls bin/speeddemon-? 2>/dev/null | wc -l | tr -d ' ')"
  if [ "$BINS" = "5" ]; then
    ok "all 5 binaries built and statically linked"
    grep -m1 'target .*linker' /tmp/sd-scorebuild.txt | sed 's/^/  /'
  else
    fail "expected 5 binaries, found $BINS"
  fi
else
  tail -25 /tmp/sd-scorebuild.txt
  fail "the scored build path is broken — the image would fail to build"
fi

step "RUN DISPATCHER (TRACK routing, loud failures)"
if sh tooling/test-dispatcher.sh >/tmp/sd-dispatch.txt 2>&1; then
  ok "dispatcher self-test passed"
else
  cat /tmp/sd-dispatch.txt
  fail "run dispatcher misroutes or fails silently"
fi

step "PER-TRACK DOCKERFILES (generated, cannot drift)"
if "$PY" tooling/gen_dockerfiles.py --check >/tmp/sd-docker.txt 2>&1; then
  ok "all 5 per-track Dockerfiles match the template"
else
  cat /tmp/sd-docker.txt
  fail "a per-track Dockerfile was hand-edited — run: python tooling/gen_dockerfiles.py"
fi

# A non-executable pre-commit hook is silently ignored by git on Linux, and a
# non-executable build.sh fails only when someone finally runs it. Neither
# shows up as a diff, so the mode has to be checked deliberately.
step "EXECUTABLE BITS (scripts and hook)"
if sh tooling/check-exec-bits.sh . >/tmp/sd-exec.txt 2>&1; then
  ok "every script and hook is executable in git"
else
  grep 'NO-X' /tmp/sd-exec.txt | sed 's/^/  /'
  fail "a script lost its executable bit — git update-index --chmod=+x <file>"
fi

# ── 5+6. the S and D axes ─────────────────────────────────────────────────
# One pass produces both. There was a `--s-only` flag here once that the
# tool never implemented, so the S axis printed nothing at all and the step
# looked like it had passed — a silent gate is worse than a failing one.
step "S AND D AXES (source size, dependency honesty)"
if "$PY" tooling/measure_axes.py --all >/tmp/sd-axes.txt 2>&1; then
  grep -E "Track [A-E]: D" /tmp/sd-axes.txt | sed 's/^/  /'
  ok "D = 1.0000 on every track"
else
  cat /tmp/sd-axes.txt
  fail "D axis is not 1.0 — a non-stdlib dependency costs ~10-17% of K"
fi
step "S AXIS DETAIL (normalised source bytes per track)"
grep -E "normalised bytes|the S axis" /tmp/sd-axes.txt | sed 's/^/  /' || true

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

# ── 9. sentinels ──────────────────────────────────────────────────────────
# The sentinel gate INVERTS at kickoff, so it must not be a bare grep:
#   before Oct 30 18:00 UTC, rule 05 forbids solution code, so every track
#   MUST still carry #SCAFFOLD_NOT_IMPLEMENTED — a missing sentinel means
#   unauthorised solution code and the guard already failed in step 1.
#   after kickoff, a sentinel means a stub could ship and score zero.
# A gate that fails for the wrong reason in one of the two modes trains you
# to ignore it, so the mode comes from the same clock the rule-05 guard uses.
step "SCAFFOLD SENTINELS (mode-aware)"
NOW_EPOCH="$(date -u +%s)"
KICKOFF_EPOCH="$(date -u -d '2026-10-30T18:00:00Z' +%s 2>/dev/null || echo 0)"
SENT_FILES="$(grep -rl 'SCAFFOLD_NOT_IMPLEMENTED' tracks/*/src 2>/dev/null | wc -l | tr -d ' ')"
if [ "$KICKOFF_EPOCH" != "0" ] && [ "$NOW_EPOCH" -lt "$KICKOFF_EPOCH" ]; then
  if [ "$SENT_FILES" = "5" ]; then
    ok "pre-kickoff: all 5 tracks are still scaffolds (rule 05 holds)"
  else
    fail "pre-kickoff: $SENT_FILES/5 tracks carry a sentinel — a track lost its stub before the window opened"
  fi
else
  if [ "$SENT_FILES" = "0" ]; then
    ok "in-window: no scaffold sentinel in the tree"
  else
    fail "$SENT_FILES track(s) still contain a scaffold sentinel — that track would score zero"
  fi
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
