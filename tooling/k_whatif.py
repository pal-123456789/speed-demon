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
import itertools
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


TRACKS = ["A", "B", "C", "D"]


BETA = 4.892  # track points per doubling of T (0.65 * 50 * 0.5 * log10(2))


def waterfill(base: dict, doubling: dict, floor: dict, hours: float) -> dict:
    """Analytic KKT allocation (model digest §2).

    P_t(h) = P0_t + BETA * r_t * (1 - exp(-h / (H_t * r_t)))   (r = doublings left)
    marginal_t(h) = (BETA / H_t) * exp(-h / (H_t * r_t))
    KKT: marginal / P == lambda for every funded track -> bisect lambda so the
    funded hours sum to `hours`. The corner lemma says exactly one track gets
    zero hours; we try every 3-of-4 drop (plus the no-drop case) and keep the
    best geometric mean.
    """
    tracks = TRACKS

    def solve_lambda(funded, lam):
        h = {}
        for t in funded:
            H, r, P0 = doubling[t], floor[t], base[t]
            # u = exp(-h/(H r));  u * (BETA/H + lam*BETA*r) = lam * (P0 + BETA*r)
            denom = BETA / H + lam * BETA * r
            u = lam * (P0 + BETA * r) / denom
            if u >= 1.0 or u <= 0.0:
                h[t] = 0.0
            else:
                h[t] = -H * r * math.log(u)
        return h

    best = None
    subsets = list(itertools.combinations(tracks, 3)) + [tuple(tracks)]
    for funded in subsets:
        lo, hi = 1e-9, 1e6
        for _ in range(60):
            lam = (lo + hi) / 2
            h = solve_lambda(funded, lam)
            # h_t is decreasing in lambda: raise lam when we overshoot hours
            if sum(h.values()) > hours:
                lo = lam
            else:
                hi = lam
        h = solve_lambda(funded, (lo + hi) / 2)
        # renormalize the residual to exactly `hours` proportionally
        s = sum(h.values())
        if s > 0:
            h = {t: v * hours / s for t, v in h.items()}
        P = {}
        for t in funded:
            H, r, P0 = doubling[t], floor[t], base[t]
            d = r * (1.0 - math.exp(-h[t] / (H * r))) if h[t] > 0 else 0.0
            P[t] = P0 + BETA * d
        gm = math.exp(sum(math.log(max(P[t], 1e-9)) for t in funded) / len(funded))
        cand = {"subset": sorted(funded), "hours": h, "totals": P, "geomean": gm}
        if best is None or gm > best["geomean"]:
            best = cand
    if best is None:
        raise ValueError("waterfill: no tracks")
    best["drop"] = [t for t in tracks if t not in best["subset"]]
    return best


def dispatch(base: dict, pph: dict, hours: float, diminish: float = 0.97) -> dict:
    """Greedy hour allocation across the tracks, maximizing the geometric
    mean of the BEST THREE resulting speed scores (the overall title rule).

    Model: the h-th hour on track t yields pph[t] * diminish^h points
    (diminishing returns, r < 1). Tries every 3-of-4 subset, allocates
    greedily inside it, keeps the best subset. Clearly a heuristic — fill
    --base/--pph with the team's own estimates (k_whatif and the bench
    produce them).
    """
    best = None
    for subset in itertools.combinations(TRACKS, 3):
        hours_i = {t: 0.0 for t in TRACKS}
        remaining = hours
        # one "hour" can be fractional for finer granularity; h steps of 0.5
        step = 0.5
        while remaining > 1e-9:
            best_t, best_gain = None, -1.0
            for t in subset:
                gain = pph[t] * (diminish ** hours_i[t])
                if gain > best_gain:
                    best_gain, best_t = gain, t
            take = min(step, remaining)
            hours_i[best_t] += take
            remaining -= take
        speeds = {t: base[t] + sum(pph[t] * (diminish ** h) for h in
                                   range(0, int(hours_i[t] * 2), 2))
                  for t in subset}
        gm = math.exp(sum(math.log(max(s, 1e-9)) for s in speeds.values()) / 3)
        cand = {"subset": sorted(subset), "hours": hours_i, "speeds": speeds,
                "geomean": gm}
        if best is None or gm > best["geomean"]:
            best = cand
    if best is None:
        raise ValueError("dispatch: no tracks")
    best["drop"] = [t for t in TRACKS if t not in best["subset"]]
    return best


