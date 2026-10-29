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
# The organizers' reference axes, EXACT (harness/calibration-seed1.json and
# harness/reference_axes.json). m_ref/s_ref were None for B/C/D while the
# organizers had not published them; a None here SILENTLY drops that ratio from
# K, so every scoreboard row for B/C/D was optimistic until this was filled in.
# tooling/check-consistency.py now cross-checks all three tables against
# verification/contract.toml.
REFS = {
    "A": {"t_ref": 32.047276, "m_ref": 227.98336, "s_ref": 56, "d_ref": 1.0},
    "B": {"t_ref": 56.352376, "m_ref": 2199.9616, "s_ref": 91, "d_ref": 1.0},
    "C": {"t_ref": 14.444059, "m_ref": 367.435776, "s_ref": 108, "d_ref": 1.0},
    "D": {"t_ref": 72.692655, "m_ref": 892.915712, "s_ref": 43, "d_ref": 1.0},
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
        try:
            out[t.upper()] = float(val)
        except ValueError:
            hint = ""
            if any(c in val for c in "<>"):
                hint = ("  (that looks like a placeholder from a doc example — "
                        "put your measured number, e.g. a=1.07)")
            raise SystemExit(f"--{t} expects a number, got {val!r}{hint}")
    return out


def scoreboard(t_abs: dict, m_abs: dict, lines: dict, judge: float = 82.0) -> dict:
    """P9 rung-to-rung scoreboard: measured absolutes -> per-track K/speed/
    total + the best-3 geometric mean (the overall-title driver)."""
    present = [x for x in TRACKS if x in t_abs]
    if not present:
        raise SystemExit("--scoreboard needs at least one track, e.g. --t a=1.07")
    rows = {}
    for t in present:
        refs = REFS[t]
        r = ratios_from_abs(t_abs[t], m_abs.get(t, 1.0), lines.get(t, 1.0), refs)
        partial = False
        if not refs.get("m_ref") or not refs.get("s_ref"):
            partial = True
            # M/S references are DEFERRED until Oct 28: hold those axes at 1.0
            # so K is a T-x-D projection (optimistic) and gets real after the
            # freeze pins the refs.
            r.setdefault("m", 1.0)
            r.setdefault("s", 1.0)
        res = coefficient(r)
        tot = track_total(res["K"], judge)
        rows[t] = {
            "t_abs": t_abs[t], "m_abs": m_abs.get(t), "lines": lines.get(t),
            "ratios": r, "K": res["K"], "speed": tot["speed_score"],
            "total": tot["total"], "partial": partial,
        }
    totals = [rows[t]["total"] for t in present]
    best3 = sorted(totals, reverse=True)[:3]
    rows["_best3_geomean"] = math.exp(sum(math.log(max(x, 1e-9)) for x in best3) / 3)
    rows["_best3"] = best3
    return rows


def main() -> int:
    p = argparse.ArgumentParser(description="Speed Demon what-if K explorer")
    p.add_argument("--track", choices=[*REFS, *[k.lower() for k in REFS]],
                   help="track preset, upper or lower case")
    p.add_argument("--t-abs", type=float, help="our absolute T in seconds")
    p.add_argument("--m-abs", type=float, help="our absolute M in MB")
    p.add_argument("--lines", type=float, help="our S code-line count")
    p.add_argument("--ratios", type=float, nargs=4, metavar=("T", "M", "S", "D"),
                   help="raw ratios, space separated (use 1.0 for unknown D)")
    p.add_argument("--pct", type=float, default=0.10, help="improvement to test per axis (default 0.10)")
    p.add_argument("--judge", type=float, default=82.0, help="judge score 0-100 (default 82)")
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
    p.add_argument("--scoreboard", action="store_true", help="measured absolutes -> per-track K/speed/total + best-3 geomean")
    p.add_argument("--t", nargs="+", metavar="T=V", help="measured T seconds per track, e.g. A=1.07 B=1.88 (--scoreboard)")
    p.add_argument("--m", nargs="+", metavar="T=V", help="measured M MB per track (--scoreboard)")
    p.add_argument("--l", nargs="+", metavar="T=V", help="measured code lines per track (--scoreboard)")
    p.add_argument("--tradeoff", action="store_true",
                   help="price a change: +N lines for -X%% T (PGO, radix select, any rung)")
    p.add_argument("--add-lines", type=float, default=0.0, help="lines the change adds (--tradeoff)")
    p.add_argument("--t-gain", type=float, default=0.0, help="%% wall-time the change saves (--tradeoff)")
    p.add_argument("--m-gain", type=float, default=0.0, help="%% peak-memory the change saves (--tradeoff)")
    args = p.parse_args()

    if args.tradeoff:
        tr = (args.track or "A").upper()
        if not args.t_abs or not args.lines:
            p.error("--tradeoff needs --t-abs, --lines (and optionally --m-abs, --track)")
        refs = REFS[tr]
        before = ratios_from_abs(args.t_abs, args.m_abs or 1.0, args.lines, refs)
        t_after = args.t_abs * (1.0 - args.t_gain / 100.0)
        m_after = (args.m_abs or 1.0) * (1.0 - args.m_gain / 100.0)
        after = ratios_from_abs(t_after, m_after, args.lines + args.add_lines, refs)
        for r in (before, after):
            r.setdefault("m", 1.0)
            r.setdefault("s", 1.0)
        b, a = coefficient(before), coefficient(after)
        bt, at = track_total(b["K"], args.judge), track_total(a["K"], args.judge)
        if args.json:
            print(json.dumps({
                "before": {"K": b["K"], "speed": bt["speed_score"], "total": bt["total"]},
                "after": {"K": a["K"], "speed": at["speed_score"], "total": at["total"]},
                "d_speed": at["speed_score"] - bt["speed_score"],
                "d_total": at["total"] - bt["total"],
            }, indent=2))
            return 0
        print(f"\nTRADE-OFF PRICE — track {tr}: +{args.add_lines:.0f} lines, "
              f"-{args.t_gain:.1f}% T, -{args.m_gain:.1f}% M")
        print("=" * 66)
        print(f"  {'':<10}{'K':>9}{'speed':>9}{'total':>9}")
        print(f"  {'before':<10}{b['K']:>9.3f}{bt['speed_score']:>9.2f}{bt['total']:>9.2f}")
        print(f"  {'after':<10}{a['K']:>9.3f}{at['speed_score']:>9.2f}{at['total']:>9.2f}")
        print(f"  {'delta':<10}{'':>9}{(at['speed_score'] - bt['speed_score']):>+9.2f}"
              f"{(at['total'] - bt['total']):>+9.2f}")
        verdict = "ADOPT" if (at["total"] - bt["total"]) > 0 else "REJECT"
        print(f"\n  verdict: {verdict}  (T carries 0.50, S only 0.15 and capped)")
        print("  Note: S is capped at 1.0 — once you are above the reference line")
        print("  count, extra lines always cost; the question is only whether the")
        print("  measured T gain outruns them.")
        print()
        return 0

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

    if args.scoreboard:
        if not args.t:
            p.error("--scoreboard needs --t 'T=seconds ...'")
        t_abs = _parse_pairs(args.t)
        m_abs = _parse_pairs(args.m) if args.m else {}
        lines = _parse_pairs(args.l) if args.l else {}
        sb = scoreboard(t_abs, m_abs, lines, args.judge)
        if args.json:
            out = {t: sb[t] for t in TRACKS if t in sb}
            out["best3_geomean"] = sb["_best3_geomean"]
            print(json.dumps(out, indent=2))
            return 0
        print("\nRUNG-TO-RUNG SCOREBOARD (P9; judge score = "
              f"{args.judge:.0f}, D = 1.00)")
        print("=" * 78)
        print(f"{'trk':<4}{'T s':>7}{'M MB':>7}{'lines':>7}{'T/M/S ratios':>22}"
              f"{'K':>7}{'speed':>7}{'total':>7}")
        present = [x for x in TRACKS if x in sb]
        for t in present:
            r = sb[t]
            rt = r["ratios"]
            ratio_txt = (f"{rt.get('t', 0):.1f}/{rt.get('m', 0):.1f}/"
                         f"{rt.get('s', 0):.2f}")
            m_txt = "-" if r["m_abs"] is None else f"{r['m_abs']:.0f}"
            l_txt = "-" if r["lines"] is None else f"{r['lines']:.0f}"
            print(f"{t:<4}{r['t_abs']:>7.3f}{m_txt:>7}{l_txt:>7}"
                  f"{ratio_txt:>22}{r['K']:>7.2f}{r['speed']:>7.1f}"
                  f"{r['total']:>7.2f}")
        print("-" * 78)
        n = len(present)
        label = "best-3" if n >= 3 else f"best-{n} (partial — the overall title needs 3)"
        print(f"  {label} geometric mean of totals: {sb['_best3_geomean']:.2f}")
        print("  elasticity reminders: 1% less T ≈ +0.11 speed pts; halving M "
              "≈ +3.0; 10% more lines must save ≥ 3% time.")
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

    refs = REFS.get((args.track or "A").upper(), REFS["A"])
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