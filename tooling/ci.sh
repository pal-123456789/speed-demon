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
#   1  format drift           (rustfmt --check, harness)
#   2  lints                  (clippy -D warnings, harness)
#   3  unit tests             (harness)
#   4  offline build          (harness, --offline)
#   5  scored build path      (per-track build.sh: rustc direct, offline)
#   6  build-context replay   (each track builds from a clean COPY of its folder)
#   7  image contract         (digest-pinned allowlisted stages, no cached outputs)
#   8  K what-if self-check   (scoring math pinned to the published example)
#   9  S and D axes           (code lines per track folder; dependency honesty)
#  10  per-track gates        (verify: determinism, sandbox, timeout)
#  11  scaffold sentinels     (a stub must never reach the freeze)
#  12  the pre-freeze lockdown
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
TRACKS="A,B,C,D"
while [ $# -gt 0 ]; do
  case "$1" in
    --fast) FAST=1 ;;
    --tracks) TRACKS="$2"; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

FAILED=0
SKIPPED=0
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

# A skip is NOT a pass. Skips are counted separately and reported at the end,
# because a green banner over a skipped gate is exactly the failure mode this
# script was audited for.
skip() {
  printf '  \033[33mSKIP\033[0m %s\n' "$1"
  SKIPPED=$((SKIPPED + 1))
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
if cargo fmt --manifest-path verification/harness/Cargo.toml -- --check >/tmp/sd-fmt.txt 2>&1; then
  ok "no formatting drift (harness)"
else
  if grep -qiE "component.*rustfmt|rustup|no such command|unrecognized" /tmp/sd-fmt.txt; then
    skip "rustfmt is not installed on this host"
  else
    cat /tmp/sd-fmt.txt
    fail "rustfmt drift — run: cargo fmt --manifest-path verification/harness/Cargo.toml"
  fi
fi

# ── 2. lints ──────────────────────────────────────────────────────────────
step "LINTS (clippy -D warnings)"
if cargo clippy --manifest-path verification/harness/Cargo.toml --all-targets --offline -- -D warnings >/tmp/sd-clippy.txt 2>&1; then
  ok "no clippy warnings (harness)"
else
  if grep -qiE "component.*clippy|rustup|no such command|unrecognized" /tmp/sd-clippy.txt; then
    skip "clippy is not installed on this host"
  else
    cat /tmp/sd-clippy.txt
    fail "clippy warnings (see above)"
  fi
fi

# ── 3. tests ──────────────────────────────────────────────────────────────
step "UNIT TESTS"
if cargo test --offline --manifest-path verification/harness/Cargo.toml >/tmp/sd-test.txt 2>&1; then
  # cargo test can exit 0 while reporting no passing test at all (a changed
  # output format, a filtered run). Counting "test result: ok" lines and
  # printing the count without checking it produced "PASS 0 test binaries
  # passed" — a green tick over zero evidence.
  NTESTS="$(grep -c 'test result: ok' /tmp/sd-test.txt || true)"
  NPASSED="$(grep -oE 'test result: ok\. [0-9]+ passed' /tmp/sd-test.txt |
    awk '{s += $4} END {print s + 0}')"
  if [ "${NPASSED:-0}" -gt 0 ]; then
    ok "$NPASSED tests passed across $NTESTS binaries"
  else
    cat /tmp/sd-test.txt
    fail "cargo test exited 0 but reported NO passing tests — the output format changed or the run was empty"
  fi
else
  tail -40 /tmp/sd-test.txt
  fail "unit tests failed"
fi

# ── 5. offline build ──────────────────────────────────────────────────────
step "OFFLINE BUILD (--offline, no network)"
if cargo build --release --offline --manifest-path verification/harness/Cargo.toml >/tmp/sd-build.txt 2>&1; then
  ok "release build succeeded offline"
else
  tail -40 /tmp/sd-build.txt
  fail "offline build failed"
fi