def _parse_pairs(values: list) -> dict:
    out = {}
    for v in values or []:
        if "=" not in v:
            raise ValueError(f"expected TRACK=value, got {v!r}")
        t, val = v.split("=", 1)
        out[t.upper()] = float(val)
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
    p.add_argument("--dispatch", action="store_true", help="allocate H hours across the tracks (best-3 rule)")
    p.add_argument("--waterfill", action="store_true", help="analytic KKT allocation (P8: base/doubling/floor)")
    p.add_argument("--hours", type=float, default=72.0, help="hours to allocate (--dispatch)")
    p.add_argument("--base", nargs="+", metavar="T=V", help="current speed score per track, e.g. A=35 B=30 C=20 D=12 (--dispatch) or current TOTAL points (--waterfill)")
    p.add_argument("--pph", nargs="+", metavar="T=V", help="points per hour per track, e.g. A=1.0 B=0.8 C=0.6 D=0.4 (--dispatch)")
    p.add_argument("--doubling", nargs="+", metavar="T=V", help="hours per T-doubling per track, e.g. A=6 B=5 C=9 D=8 (--waterfill)")
    p.add_argument("--floor", nargs="+", metavar="T=V", help="doublings left before the floor per track, e.g. A=4 B=4 C=6 D=6 (--waterfill; default 20)")
    p.add_argument("--diminish", type=float, default=0.97, help="returns decay per hour on the same track (default 0.97)")
    args = p.parse_args()

    if args.waterfill:
        if not args.base or not args.doubling:
            p.error("--waterfill needs --base 'T=V' (total points) and --doubling 'T=V' (hours per doubling)")
        base = _parse_pairs(args.base)
        doubling = _parse_pairs(args.doubling)
        floor = _parse_pairs(args.floor) if args.floor else {t: 20.0 for t in TRACKS}
        plan = waterfill(base, doubling, floor, args.hours)
        if args.json:
            print(json.dumps(plan, indent=2))
            return 0
        print("\n72h WATER-FILL (analytic KKT; P0 = TOTAL track points, "
              "beta = 4.892 pts/doubling)")
        print("=" * 70)
        for t in TRACKS:
            h = plan["hours"].get(t, 0.0)
            P = plan["totals"].get(t, base.get(t, 0.0))
            role = "DROPPED" if t in plan["drop"] else "COUNTED"
            print(f"  track {t}: {h:5.1f} h   total {base[t]:5.2f} -> {P:5.2f}   [{role}]")
        print(f"\n  best-3 geometric mean of totals: {plan['geomean']:.2f}")
        print(f"  drop candidate: {', '.join(plan['drop'])}")
        print("  (= the corner lemma: exactly one track gets zero hours;")
        print("    re-solve every 8h from MEASURED doubling/floors.)")
        print()
        return 0

    if args.dispatch:
        if not args.base or not args.pph:
            p.error("--dispatch needs --base 'T=V ...' and --pph 'T=V ...'")
        base = _parse_pairs(args.base)
        pph = _parse_pairs(args.pph)
        plan = dispatch(base, pph, args.hours, args.diminish)
        if args.json:
            print(json.dumps(plan, indent=2))
            return 0
        print("\n72h DISPATCH (best-3-of-4 geometric mean; heuristic model)")
        print("=" * 64)
        for t in TRACKS:
            h = plan["hours"][t]
            sp = base[t] + sum(pph[t] * (args.diminish ** k) for k in
                               range(0, int(h * 2), 2))
            role = "DROPPED" if t in plan["drop"] else "COUNTED"
            print(f"  track {t}: {h:5.1f} h   base {base[t]:5.1f} -> {sp:5.1f}   [{role}]")
        print(f"\n  best-3 geometric mean of speeds: {plan['geomean']:.2f}")
        print(f"  drop candidate: {', '.join(plan['drop'])}")
        print("  (= track total also adds 0.35×judge; this ranks the speed half.)")
        print()
        return 0

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