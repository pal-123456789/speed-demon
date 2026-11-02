#!/bin/bash
# Run Track D's two verification harnesses, and require them to pass.
#
#   tooling/fuzz-d.py          differential fuzz: random graph.txt + queries.txt against a plain
#                              heapq Dijkstra, with weights at the heavy-weight side-list boundary
#                              and sizes straddling the 16,384-node part cut.
#   tooling/d-logic-check.py   a line-by-line port of the search loop diffed against plain
#                              Dijkstra: the nd < best pruning, the stale-entry skip, the
#                              no-in-edge shortcut and the smaller-side rule, none of which the
#                              corpus hash can isolate.
#
# The FUZZ runs entirely inside WSL, because D maps its input and the Windows build takes a shimmed
# fallback whose failures are not evidence about the track. One wsl invocation does build, copy and
# fuzz, so there is no cross-boundary binary check to go wrong.
#
# usage: check-d-harnesses.sh [TRIALS] [SEEDS]
set -u
cd "$(dirname "$0")/.." || exit 1
TRIALS=${1:-40}
SEEDS=${2:-3}
fails=0

echo "== D logic check (pure Python, ported search loop vs Dijkstra) =="
if python tooling/d-logic-check.py; then
  echo "   PASS"
else
  echo "   FAIL"
  fails=$((fails + 1))
fi

echo ""
echo "== D differential fuzz, $TRIALS trials x $SEEDS seeds, on the LINUX binary =="
if ! command -v wsl.exe >/dev/null 2>&1; then
  echo "   SKIP: no WSL, so the Linux binary cannot be built here"
  fails=$((fails + 1))
else
  for s in $(seq 1 "$SEEDS"); do
    out=$(wsl.exe -d Ubuntu -- bash -lc "
      cd /mnt/d/SpeedDemon-Campaign || exit 1
      rm -f track-d/run
      sh track-d/build.sh >/tmp/h-build.log 2>&1 || { echo BUILD_FAILED; exit 1; }
      sed 's/\r\$//' tooling/fuzz-d.py > /tmp/fuzz_d.py
      python3 /tmp/fuzz_d.py track-d/run $TRIALS $s 2>&1 | tail -1
    " 2>/dev/null | tr -d '\r' | tail -1)
    echo "   seed $s: $out"
    case "$out" in
      "$TRIALS/$TRIALS"*) ;;
      *) fails=$((fails + 1)) ;;
    esac
  done
fi

echo ""
if [ "$fails" -eq 0 ]; then
  echo "check-d-harnesses: both harnesses pass"
  exit 0
fi
echo "check-d-harnesses: $fails harness check(s) failed"
exit 1