# The SCORED image is built per track folder: `docker build ... track-$TRACK`
# with the track folder as the context. Each track's build.sh compiles its
# self-contained src/ with rustc directly inside the digest-pinned rust image —
# no cargo, no registry, no network. That exact path is what the image uses, so
# it gets its own gate.
step "SCORED BUILD PATH (per-track build.sh: rustc direct, offline)"
if sh track-A/build.sh && sh track-B/build.sh && sh track-C/build.sh && sh track-D/build.sh \
   >/tmp/sd-scorebuild.txt 2>&1; then
  RUNS="$(ls track-?/run 2>/dev/null | wc -l | tr -d ' ')"
  if [ "$RUNS" = "4" ]; then
    ok "all 4 track folders built ./run (rustc, offline)"
  else
    fail "expected 4 binaries, found $RUNS"
  fi
else
  tail -25 /tmp/sd-scorebuild.txt
  fail "the scored build path is broken — the image would fail to build"
fi

# The scored build copies the track folder as the Docker context ($COPY .
# /src). A build that "works here" because it reads a file OUTSIDE the folder
# (untracked, gitignored, or just nearby) fails on the scoring box. Replay the
# context from a clean copy so that contract is a gate, not a surprise at H68.
step "BUILD-CONTEXT REPLAY (each track builds from a clean copy)"
if sh tooling/context-replay.sh >/tmp/sd-replay.txt 2>&1; then
  ok "all 4 tracks build + run from a clean context copy"
else
  grep 'FAIL' /tmp/sd-replay.txt | sed 's/^/  /'
  fail "a track depends on something outside its folder — the scored build would fail"
fi

# The K formula is the team's compass; the what-if tool must keep reproducing
# the published worked example (K 6.41, speed 40.4) or the scoring math has
# drifted. Also a reminder of where the next hour pays most.
step "K WHAT-IF SELF-CHECK (scoring math pinned to the published example)"
if "$PY" tooling/k_whatif.py --selfcheck >/tmp/sd-kwhatif.txt 2>&1; then
  ok "what-if explorer reproduces the published example (K 6.41, speed 40.4)"
else
  cat /tmp/sd-kwhatif.txt
  fail "the K math drifted from the events page"
fi

# The rules announced before the Oct 28 freeze require the final image to
# "build from a published, digest-pinned toolchain image, so code can't hide
# in a custom base image", cap builds at 10 minutes and images at 2 GB, and
# forbid shipping cached outputs. None of that was enforced anywhere.
step "IMAGE CONTRACT (digest-pinned base, no cached outputs, size/time caps)"
if "$PY" tooling/check_base_pinned.py >/tmp/sd-imgcontract.txt 2>&1; then
  ok "every base image is digest-pinned; no cached outputs in the context"
else
  grep 'FAIL' /tmp/sd-imgcontract.txt | sed 's/^/  /'
  fail "the image contract is violated (docs/RULE-CHANGES-oct28.md §3-4)"
fi

# A .ps1 without a UTF-8 BOM is unparseable by Windows PowerShell 5.1, and
# the em-dash that broke it (0x94) silently ends a string instead of raising —
# so the breakage shows up as a wrong answer, not a parse error.
step "POWERSHELL ENCODING (UTF-8 BOM, PS 5.1 parseable)"
if "$PY" tooling/check_ps1_bom.py >/tmp/sd-bom.txt 2>&1; then
  ok "every .ps1 has a UTF-8 BOM and parses under PowerShell 5.1"
else
  grep -v '^ok' /tmp/sd-bom.txt | head -20 | sed 's/^/  /'
  fail "a .ps1 would fail to parse under Windows PowerShell 5.1"
fi

# The PGO recipe is an in-window decision once the real implementations exist;
# nothing wired for the pre-kickoff scaffolds. (Build-time training is allowed
# per the rules, but a profile of a stub is worse than none, so there is no PGO
# gate until H0.)

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

