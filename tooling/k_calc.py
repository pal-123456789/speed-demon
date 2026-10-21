#!/usr/bin/env python3
"""
Speed Demon — efficiency coefficient (K) calculator.

    K = (Tref/T)^0.40 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15 x (Xref/X)^0.10

Every ratio is capped at 50x. The exponents sum to exactly 1.0, so K is a
weighted geometric mean of the five ratios: K = 6 means "six times the
reference, all things considered".

    speed_score = 100 * ln(K) / ln(25), clamped to [0, 100]

Track E exception: the M slot holds COMPRESSED SIZE (a ratio of ratios),
not peak memory.

Usage
-----
    python tooling/k_calc.py --preset nightshade
    python tooling/k_calc.py --t 31.45 --m 14 --s 0.59 --d 1 --x 1.06
    python tooling/k_calc.py --t 30 --m 15 --s 0.62 --d 1 --x 1.05 --json
    python tooling/k_calc.py --table            # what each axis is worth
"""

import argparse
import json
import math
import sys

WEIGHTS = {"t": 0.40, "m": 0.20, "s": 0.15, "d": 0.15, "x": 0.10}
CAP = 50.0
LN25 = math.log(25.0)

# The worked example printed on the event page (Team Nightshade, Track A).
NIGHTSHADE = {"t": 31.4504, "m": 14.0, "s": 0.5889, "d": 1.0, "x": 1.0612}

# The K -> points curve published with the rules.
CURVE = [1.0, 2.0, 3.5, 5.0, 8.0, 12.0, 18.0, 25.0]
CURVE_PTS = [0.0, 21.5, 38.9, 50.0, 64.6, 77.2, 89.8, 100.0]


def speed_score(k: float) -> float:
    """100 * ln(K)/ln(25), clamped to [0, 100]."""
    if k <= 0:
        return 0.0
    return max(0.0, min(100.0, 100.0 * math.log(k) / LN25))


def coefficient(ratios: dict) -> dict:
    """Compute K and the per-axis contributions from raw ratios."""
    capped = {k: min(CAP, max(0.0, v)) for k, v in ratios.items()}
    contribs = {k: capped[k] ** WEIGHTS[k] for k in WEIGHTS}
    k = 1.0
    running = []
    for axis in ("t", "m", "s", "d", "x"):
        k *= contribs[axis]
        running.append((axis, k))
    return {
        "K": k,
        "capped": capped,
        "contributions": contribs,
        "running_product": running,
        "speed_score": speed_score(k),
    }


def track_total(k: float, judge_score: float = 82.0) -> dict:
    """Track total = 65% machine (speed score) + 35% judges."""
    sp = speed_score(k)
    return {
        "speed_score": sp,
        "judge_score": judge_score,
        "total": 0.65 * sp + 0.35 * judge_score,
    }


def geometric_mean(totals: list) -> float:
    """Overall title is the geometric mean of track totals, not the average."""
    if not totals:
        return 0.0
    return math.exp(sum(math.log(max(t, 1e-9)) for t in totals) / len(totals))


def print_table() -> None:
    print("Efficiency coefficient: K = (Tref/T)^0.40 x (Mref/M)^0.20 x "
          "(Sref/S)^0.15 x (Dref/D)^0.15 x (Xref/X)^0.10")
    print("Every ratio is capped at 50x. Exponents sum to 1.0.\n")
    print("  AXIS                WEIGHT   2x gives   10x gives  100x gives")
    for axis, label in (("t", "T  wall time"), ("m", "M  peak memory"),
                        ("s", "S  source size"), ("d", "D  dependencies"),
                        ("x", "X  scaling")):
        w = WEIGHTS[axis]
        print(f"  {label:<18}  {w:.2f}     {2**w:6.3f}   {10**w:7.3f}   {100**w:7.3f}")
    print("\n  K -> speed score: 100 * ln(K)/ln(25), clamped to [0,100]")
    print("  AXIS  " + "".join(f"{k:>8}" for k in CURVE))
    print("  SCORE " + "".join(f"{p:>8.1f}" for p in CURVE_PTS))
    print("\n  Realistic per-track ceiling (T and M both near the 50x cap):")
    best = coefficient({"t": 50, "m": 50, "s": 1.0, "d": 1.0, "x": 1.0})
    print(f"    K = {best['K']:.2f}  -> speed {best['speed_score']:.1f}/100")


def main() -> int:
    p = argparse.ArgumentParser(description="Speed Demon K calculator")
    p.add_argument("--t", type=float, help="T ratio  (Tref/T)")
    p.add_argument("--m", type=float, help="M ratio  (Mref/M)  [compressed size on Track E]")
    p.add_argument("--s", type=float, help="S ratio  (Sref/S)")
    p.add_argument("--d", type=float, help="D ratio  (Dref/D)")
    p.add_argument("--x", type=float, help="X ratio  (Xref/X)")
    p.add_argument("--judge", type=float, default=82.0, help="judge score 0-100 (default 82)")
    p.add_argument("--preset", choices=["nightshade"], help="load a published example")
    p.add_argument("--table", action="store_true", help="print the scoring tables and exit")
    p.add_argument("--json", action="store_true", help="emit JSON")
    p.add_argument("--geo", type=float, nargs="*", help="geometric mean of these track totals")
    args = p.parse_args()

    if args.table:
        print_table()
        return 0

    ratios = dict(NIGHTSHADE) if args.preset else {}
    for axis in ("t", "m", "s", "d", "x"):
        v = getattr(args, axis)
        if v is not None:
            ratios[axis] = v

    if not ratios:
        p.print_help()
        print("\nGive --preset nightshade or all five ratios.", file=sys.stderr)
        return 2

    missing = [a for a in ("t", "m", "s", "d", "x") if a not in ratios]
    if missing:
        print(f"Missing axes: {', '.join(missing)}", file=sys.stderr)
        return 2

    res = coefficient(ratios)
    tot = track_total(res["K"], args.judge)

    if args.json:
        out = {
            "ratios": ratios,
            "capped": res["capped"],
            "contributions": res["contributions"],
            "K": res["K"],
            "speed_score": tot["speed_score"],
            "judge_score": tot["judge_score"],
            "track_total": tot["total"],
        }
        if args.geo:
            out["geometric_mean_of_totals"] = geometric_mean(args.geo)
        print(json.dumps(out, indent=2))
        return 0

    print("\nEFFICIENCY COEFFICIENT")
    print("-" * 62)
    print(f"{'AXIS':<18}{'RATIO':>12}{'WEIGHT':>9}{'CONTRIBUTION':>16}")
    for axis, label in (("t", "T  wall time"), ("m", "M  peak memory"),
                        ("s", "S  source size"), ("d", "D  dependencies"),
                        ("x", "X  scaling")):
        w = WEIGHTS[axis]
        r = res["capped"][axis]
        raw = ratios[axis]
        note = " (capped)" if raw > CAP else ""
        print(f"{label:<18}{r:>9.4f}x{note:<3}{w:>9.2f}{res['contributions'][axis]:>16.4f}")
    print("-" * 62)
    for axis, running in res["running_product"]:
        print(f"  running product after {axis.upper()}: {running:.4f}")
    print("-" * 62)
    print(f"  K               = {res['K']:.4f}")
    print(f"  speed score     = {tot['speed_score']:.2f} / 100")
    print(f"  judge score     = {tot['judge_score']:.2f} / 100")
    print(f"  TRACK TOTAL     = {tot['total']:.2f} / 100")
    if args.geo:
        print(f"  geometric mean of {args.geo} = {geometric_mean(args.geo):.4f}")
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
