#!/usr/bin/env python3
"""official-tests — run the organizers' own test suite against their artifacts.

`reference/official/` holds a byte-identical copy of the public harness repo
(tests, generators, reference, harness, specs). Its test suite is the
organizers' OWN verification: the spec worked examples byte-for-byte, the
hand-written edge cases, generator determinism, and the S/D measurement rules.

Running it locally proves our vendored copies are intact and that our reading
of every contract matches theirs. Two modes:

    --fast   (default) the spec-conformance classes only: worked examples and
             hand-written edge cases for all four tracks. Milliseconds.
    --full   the whole suite (58 tests, ~2 minutes; includes generator-scale
             and cross-check tests).

Usage:
    python tooling/official-tests.py
    python tooling/official-tests.py --full
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
OFFICIAL = ROOT / "reference" / "official"

FAST = [
    # the spec worked examples and hand-written edge cases, per track
    "tests.test_a_ingest.Reference",   # holds the spec worked example + edge cases
    "tests.test_b_index.WorkedExample",
    "tests.test_b_index.EdgeCases",
    "tests.test_c_join.HandWrittenCases",
    "tests.test_d_traverse.HandWrittenCases",
]
FULL = [
    "tests.test_a_ingest",
    "tests.test_b_index",
    "tests.test_c_join",
    "tests.test_d_traverse",
]


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--full", action="store_true", help="run every official test")
    args = ap.parse_args(argv)

    if not (OFFICIAL / "tests").is_dir():
        print(f"the official copy is missing at {OFFICIAL}", file=sys.stderr)
        return 2

    targets = FULL if args.full else FAST
    print(f"official test suite ({'full' if args.full else 'fast'}): "
          f"{len(targets)} target module/class(es)")
    p = subprocess.run([sys.executable, "-m", "unittest", *targets, "-v"],
                       cwd=OFFICIAL, capture_output=True, text=True)
    tail = (p.stderr or "").strip().splitlines()
    for line in tail[-14:]:
        print("  " + line)
    if p.returncode == 0:
        print("official-tests: PASS — the organizers' own suite agrees with our artifacts")
        return 0
    print("official-tests: FAILED", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())