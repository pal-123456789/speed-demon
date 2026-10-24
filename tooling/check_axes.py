#!/usr/bin/env python3
"""Assert the S and D axes against the rules announced before the Oct 28 freeze.

Reads the JSON emitted by `tooling/measure_axes.py --all --json` and checks
the properties that the axes are SUPPOSED to have, so a CI step cannot print
PASS over an axis that is actually wrong.

docs/RULE-CHANGES-oct28.md:

  * D counts direct dependencies declared in the manifest; one dependency is a
    factor of 0.90. This repository declares none, so D must be exactly 1.0 on
    every track.
  * S is counted in code lines, repo-wide, and its factor is capped at 1.0.
    The cap means being small earns nothing, so a ratio above 1.0 is not an
    error — but it must never be applied uncapped anywhere else.

Exit status: 0 all assertions hold, 1 an assertion failed, 2 the input was
unusable.
"""

from __future__ import annotations

import json
import sys

TRACKS = ("A", "B", "C", "D", "E")


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: check_axes.py <measure_axes.json>", file=sys.stderr)
        return 2
    try:
        with open(argv[1], "r", encoding="utf-8") as fh:
            data = json.load(fh)
    except (OSError, ValueError) as exc:
        print(f"check_axes: cannot read {argv[1]}: {exc}", file=sys.stderr)
        return 2

    if not isinstance(data, list) or not data:
        print("check_axes: expected a non-empty JSON list of track results", file=sys.stderr)
        return 2

    failures: list[str] = []
    seen: set[str] = set()

    for entry in data:
        track = entry.get("track", "?")
        seen.add(track)
        d = entry.get("D", {})
        s = entry.get("S", {})

        deps = d.get("declared_dependencies")
        dval = d.get("D")
        if deps is None or dval is None:
            failures.append(f"track {track}: the D measurement is missing fields")
            continue

        if deps != 0:
            failures.append(
                f"track {track}: {deps} declared dependencies — D would be "
                f"{dval:.4f}, not 1.0000. The rule is 0.90 per direct dependency."
            )
        elif abs(dval - 1.0) > 1e-12:
            failures.append(
                f"track {track}: zero dependencies but D = {dval:.4f}, expected 1.0000"
            )

        lines = s.get("repo_code_lines")
        if not lines:
            failures.append(f"track {track}: no repo code lines were measured")
        else:
            print(f"S track {track}: {lines} repo code lines "
                  f"(counter: {s.get('counter', '?')})")
        print(f"D track {track}: D = {dval:.4f}, {deps} direct dependencies")

        factor = entry.get("S_factor_capped")
        if factor is not None and factor > 1.0 + 1e-12:
            failures.append(
                f"track {track}: the capped S factor is {factor}, which is above "
                f"the 1.0 cap"
            )

    missing = [t for t in TRACKS if t not in seen]
    if missing:
        failures.append(f"no result for track(s) {', '.join(missing)}")

    if failures:
        print()
        for f in failures:
            print(f"check_axes: FAIL {f}", file=sys.stderr)
        return 1
    print("check_axes: every track is D = 1.0000 with zero declared dependencies")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))