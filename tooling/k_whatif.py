#!/usr/bin/env python3
"""
Speed Demon — what-if K explorer: where the next hour buys the most points.

The scoring formula is the team's one genuine compass during the 72 hours:
K = (Tref/T)^0.50 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15, speed = 50*log10(K).
This tool answers the question the formula raises: if I shave 10% off T, what
do I gain? Compare that to 10% off M, or 10% fewer lines. Rank the axes.

Usage:
    python tooling/k_whatif.py --track A --t-abs 1.2 --m-abs 30          # absolute numbers
    python tooling/k_whatif.py --track A --t-abs 1.2 --m-abs 30 --lines 180
    python tooling/k_whatif.py --ratios 25 5.7 0.51 1.0                  # raw ratios
    python tooling/k_whatif.py --selfcheck                               # pin to the published example

The sensitivity view ("if this axis improves by 10% / 20%") is the point: it
ranks the four axes by points-per-percent so the hour is spent on the axis
with the biggest exponent payoff at the CURRENT operating point.

Exit: 0 always (except --selfcheck, which fails on drift).
"""

import argparse
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from k_calc import (  # noqa: E402  (reuse the single source of truth)
    AXES,
    CAPS,
    WEIGHTS,
    NIGHTSHADE,
    coefficient,
    speed_score,
    track_total,
)

# Reference values from the events page (measured, 5 runs each). M/S_ref for
# B/C/D are still DEFERRED at Oct 28 — supply them or use --ratios.
REFS = {
    "A": {"t_ref": 32.05, "m_ref": 228.0, "s_ref": 56, "d_ref": 1.0},
    "B": {"t_ref": 56.35, "m_ref": None, "s_ref": None, "d_ref": 1.0},
    "C": {"t_ref": 14.44, "m_ref": None, "s_ref": None, "d_ref": 1.0},
    "D": {"t_ref": 72.69, "m_ref": None, "s_ref": None, "d_ref": 1.0},
}


def ratios_from_abs(t: float, m: float, s_lines: float, refs: dict) -> dict:
    """Absolute numbers → ratios. s_lines is OUR code lines (S = ref/ours)."""
    r = {}
    r["t"] = refs["t_ref"] / t if t else 0.0
    if refs.get("m_ref"):
        r["m"] = refs["m_ref"] / m if m else 0.0
    if refs.get("s_ref"):
        r["s"] = refs["s_ref"] / s_lines if s_lines else 0.0
    r["d"] = refs["d_ref"] / 1.0  # D = 1 + deps; we ship zero deps
    return r


def sensitivity(ratios: dict, pct: float) -> dict:
    """What K and speed do if EACH axis alone improves by pct (10% = 0.10)."""
    out = {}
    base = coefficient(ratios)
    base_speed = base["speed_score"]
    for axis in AXES:
        v = ratios[axis]
        # An improvement lowers T/M/S absolute numbers → raises the ratio.
        improved = v * (1.0 + pct)
        trial = dict(ratios)
        trial[axis] = improved
        res = coefficient(trial)
        out[axis] = {
            "ratio_from": v,
            "ratio_to": improved,
            "k": res["K"],
            "speed": res["speed_score"],
            "speed_gain": res["speed_score"] - base_speed,
        }
    return out


def main() -> int:
    p = argparse.ArgumentParser(description="Speed Demon what-if K explorer")
    p.add_argument("--track", choices=list(REFS), help="track preset (M/S_ref DEFERRED for B/C/D)")
    p.add_argument("--t-abs", type=float, help="our absolute T in seconds")
    p.add_argument("--m-abs", type=float, help="our absolute M in MB")
    p.add_argument("--lines", type=float, help="our S code-line count")
    p.add_argument("--ratios", type=float, nargs=4, metavar=("T", "M", "S", "D"),
                   help="raw ratios, space separated (use 1.0 for unknown D)")
    p.add_argument("--pct", type=float, default=0.10, help="improvement to test per axis (default 0.10)")
    p.add_argument("--json", action="store_true", help="emit JSON")
    p.add_argument("--selfcheck", action="store_true", help="assert the Nightshade example still reproduces K=6.41, speed=40.4")
    args = p.parse_args()

    if args.selfcheck:
        res = coefficient(dict(NIGHTSHADE))
        ok_k = abs(res["K"] - 6.4136) < 0.01
        ok_s = abs(res["speed_score"] - 40.36) < 0.05
        if ok_k and ok_s:
            print(f"selfcheck: PASS  K={res['K']:.4f}  speed={res['speed_score']:.2f}")
            return 0
        print(f"selfcheck: FAIL  K={res['K']:.4f} (want ~6.41)  speed={res['speed_score']:.2f} (want ~40.4)",
              file=sys.stderr)
        return 1

    refs = REFS.get(args.track) if args.track else REFS["A"]
    if args.ratios:
        ratios = dict(zip(AXES, args.ratios))
    else:
        if not args.t_abs:
            p.error("need --t-abs (absolute seconds) or --ratios")
        ratios = ratios_from_abs(args.t_abs, args.m_abs or 1.0, args.lines or 1.0, refs)
        if not refs.get("m_ref"):
            print("note: M/S reference DEFERRED for this track — supply --m-abs/--lines or --ratios", file=sys.stderr)

    base = coefficient(ratios)
    tot = track_total(base["K"])
    sens = sensitivity(ratios, args.pct)
    ranked = sorted(sens.items(), key=lambda kv: kv[1]["speed_gain"], reverse=True)

    if args.json:
        print(json.dumps({
            "ratios": ratios,
            "base": {"K": base["K"], "speed": tot["speed_score"], "track_total": tot["total"]},
            "sensitivity": {a: v for a, v in sens.items()},
            "ranked_by_gain": [a for a, _ in ranked],
        }, indent=2))
        return 0

    print("\nWHAT-IF K EXPLORER")
    print("=" * 64)
    print(f"  base ratios  T {ratios['t']:.3f}  M {ratios['m']:.3f}  S {ratios['s']:.3f}  D {ratios['d']:.3f}")
    print(f"  K = {base['K']:.4f}   speed = {tot['speed_score']:.2f}/100   track total = {tot['total']:.2f}")
    print(f"\n  {args.pct*100:.0f}% improvement on ONE axis alone (others unchanged):")
    print(f"  {'AXIS':<10}{'K after':>12}{'speed after':>14}{'gain':>9}{'rank':>7}")
    for axis, v in ranked:
        print(f"  {axis.upper():<10}{v['k']:>12.4f}{v['speed']:>13.2f}{v['speed_gain']:>+8.2f}{'':>5}#{ranked.index((axis, v))+1}")
    print(f"\n  Verdict: the axis ranked #1 is where the next {args.pct*100:.0f}% pays most.")
    print("  (Don't forget: T is uncapped; M caps at 50x; S and D can only cost.)")
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())