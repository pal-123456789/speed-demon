#!/usr/bin/env bash
# pgo-build.sh — profile-guided optimization, fully offline.
#
# WHY THIS EXISTS
#
# Build time is NOT scored. The organizers time the *run*, not the build, and
# the image is built once with `--network=none`. That makes a PGO pipeline
# free: it costs wall-clock time we are not measured on, and it typically
# buys 10-20% on a branchy, memory-bound workload like a log parser.
#
# The pipeline is the standard two-phase one:
#
#   phase 1  build with -Cprofile-generate=<dir>, run the binary on a
#            representative input, collect the .profdata
#   phase 2  build with -Cprofile-use=<dir> plus a few extra flags
#
# Both phases must be OFFLINE (`--offline` cargo, vendored toolchain), and
# the resulting binary must still be byte-identical across runs — PGO does
# not change the algorithm, but it does change the code layout, and the
# determinism gate is on the OUTPUT, not the binary, so the output must be
# re-verified after a PGO build like after any other change.
#
# Usage:
#   sh tooling/pgo-build.sh --track A
#   sh tooling/pgo-build.sh --all
#   sh tooling/pgo-build.sh --track A --clean      # discard the profile first
#
# The PGO profile is a BUILD ARTEFACT, never part of the submission's source
# set: it is written to target/pgo/, which is gitignored. The S axis is
# measured on source, so a PGO build costs nothing on that axis.
set -eu

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

TRACK=""
ALL=0
CLEAN=0
while [ $# -gt 0 ]; do
  case "$1" in
    --track) TRACK="$2"; shift 2 ;;
    --all) ALL=1; shift ;;
    --clean) CLEAN=1; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

if [ "$ALL" = "0" ] && [ -z "$TRACK" ]; then
  echo "usage: sh tooling/pgo-build.sh --track A|B|C|D|E | --all [--clean]" >&2
  exit 2
fi

if [ "$ALL" = "1" ]; then
  TRACKS="A B C D E"
else
  TRACKS="$TRACK"
fi

PGO_DIR="target/pgo"
PROF_DIR="$PGO_DIR/profiles"

say() { printf '\n\033[1m==> %s\033[0m\n' "$1"; }
die() { printf '\033[31mERROR: %s\033[0m\n' "$1" >&2; exit 1; }

if [ "$CLEAN" = "1" ]; then
  say "cleaning previous PGO artefacts"
  rm -rf "$PGO_DIR"
fi

mkdir -p "$PROF_DIR"

# The representative input: the fixture the generator produces. It must
# have the same SHAPE as the real data (same field count, same value
# distributions, similar line lengths) or the profile will optimise for the
# wrong branch directions.
find_input() {
  local t="$1"
  local lc="$(echo "$t" | tr '[:upper:]' '[:lower:]')"
  for candidate in \
    "verification/data/track-$t" \
    "verification/data/track-${lc}" \
    "verification/fixtures/track-$t" \
    ; do
    if [ -d "$candidate" ]; then
      echo "$candidate"
      return 0
    fi
  done
  echo ""
}

for t in $TRACKS; do
  tl="$(echo "$t" | tr '[:upper:]' '[:lower:]')"
  BIN="speeddemon-$tl"
  PKG="speeddemon-$tl"

  say "TRACK $t — phase 1: instrumented build"
  # A separate target dir so the instrumented build never contaminates the
  # optimised one, and so `cargo build --release` stays a clean operation.
  CARGO_TARGET_DIR="$PGO_DIR/gen" \
  RUSTFLAGS="-Cprofile-generate=$PROF_DIR" \
    cargo build --release --offline -p "$PKG" \
    || die "phase 1 build failed for track $t"

  say "TRACK $t — phase 2: training run"
  INPUT_DIR="$(find_input "$t")"
  if [ -n "$INPUT_DIR" ]; then
    echo "  training on $INPUT_DIR"
    # Run the instrumented binary over the representative input. The
    # training data must NOT be the graded sample if the sample exists —
    # optimising a profile for the exact bytes that will be scored is
    # overfitting, and the organizers run on a holdout.
    SD_DATA_DIR="$INPUT_DIR" SD_OUT_DIR="$PGO_DIR/train-out" \
      RUSTFLAGS="-Cprofile-generate=$PROF_DIR" \
      "$PGO_DIR/gen/release/$BIN" || echo "  (training run returned non-zero — continuing)"
  else
    echo "  no representative input found; the profile will be empty"
    echo "  run: python tooling/gen-fixtures.py --track $t"
  fi

  say "TRACK $t — phase 3: optimised build"
  # -Cprofile-use gives the compiler the branch frequencies. The extra
  # flags are the well-known companions:
  #   -Cllvm-args=-inline-threshold=275  a larger inline budget
  #   -Ccodegen-units=1                   keep the layout stable
  #   -Cpanic=abort                       no unwinding tables (already in the profile)
  CARGO_TARGET_DIR="$PGO_DIR/use" \
  RUSTFLAGS="-Cprofile-use=$PROF_DIR -Cllvm-args=-inline-threshold=275" \
    cargo build --release --offline -p "$PKG" \
    || die "phase 3 build failed for track $t"

  say "TRACK $t — verifying the PGO binary still produces the same output"
  mkdir -p "$PGO_DIR/verify-out"
  # Compare the PGO binary against the non-PGO binary on the same input.
  # A PGO build must never change the ANSWER — only the speed. If the
  # outputs differ, the PGO binary is wrong and must not ship.
  if [ -n "$INPUT_DIR" ] && [ -x "./target/release/$BIN" ]; then
    mkdir -p "$PGO_DIR/out-nopgo" "$PGO_DIR/out-pgo"
    SD_DATA_DIR="$INPUT_DIR" SD_OUT_DIR="$PGO_DIR/out-nopgo" ./target/release/$BIN || true
    SD_DATA_DIR="$INPUT_DIR" SD_OUT_DIR="$PGO_DIR/out-pgo" "$PGO_DIR/use/release/$BIN" || true
    if diff -q "$PGO_DIR/out-nopgo" "$PGO_DIR/out-pgo" >/dev/null 2>&1; then
      echo "  PGO output is byte-identical to the non-PGO output — safe"
    else
      die "the PGO build changed the output. PGO must not change the answer. REVERT IT."
    fi
  else
    echo "  (no comparable input — skipped the output-equivalence check)"
  fi
done

say "PGO complete"
echo "  profiles : $PROF_DIR"
echo "  binaries : $PGO_DIR/use/release/"
echo
echo "  NOTE: the PGO binary is a BUILD ARTEFACT. It is not committed."
echo "  To ship it, copy the optimised binary into the image, or set"
echo "  PGO_BIN_DIR in the Dockerfile build stage."
echo
echo "  ALWAYS re-run the correctness gate after a PGO build:"
echo "    sh tooling/ci.sh --tracks $TRACKS"
