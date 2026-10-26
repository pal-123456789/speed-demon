"""Verify every K value quoted in the project's documentation.

The K formula and its weights are the single most-referenced numbers in this
project: every strategy decision is expressed as "this is worth X points of
K". A wrong K in a doc propagates into a plan, and from the plan into code.
So the arithmetic in the docs is treated as testable code.

The frozen model has FOUR axes:
    K = (Tref/T)^0.50 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15
    speed_score = 50 * log10(K), clamped to [0, 100]
Wall time is uncapped, memory is capped at 50x, S and D are capped at 1.0.

Targets: any markdown table row of the form
    | name | T | M | S | D | K | speed |
where T/M/S/D are ratios (ref/ours) and K/speed are the claimed results.

A row that still carries a fifth axis (the removed X scaling axis) is
REPORTED as an error rather than quietly verified against the old formula —
a leftover five-axis table is exactly the defect this tool exists to catch.
"""

import io
import math
import re
import sys
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WEIGHTS = {"T": 0.50, "M": 0.20, "S": 0.15, "D": 0.15}
# Per-axis cap on the ratio. T has none; M is 50x; S and D are 1.0.
CAPS = {"T": None, "M": 50.0, "S": 1.0, "D": 1.0}


def k_of(ratios):
    k = 1.0
    for axis, w in WEIGHTS.items():
        r = ratios[axis]
        if r is None:
            return None
        cap = CAPS[axis]
        if cap is not None:
            r = min(r, cap)
        k *= max(0.0, r) ** w
    return k


def speed_of(k):
    if k <= 0:
        return 0.0
    return max(0.0, min(100.0, 50.0 * math.log10(k)))


NUM = re.compile(r"^-?\d+(?:\.\d+)?$")


def _clean(cell):
    return cell.replace("*", "").replace("\u00d7", "").rstrip("x").strip()


def parse_row(cells):
    """Return (label, ratios dict, claimed_k, claimed_speed) or None.

    A row with five numeric axis cells is a five-axis row: the X axis is gone,
    so it is reported separately instead of being verified.
    """
    if len(cells) < 7:
        return None
    label = cells[0]
    axis_cells = cells[1:5]
    ratios = {}
    for axis, cell in zip("TMSD", axis_cells):
        raw = _clean(cell)
        if not NUM.match(raw):
            return None
        ratios[axis] = float(raw)
    kcell = cells[5].replace("*", "").strip()
    scell = cells[6].replace("*", "").strip()
    if not NUM.match(kcell) or not NUM.match(scell):
        return None
    return label, ratios, float(kcell), float(scell)


def looks_five_axis(cells):
    """True when the row carries a fifth numeric axis cell (the old X axis)."""
    if len(cells) < 8:
        return False
    return all(NUM.match(_clean(c)) for c in cells[1:6])


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
            if looks_five_axis(cells):
                bad += 1
                print(f"  FIVE-AXIS ROW {path}:{lineno}")
                print(f"    row      : {cells[0]}")
                print("    the X (scaling) axis was removed before the freeze — "
                      "this row still has a fifth ratio")
                continue
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
            "docs/track-a-ingest.md",
            "docs/track-b-index.md",
            "docs/track-c-join.md",
            "docs/track-d-traverse.md",
            "README.md",
        ]
    sys.exit(main(args))
