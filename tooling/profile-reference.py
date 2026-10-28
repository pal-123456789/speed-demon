#!/usr/bin/env python3
"""profiler — measure WHERE the reference actually spends its time.

Our design docs carry hypotheses ("timestamp parsing is 35-45% of Track A",
"the edge parse dominates Track D"). This tool replaces them with numbers by
running the organizers' reference under cProfile on a real data dir and
attributing self-time to pipeline phases.

That attribution is what sets the optimisation order: a phase holding 40% of
the reference's time is worth 0.50-weight effort; a phase holding 3% is not,
no matter how elegant the trick.

Usage:
    python tooling/profile-reference.py --track a
    python tooling/profile-reference.py --track all --top 12
    python tooling/profile-reference.py --track d --data verification/data-adversarial/track-d
"""

from __future__ import annotations

import argparse
import cProfile
import importlib.util
import io
import pstats
import sys
import time
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
REFERENCE = {"a": "a_ingest.py", "b": "b_index.py", "c": "c_join.py", "d": "d_traverse.py"}
TRACKS = ("a", "b", "c", "d")

# Which phase a function belongs to, by name fragment, per track. Ordered:
# the first fragment that matches wins.
PHASES: dict[str, list[tuple[str, tuple[str, ...]]]] = {
    "a": [
        ("timestamp", ("strptime", "datetime", "_strptime", "time")),
        ("line parse", ("split", "rstrip", "readline", "TextIOWrapper")),
        ("aggregate", ("append", "dict", "setdefault", "get")),
        ("percentile/sort", ("sorted", "sort", "percentile")),
        ("output", ("strftime", "join", "write", "format")),
    ],
    "b": [
        ("tokenize", ("findall", "lower", "compile", "sub")),
        ("index build", ("set", "add", "partition", "get", "update")),
        ("boolean eval", ("run_bool", "intersection", "union", "difference", "&", "|", "-")),
        ("phrase eval", ("run_phrase", "slice", "tokenize")),
        ("output", ("format_result", "join", "str", "write")),
    ],
    "c": [
        ("csv parse", ("reader", "next", "__next__", "parse_int")),
        ("amount", ("parse_cents", "match", "search", "format_cents")),
        ("join", ("get", "load_regions")),
        ("aggregate", ("add", "set", "min", "max", "len")),
        ("output", ("line", "join", "encode", "write")),
    ],
    "d": [
        ("edge parse", ("read_graph", "split", "int", "append")),
        ("dijkstra", ("shortest_path", "heappush", "heappop", "heapq")),
        ("query i/o", ("read_queries", "write", "join", "open")),
        ("other", ("range", "len", "list")),
    ],
}
LADDER_HINT = {
    "a": {"timestamp": "A03/A13 (hour key, no calendar math)",
          "line parse": "A01/A17 (SWAR/AVX2 split)",
          "aggregate": "A11/A14 (bump chunks, parallel shards)",
          "percentile/sort": "A07/A13 (selection or radix histogram)",
          "output": "A09/A20 (hand-rolled formatting, one write)"},
    "b": {"tokenize": "B01/B21 (class table, SIMD)",
          "index build": "B11/B13 (single pass, deterministic ids)",
          "boolean eval": "B14 (shortest-lead intersection)",
          "phrase eval": "B05/B15 (positions, k-cursor)",
          "output": "B09 (top-10 formatting)"},
    "c": {"csv parse": "C01/C12 (oracle, parity split)",
          "amount": "C04/C22 (i64 cents, SWAR)",
          "join": "C06/C25 (hash join, prefetch)",
          "aggregate": "C14/C24 (i128 sums, distinct dedupe)",
          "output": "C23 (table-driven formatting)"},
    "d": {"edge parse": "D02/D21 (SWAR u32, chunked pread)",
          "dijkstra": "D11/D12 (radix heap, early exit, warm scratch)",
          "query i/o": "D13 (parallel queries)",
          "other": "-"},
}


def load_reference(track: str):
    """Import the reference module so we can call main(data, out) in-process."""
    path = ROOT / "reference" / REFERENCE[track]
    spec = importlib.util.spec_from_file_location(f"ref_{track}", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def phase_of(track: str, func: str) -> str:
    low = func.lower()
    for name, frags in PHASES[track]:
        if any(f.lower() in low for f in frags):
            return name
    return "unattributed"


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--track", choices=[*TRACKS, "all"], default="all")
    ap.add_argument("--data", help="data dir (default verification/data/track-<t>)")
    ap.add_argument("--top", type=int, default=10, help="how many functions to list")
    args = ap.parse_args(argv)

    tracks = list(TRACKS) if args.track == "all" else [args.track]
    rc = 0
    for t in tracks:
        data = Path(args.data) if args.data else ROOT / "verification" / "data" / f"track-{t}"
        if not data.is_dir():
            print(f"[track {t}] no data at {data}", file=sys.stderr)
            rc = 1
            continue
        out = Path(__import__("tempfile").mkdtemp(prefix=f"prof-{t}-"))
        mod = load_reference(t)

        prof = cProfile.Profile()
        wall0 = time.perf_counter()
        prof.enable()
        try:
            mod.main(str(data), str(out))
        finally:
            prof.disable()
        wall = time.perf_counter() - wall0

        buf = io.StringIO()
        stats = pstats.Stats(prof, stream=buf).sort_stats("tottime")
        # attribute self-time to phases
        phase_time: dict[str, float] = {}
        total_self = 0.0
        rows = []
        for func, (_cc, _nc, tt, ct, _callers) in stats.stats.items():
            fname = func[2]
            where = phase_of(t, fname)
            phase_time[where] = phase_time.get(where, 0.0) + tt
            total_self += tt
            rows.append((tt, ct, fname))
        rows.sort(reverse=True)

        print(f"\n=== TRACK {t.upper()} — reference profile ({data.name}) ===")
        print(f"  wall clock : {wall:.2f} s   (self-time attributed: {total_self:.2f} s)")
        print(f"  output     : {out.name}")
        print(f"\n  {'phase':<18}{'self s':>9}{'share':>9}   the rung it justifies")
        for name, secs in sorted(phase_time.items(), key=lambda kv: -kv[1]):
            share = (secs / total_self * 100) if total_self else 0.0
            hint = LADDER_HINT.get(t, {}).get(name, "-")
            print(f"  {name:<18}{secs:>9.3f}{share:>8.1f}%   {hint}")
        print(f"\n  top {args.top} functions by self time:")
        for tt, ct, fname in rows[: args.top]:
            print(f"    {tt:>8.4f}s self  {ct:>8.4f}s cum  {fname}")
        # sanity: the phase that dominates should own the first rung
        if phase_time and total_self:
            top_phase, top_secs = max(phase_time.items(), key=lambda kv: kv[1])
            print(f"\n  >>> attack first: {top_phase} "
                  f"({top_secs / total_self * 100:.1f}% of self time)")
        import shutil
        shutil.rmtree(out, ignore_errors=True)
    return rc


if __name__ == "__main__":
    raise SystemExit(main())