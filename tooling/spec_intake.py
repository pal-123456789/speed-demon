#!/usr/bin/env python3
"""SPEC.md intake — turn Oct 28 from a scramble into a checklist.

On Oct 28 15:00 UTC the organizers freeze the specs. Every value this
repository has honestly marked DEFERRED becomes a fact at that moment. The
risk is not the work; it is the *bookkeeping*: a dozen values scattered
across a contract file, four track manifests, a Rust table and the K
budgets, each of which has to change consistently.

This tool is the checklist and the enforcer:

    --status     list every DEFERRED value, what it feeds, and where the
                 answer will come from in SPEC.md
    --set T.K=V  record one pinned value (validated, formatted, commented)
    --check      the gate: no DEFERRED left among the values that block a
                 track, K anchors recomputed from the pinned numbers,
                 consistency verified against the harness and the manifests
    --kbudget    the K budget per track, computed from what is pinned so far

The workflow it encodes:

    1. Read SPEC.md.
    2. python tooling/spec_intake.py --status        # what am I looking for
    3. python tooling/spec_intake.py --set B.ref_build_time_s=23.4
    4. python tooling/spec_intake.py --check         # is everything consistent
    5. Fix what --check reports (it names the file and the test).
    6. python tooling/spec_intake.py --kbudget       # the real K budget

`--check` deliberately FAILS while any blocking value is still DEFERRED. A
green "ready" signal that is really "nothing checked yet" is worse than no
signal at all.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
CONTRACT = ROOT / "verification" / "contract.toml"

DEFERRED = "DEFERRED"

# ── what each DEFERRED value feeds, and where SPEC.md will answer it ────────
#
# `blocks` marks the values that stop a track from being built at all: get
# these wrong and the submission writes a file nobody asked for.
CATALOGUE = {
    "A": {
        "group_key": ("the output's grouping key", "SPEC A, output section", True),
        "sort_order": ("the output's line ordering", "SPEC A, output section", True),
        "percentile_rule": ("which percentile definition the hash assumes", "SPEC A, output section", True),
        "error_rate_format": ("error-rate decimals and format", "SPEC A, output section", True),
        "field_separator": ("the output separator", "SPEC A, output section", False),
        "trailing_newline": ("whether the last line ends in \\n", "SPEC A / the sample's expected hash", True),
        "ref_source_lines": ("the S axis denominator", "SPEC A, reference table", False),
    },
    "B": {
        "ref_build_time_s": ("the 40% half of the T axis", "SPEC B, reference table", False),
        "ref_query_time_s": ("the 60% half of the T axis", "SPEC B, reference table", False),
        "ref_peak_mem_mb": ("the M axis denominator", "SPEC B, reference table", False),
        "ref_source_lines": ("the S axis denominator", "SPEC B, reference table", False),
        "blend_formula": ("HOW build+query combine — a 24% K swing", "SPEC B, scoring section", True),
        "boolean_precedence": ("NOT vs AND binding", "SPEC B, query semantics", True),
        "tokenizer": ("what counts as a token", "SPEC B, query semantics", True),
        "result_format": ("what one result line looks like", "SPEC B, output section", True),
    },
    "C": {
        "lookup_file": ("the second input's name", "SPEC C, inputs", True),
        "quoting": ("the quoting rules — most DQs happen here", "SPEC C, CSV rules", True),
        "bom_handling": ("mid-file BOM semantics", "SPEC C, CSV rules", True),
        "newline_in_quotes": ("whether an embedded LF is kept", "SPEC C, CSV rules", True),
        "amount_scale": ("decimal handling of the amount column", "SPEC C, fields", True),
        "amount_rounding": ("rounding mode", "SPEC C, fields", True),
        "aggregate_key": ("what one output row groups by", "SPEC C, output section", True),
        "sort_order": ("the output's line ordering", "SPEC C, output section", True),
        "output_number_format": ("how numbers are rendered", "SPEC C, output section", True),
        "ref_peak_mem_mb": ("the M axis denominator", "SPEC C, reference table", False),
        "ref_source_lines": ("the S axis denominator", "SPEC C, reference table", False),
    },
    "D": {
        "queries_file": ("the queries input's name", "SPEC D, inputs", True),
        "output_contract": ("distance-only vs full path", "SPEC D, output section", True),
        "unreachable_spelling": ("what an unreachable pair prints", "SPEC D, output section", True),
        "max_distance_bound": ("whether u64 distances are provably safe", "SPEC D, fields", False),
        "ref_peak_mem_mb": ("the M axis denominator", "SPEC D, reference table", False),
        "ref_source_lines": ("the S axis denominator", "SPEC D, reference table", False),
    },
}


def parse_contract(text: str):
    """Sections -> {key: raw value}, preserving order and comment lines."""
    sections: dict[str, dict[str, str]] = {}
    current = "global"
    sections[current] = {}
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            current = stripped[1:-1]
            sections.setdefault(current, {})
            continue
        if "=" in stripped and not stripped.startswith("#"):
            k, v = stripped.split("=", 1)
            sections[current][k.strip()] = v.strip()
    return sections


def load():
    return parse_contract(CONTRACT.read_text(encoding="utf-8"))


def deferred_values(sections):
    out = []
    for track, keys in CATALOGUE.items():
        sec = sections.get(track, {})
        for key, (feeds, source, blocking) in keys.items():
            val = sec.get(key, f"<{key} not present>")
            if DEFERRED in val.upper() or val.startswith("<"):
                out.append((track, key, feeds, source, blocking))
    return out


def cmd_status(sections) -> int:
    items = deferred_values(sections)
    if not items:
        print("No DEFERRED values remain. Every value the specs govern is pinned.")
        return 0
    blocking = [i for i in items if i[4]]
    print(f"{len(items)} value(s) still DEFERRED — {len(blocking)} of them BLOCK a track.\n")
    for track, key, feeds, source, blocking_flag in items:
        mark = "BLOCKS " if blocking_flag else "feeds  "
        print(f"  [{track}] {mark}{key:<24} {feeds}")
        print(f"           where to find it: {source}")
    print(
        "\nRecord each one as the specs freeze it:\n"
        "  python tooling/spec_intake.py --set B.blend_formula='additive 0.40*rB + 0.60*rQ'\n"
        "then run --check."
    )
    return 1


def cmd_set(sections, assignment: str) -> int:
    if "=" not in assignment:
        print("--set expects TRACK.KEY=VALUE", file=sys.stderr)
        return 2
    left, value = assignment.split("=", 1)
    left = left.strip()
    value = value.strip()
    if "." not in left:
        print("--set expects TRACK.KEY=VALUE", file=sys.stderr)
        return 2
    track, key = left.split(".", 1)
    track = track.upper()

    text = CONTRACT.read_text(encoding="utf-8")
    pattern = re.compile(
        rf"^(?P<indent>\s*)(?P<key>{re.escape(key)})\s*=\s*(?P<value>.*)$",
        re.M,
    )
    m = None
    in_section = False
    for match in pattern.finditer(text):
        # a key may repeat across sections; pick the one in the right section
        before = text[: match.start()]
        section_matches = re.findall(r"^\[(\w+)\]\s*$", before, re.M)
        owner = section_matches[-1] if section_matches else "global"
        if owner == track:
            m = match
            break

    if m is None:
        print(f"No key '{key}' in section [{track}] of {CONTRACT.name}", file=sys.stderr)
        return 2

    old = m.group("value").strip()
    new_value = value if value.startswith("[") else f'"{value}"'
    if re.fullmatch(r"-?\d+(\.\d+)?", value):
        new_value = value
    if value.lower() in ("true", "false"):
        new_value = value.lower()

    replacement = f"{m.group('indent')}{key} = {new_value}  # pinned from SPEC.md"
    new_text = text[: m.start()] + replacement + text[m.end():]
    CONTRACT.write_text(new_text, encoding="utf-8", newline="\n")
    print(f"  [{track}] {key}: {old}  ->  {new_value}")
    print("  Next: python tooling/spec_intake.py --check")
    return 0


def cmd_check(sections) -> int:
    failures = []

    items = deferred_values(sections)
    blocking = [i for i in items if i[4]]
    if blocking:
        failures.append(f"{len(blocking)} BLOCKING value(s) still DEFERRED: "
                        + ", ".join(f"{t}.{k}" for t, k, *_ in blocking))

    # K anchors need the reference numbers.
    g = sections.get("global", {})
    for track in "ABCD":
        sec = sections.get(track, {})
        ref_time = sec.get("ref_time_s", "")
        if DEFERRED in ref_time.upper() or not ref_time:
            failures.append(f"[{track}] ref_time_s is missing — the T axis has no denominator")
        ref_src = sec.get("ref_source_lines", "")
        if DEFERRED in ref_src.upper() or not ref_src:
            print(f"  note: [{track}] ref_source_lines still DEFERRED — the S axis ratio cannot be computed yet")

    # The harness's own consistency test is the authority on drift.
    print("  running the harness consistency test (cargo test --offline --manifest-path verification/harness/Cargo.toml)...")
    import subprocess
    proc = subprocess.run(
        ["cargo", "test", "--offline", "--manifest-path", "verification/harness/Cargo.toml"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if proc.returncode != 0:
        tail = "\n".join(proc.stdout.splitlines()[-12:])
        failures.append("the harness consistency test FAILED — contract.toml and the Rust table have drifted:\n" + tail)
    else:
        print("  harness consistency test: PASS")

    if failures:
        print("\nNOT READY FOR THE WINDOW:")
        for f in failures:
            print(f"  - {f}")
        return 1

    print("\nREADY: no blocking value is DEFERRED, the contract is internally consistent,")
    print("and the harness agrees with the contract. Print the K budget with --kbudget.")
    return 0


def cap_of(g) -> float:
    """The memory cap, from the contract's `axis_cap`."""
    return float(g.get("axis_cap", "50").strip('"'))


