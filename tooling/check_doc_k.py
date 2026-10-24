"""Verify every K value quoted in the project's documentation.

The K formula and its weights are the single most-referenced numbers in this
project: every strategy decision is expressed as "this is worth X points of
K". A wrong K in a doc propagates into a plan, and from the plan into code.
So the arithmetic in the docs is treated as testable code.

Targets: any markdown table row of the form
    | name | T | M | S | D | X | K | speed |
where T/M/S/D/X are ratios (ref/ours) and K/speed are the claimed results.
"""

import io
import re
import sys

WEIGHTS = {"T": 0.40, "M": 0.20, "S": 0.15, "D": 0.15, "X": 0.10}
K_ANCHOR = 25.0
CAP = 50.0


def k_of(ratios):
    k = 1.0
    for axis, w in WEIGHTS.items():
        r = ratios[axis]
        if r is None:
            return None
        k *= min(r, CAP) ** w
    return k


def speed_of(k):
    s = 100.0 * __import__("math").log(k) / __import__("math").log(K_ANCHOR)
    return max(0.0, min(100.0, s))


NUM = re.compile(r"^-?\d+(?:\.\d+)?$")


def parse_row(cells):
    """Return (label, ratios dict, claimed_k, claimed_speed) or None."""
    if len(cells) < 8:
        return None
    label = cells[0]
    # The five axis cells are 1..5, then K, then speed.
    axis_cells = cells[1:6]
    ratios = {}
    for axis, cell in zip("TMSDX", axis_cells):
        raw = cell.replace("*", "").replace("\u00d7", "").rstrip("x").strip()
        if not NUM.match(raw):
            return None
        ratios[axis] = float(raw)
    kcell = cells[6].replace("*", "").strip()
    scell = cells[7].replace("*", "").strip()
    if not NUM.match(kcell) or not NUM.match(scell):
        return None
    return label, ratios, float(kcell), float(scell)


def main(paths):
    bad = 0
    checked = 0
    for path in paths:
        try:
            text = io.open(path, encoding="utf-8").read()
        except OSError:
            continue
        for lineno, line in enumerate(text.splitlines(), 1):
            if not line.lstrip().startswith("|"):
                continue
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            parsed = parse_row(cells)
            if not parsed:
                continue
            label, ratios, claimed_k, claimed_s = parsed
            computed = k_of(ratios)
            if computed is None:
                continue
            computed_s = speed_of(computed)
            checked += 1
            k_ok = abs(computed - claimed_k) <= max(0.02, claimed_k * 0.002)
            s_ok = abs(computed_s - claimed_s) <= 0.15
            if not (k_ok and s_ok):
                bad += 1
                print(f"  MISMATCH {path}:{lineno}")
                print(f"    row      : {label}")
                print(f"    ratios   : {ratios}")
                print(f"    claimed  : K={claimed_k} speed={claimed_s}")
                print(f"    computed : K={computed:.4f} speed={computed_s:.2f}")
    print(f"\n  checked {checked} K row(s), {bad} mismatch(es)")
    return 1 if bad else 0


if __name__ == "__main__":
    args = sys.argv[1:]
    if not args:
        args = [
            "docs/CAMPAIGN-STATUS.md",
            "docs/MASTER-PLAN.md",
            "docs/specs/track-a-ingest.md",
            "docs/specs/track-b-index.md",
            "docs/specs/track-c-join.md",
            "docs/specs/track-d-traverse.md",
            "docs/specs/track-e-compress.md",
            "docs/track-A-ingest.md",
            "docs/track-B-index.md",
            "docs/track-C-join.md",
            "docs/track-D-traverse.md",
            "docs/track-E-compress.md",
            "README.md",
        ]
    sys.exit(main(args))