# Every other gate tests a piece. This one tests the ASSEMBLY: each track's
# binary run against a real data dir + empty out dir, /data-style read-only
# input, /out writable, exactly one file out. A piece can be green while the
# assembly is broken.
#
# On Windows the track binaries are PE files; the functional gates still run
# (the binary executes fine), only the ELF assertion is N/A. WSL would be a
# real Linux kernel — the strongest available substitute for the scoring box.
step "END-TO-END SANDBOX (all 4 track binaries, real Linux)"
if [ -x track-A/run ]; then
  if sh tooling/e2e-sandbox.sh >/tmp/sd-e2e.txt 2>&1; then
    ok "all 4 tracks: binary runs, 1 file out, determinism, failure modes"
  else
    grep -E 'FAIL|passed,' /tmp/sd-e2e.txt | sed 's/^/  /'
    fail "end-to-end sandbox test failed"
  fi
elif command -v wsl.exe >/dev/null 2>&1 || command -v wsl >/dev/null 2>&1; then
  WSL_BIN="$(command -v wsl.exe || command -v wsl)"
  # Translate the repo path to its /mnt/<drive>/... form for the WSL side.
  WSL_ROOT="$("$WSL_BIN" -e sh -c 'wslpath -a "$1" 2>/dev/null' _ "$PWD" 2>/dev/null | tr -d '\r')"
  if [ -n "${WSL_ROOT:-}" ] &&
     "$WSL_BIN" -e sh -c "cd '$WSL_ROOT' && sh track-A/build.sh && sh track-B/build.sh && sh track-C/build.sh && sh track-D/build.sh && sh tooling/e2e-sandbox.sh" >/tmp/sd-e2e.txt 2>&1; then
    ok "all 4 tracks via WSL: build, run, 1 file out, determinism"
  else
    grep -E 'FAIL|passed,|FATAL' /tmp/sd-e2e.txt 2>/dev/null | sed 's/^/  /'
    fail "end-to-end sandbox test failed under WSL"
  fi
else
  skip "end-to-end sandbox skipped (no ELF binaries and no WSL on this host)"
fi

# ── 5+6. the S and D axes ─────────────────────────────────────────────────
# One pass produces both. There was a `--s-only` flag here once that the
# tool never implemented, so the S axis printed nothing at all and the step
# looked like it had passed — a silent gate is worse than a failing one.
#
# The old check grepped for a summary line the tool does not print, then
# printed PASS regardless of what the grep found. Now the JSON is asserted:
# every track must really be D = 1.0 with zero declared dependencies.
step "S AND D AXES (code lines, dependency honesty)"
if "$PY" tooling/measure_axes.py --all --json >/tmp/sd-axes.json 2>/tmp/sd-axes.txt; then
  if "$PY" tooling/check_axes.py /tmp/sd-axes.json >>/tmp/sd-axes.txt 2>&1; then
    grep -E "^(D|S) " /tmp/sd-axes.txt | sed 's/^/  /' || true
    ok "every track: D = 1.0000, zero declared dependencies"
  else
    cat /tmp/sd-axes.txt
    fail "the S/D axes do not match the rules in docs/RULE-CHANGES-oct28.md"
  fi
else
  cat /tmp/sd-axes.txt
  fail "measure_axes.py failed"
fi

# ── 7. per-track gates ────────────────────────────────────────────────────
step "TRACK GATES (determinism, sandbox, timeout)"
HARNESS="verification/harness/target/release/sd-harness"
if [ ! -x "$HARNESS" ] && [ -x "$HARNESS.exe" ]; then
  HARNESS="$HARNESS.exe"
fi
if [ ! -x "$HARNESS" ]; then
  cargo build --release --offline --manifest-path verification/harness/Cargo.toml >/dev/null 2>&1 || true