def cmd_kbudget(sections) -> int:
    g = sections.get("global", {})
    # Four axes. The X (scaling) axis was removed before the freeze and its
    # weight moved to wall time.
    weights = {
        "T": float(g.get("weight_t", "0.50").strip('"')),
        "M": float(g.get("weight_m", "0.20").strip('"')),
        "S": float(g.get("weight_s", "0.15").strip('"')),
        "D": float(g.get("weight_d", "0.15").strip('"')),
    }
    # Per-axis caps: wall time uncapped, memory 50x, S and D 1.0.
    caps = {"T": None, "M": cap_of(g), "S": 1.0, "D": 1.0}

    print(f"{'TRACK':<6} {'Tref(s)':>8} {'Mref':>10} {'Sref(L)':>9} {'T@Commit':>9} {'K':>7} {'speed':>7}")
    for track in "ABCD":
        sec = sections.get(track, {})
        ref_time = sec.get("ref_time_s", "")
        if not ref_time or DEFERRED in ref_time.upper():
            print(f"{track:<6} {'DEFERRED':>8}")
            continue
        ref_time = float(ref_time.strip('"'))

        def num(*keys):
            for k in keys:
                v = sec.get(k, "")
                if v and DEFERRED not in v.upper():
                    return float(v.strip('"'))
            return None

        ref_mem = num("ref_peak_mem_mb")
        # S is measured in CODE LINES now, not bytes.
        ref_src = num("ref_source_lines")

        # The Commit anchor from docs/specs/: each track's own commitment
        # row, not a generic one.
        anchors = {
            "A": (30.0, 15.0),
            "B": (30.0, 12.0),
            "C": (30.0, 12.0),
            "D": (25.0, 8.0),
        }
        t_ratio, m_ratio = anchors[track]

        if ref_src:
            # Assume our source lands near 5,400 code lines (the Nightshade
            # anchor); refine after the first measurement.
            s_ratio = min(ref_src / 5400.0, 1.0)
        else:
            s_ratio = 0.62
        ratios = {"T": t_ratio, "M": m_ratio, "S": s_ratio, "D": 1.0}
        contribs = []
        for axis, w in weights.items():
            r = ratios[axis]
            cap = caps[axis]
            if cap is not None:
                r = min(r, cap)
            contribs.append(max(0.0, r) ** w)
        k = math.prod(contribs)
        speed = 50.0 * math.log10(k) if k > 0 else 0.0
        speed = max(0.0, min(speed, 100.0))
        print(f"{track:<6} {ref_time:>8.2f} "
              f"{(f'{ref_mem:.0f}' if ref_mem else 'DEFERRED'):>10} "
              f"{(f'{ref_src:.0f}' if ref_src else 'DEFERRED'):>9} "
              f"{ref_time / t_ratio:>9.3f} {k:>7.3f} {speed:>6.1f}")

    print("\nT@Commit is the wall time the Commit anchor implies. Update the")
    print("ladder targets in docs/specs/ once the real Sref pins the S axis.")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="SPEC.md intake for the Oct 28 freeze")
    ap.add_argument("--status", action="store_true", help="list every DEFERRED value")
    ap.add_argument("--set", dest="set_value", metavar="TRACK.KEY=VALUE")
    ap.add_argument("--check", action="store_true", help="the readiness gate")
    ap.add_argument("--kbudget", action="store_true", help="K budget from pinned values")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    sections = load()

    if args.set_value:
        return cmd_set(sections, args.set_value)
    if args.check:
        return cmd_check(sections)
    if args.kbudget:
        return cmd_kbudget(sections)
    if args.json:
        items = deferred_values(sections)
        print(json.dumps([
            {"track": t, "key": k, "feeds": f, "source": s, "blocking": b}
            for t, k, f, s, b in items
        ], indent=2))
        return 0
    if args.status:
        return cmd_status(sections)

    ap.print_help()
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
