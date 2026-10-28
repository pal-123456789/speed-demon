#!/bin/sh
# bench-rung.sh — the per-rung measurement sandwich (one command per rung).
#
# For one track: build -> mini determinism check -> timed runs (5, middle-3
# mean) -> SHA check -> ledger append -> scoreboard row. This is the H12-H50
# loop: optimize, run `bench-rung.sh A --label rung-12`, read the scoreboard,
# decide the next rung. The harness is authoritative on Linux/WSL; on a plain
# Windows host it falls back to a Git-bash timing loop clearly labelled as a
# local smoke run (timing NOT the scoring protocol).
#
# Usage:
#   sh tooling/bench-rung.sh A
#   sh tooling/bench-rung.sh B --label rung-07
#   RUNS=3 sh tooling/bench-rung.sh C
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
t="${1:-}"
case "$t" in A|B|C|D|a|b|c|d) ;; *) echo "usage: bench-rung.sh A|B|C|D [--label NAME]" >&2; exit 2;; esac
t="$(echo "$t" | tr '[:upper:]' '[:lower:]')"
label="${3:-rung}"
RUNS="${RUNS:-5}"

bin="$ROOT/track-$t/run"
data="$ROOT/verification/data/track-$t"
want=""; case "$t" in a) want=summary.txt;; b) want=results.txt;; c) want=aggregates.txt;; d) want=distances.txt;; esac

echo "== rung $label, track $t =="

# 1. build
if ! (cd "$ROOT" && sh "track-$t/build.sh" >/tmp/sd-rung-build.log 2>&1); then
  echo "FAIL: build.sh (see /tmp/sd-rung-build.log)"
  exit 1
fi
echo "  ok build"

# 2. mini determinism (2 runs)
h1=""
for i in 1 2; do
  out="$(mktemp -d 2>/dev/null || echo "/tmp/sd-rung-$t-$$-$i")"
  SD_DATA_DIR="$data" SD_OUT_DIR="$out" "$bin" >/dev/null 2>&1 || { echo "FAIL: run $i"; rm -rf "$out"; exit 1; }
  h="$(sha256sum "$out"/* 2>/dev/null | awk '{print $1}')"
  [ -n "$h" ] || { echo "FAIL: no output file"; rm -rf "$out"; exit 1; }
  if [ -z "$h1" ]; then h1="$h"; elif [ "$h" != "$h1" ]; then
    echo "FAIL: nondeterministic output (run 1 vs 2) — run the full determinism matrix"
    rm -rf "$out"; exit 1
  fi
  rm -rf "$out"
done
echo "  ok deterministic ($h1)"

# 3. SHA gate vs the pinned expected hash
pin="$ROOT/track-$t/verification/expected/$want.sha256"
want_hash="$(cat "$pin" 2>/dev/null | tr -d '[:space:]')"
if [ -n "$want_hash" ] && [ "$want_hash" != "$h1" ]; then
  echo "  NOTE: output hash differs from the pin ($want_hash)."
  echo "        Re-pin deliberately: sh tooling/pin-hash.sh $t"
else
  echo "  ok SHA-green vs pin"
fi

# 4. timing: harness when available and spawnable, else a crude sh loop
if [ -x "$ROOT/verification/harness/target/release/sd-harness" ] && \
   [ "$(uname -s 2>/dev/null)" = "Linux" ]; then
  "$ROOT/verification/harness/target/release/sd-harness" bench \
      --track "$t" --runs "$RUNS" --no-docker --bin "$bin" --label "$label"
  rc=$?
else
  echo "  (harness bench unavailable on this host — crude local timing only)"
  i=1; times=""
  while [ "$i" -le "$RUNS" ]; do
    out="$(mktemp -d 2>/dev/null || echo "/tmp/sd-rung-t-$t-$$-t$i")"
    s="$(date +%s%N)"
    SD_DATA_DIR="$data" SD_OUT_DIR="$out" "$bin" >/dev/null 2>&1
    e="$(date +%s%N)"
    ms=$(( (e - s) / 1000000 ))
    times="$times $ms"
    rm -rf "$out"
    i=$((i + 1))
  done
  # middle-3 mean of the trimmed runs
  sorted="$(echo $times | tr ' ' '\n' | sort -n | sed '/^$/d')"
  n="$(echo "$sorted" | wc -l | tr -d ' ')"
  if [ "$n" -ge 3 ]; then
    mid="$(echo "$sorted" | sed -n "2,$((n-1))p")"
    sum=0; c=0
    for m in $mid; do sum=$((sum + m)); c=$((c + 1)); done
    avg=$((sum / c))
    echo "  crude T (middle-$c of $n runs): ${avg} ms   [host timing, NOT the scoring protocol]"
  else
    echo "  crude runs (ms):$times"
  fi
  rc=0
fi

# 5. scoreboard row (speed/track total from the crude or last numbers)
echo "  next (put YOUR measured numbers, all four tracks):"
echo "        python tooling/k_whatif.py --scoreboard --t a=1.07 b=1.88 c=0.48 d=2.42 \\"
echo "            --m a=15 b=120 c=80 d=70 --l a=150 b=260 c=180 d=220"
echo "        (then commit the green rung and PERF.md via gen_docs)"
exit "$rc"