fi
for t in $(echo "$TRACKS" | tr ',' ' '); do
  tt="$(echo "$t" | tr '[:lower:]' '[:upper:]')"
  if [ "$FAST" = "1" ]; then
    skip "track $tt gates skipped (--fast)"
  elif [ "$(uname -s 2>/dev/null)" != "Linux" ]; then
    # The harness spawns the track binary, which on this host is a Windows PE
    # build; the definitive ELF semantics need a Linux host (or the WSL e2e
    # run in gate 10, which covers the same functional gates).
    skip "track $tt gates skipped (harness verify needs Linux ELF binaries; e2e suite covers this host)"
  else
    # The Rust harness is the canonical gate runner: the same code runs on
    # the scoring box, where PowerShell does not exist.
    if [ -x "$REPO/track-$tt/run" ]; then
      BIN="$REPO/track-$tt/run"
    else
      BIN="$REPO/track-$tt/run.exe"
    fi
    if "$HARNESS" verify --track "$tt" --no-docker --bin "$BIN" >/tmp/sd-track-$tt.txt 2>&1; then
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
SENT_FILES="$(grep -rl 'SCAFFOLD_NOT_IMPLEMENTED' track-*/src 2>/dev/null | wc -l | tr -d ' ')"
if [ "$KICKOFF_EPOCH" != "0" ] && [ "$NOW_EPOCH" -lt "$KICKOFF_EPOCH" ]; then
  if [ "$SENT_FILES" = "4" ]; then
    ok "pre-kickoff: all 4 track folders are still scaffolds (rule 05 holds)"
  else
    fail "pre-kickoff: $SENT_FILES/4 track folders carry a sentinel — a track lost its stub before the window opened"
  fi
else
  if [ "$SENT_FILES" = "0" ]; then
    ok "in-window: no scaffold sentinel in the tree"
  else
    fail "$SENT_FILES track folder(s) still contain a scaffold sentinel — that track would score zero"
  fi
fi

# ── 9. lockdown ───────────────────────────────────────────────────────────
# The lockdown is the FREEZE-DAY gate (tree clean, hashes pinned, public
# remote, no sentinel). Pre-kickoff those assertions are all false BY DESIGN
# — the scaffold sentinel is mandatory, the tree is mid-edit, and the public
# repo lands on Nov 2 — so running it before kickoff only trains people to
# ignore a red gate. It runs from kickoff onwards; KICKOFF_EPOCH comes from
# the same clock as the sentinel gate above.
step "PRE-FREEZE LOCKDOWN"
if [ "$FAST" = "1" ]; then
  skip "lockdown skipped (--fast)"
elif [ "$KICKOFF_EPOCH" != "0" ] && [ "$NOW_EPOCH" -lt "$KICKOFF_EPOCH" ]; then
  skip "lockdown is a freeze-day gate — it runs from kickoff (pre-kickoff the sentinel, tree, and remote assertions are false by design)"
else
  if "$HARNESS" lockdown >/tmp/sd-freeze.txt 2>&1; then
    ok "lockdown passed"
  else
    grep -E "FAIL|LOCKDOWN" /tmp/sd-freeze.txt | head -30
    fail "lockdown failed"
  fi
fi

printf '\n%s\n' "============================================================"
if [ "$FAILED" = "0" ] && [ "$SKIPPED" = "0" ]; then
  printf '\033[32mCI: ALL GATES GREEN\033[0m\n'
  exit 0
elif [ "$FAILED" = "0" ]; then
  # A skip is not a pass. Saying "ALL GATES GREEN" over a skipped gate is the
  # exact defect this script was audited for, so the banner says what happened.
  printf '\033[33mCI: %d GATE(S) SKIPPED, none failed\033[0m\n' "$SKIPPED"
  exit 0
else
  printf '\033[31mCI: %d GATE(S) FAILED\033[0m\n' "$FAILED"
  [ "$SKIPPED" != "0" ] && printf '\033[33mCI: %d GATE(S) ALSO SKIPPED\033[0m\n' "$SKIPPED"
  exit 1
fi
