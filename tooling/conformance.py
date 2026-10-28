#!/usr/bin/env python3
"""Conformance lab — differential testing against the organizers' reference.

The rules make byte-identical copies of the published references free, so the
naive Python reference for every track is vendored under `reference/`. That
turns "does my output match?" from a hope into a local byte diff, and it lets
us validate our own fixture generators against the oracle BEFORE kickoff.

Three jobs:

  --oracle      run the reference on a data dir and report/save what it
                produces. Also proves the data dir is oracle-ACCEPTABLE
                (the reference raises on anything outside the contract, e.g.
                a non-integer region_id, a doc id out of order).
  --diff        run the reference and the Rust candidate on the same data and
                byte-compare the outputs, printing the first divergence with
                line/byte context (the H0+ inner loop).
  --minimize    when they differ, shrink the input by delta-debugging records
                until the mismatch is minimal — the repro you actually debug.

Usage:
    python tooling/conformance.py --track a --oracle
    python tooling/conformance.py --track all --oracle          # fixture validity gate
    python tooling/conformance.py --track c --diff --build
    python tooling/conformance.py --track c --diff --minimize
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _shell import shell  # noqa: E402  (portable sh discovery)

ROOT = Path(__file__).resolve().parent.parent
REFERENCE = {"a": "a_ingest.py", "b": "b_index.py", "c": "c_join.py", "d": "d_traverse.py"}
OUTPUT = {"a": "summary.txt", "b": "results.txt", "c": "aggregates.txt", "d": "distances.txt"}
TRACKS = ("a", "b", "c", "d")
PY = sys.executable


def run_reference(track: str, data: Path, out: Path) -> tuple[int, str]:
    """Run the official reference. Returns (rc, stderr tail)."""
    out.mkdir(parents=True, exist_ok=True)
    ref = ROOT / "reference" / REFERENCE[track]
    if not ref.is_file():
        return 127, f"missing reference {ref}"
    p = subprocess.run([PY, str(ref), "--data", str(data), "--out", str(out)],
                       capture_output=True, text=True)
    return p.returncode, (p.stderr or "").strip().splitlines()[-1] if p.stderr else ""


def build_candidate(track: str) -> tuple[int, str]:
    p = subprocess.run([shell(), f"track-{track}/build.sh"], cwd=ROOT,
                       capture_output=True, text=True)
    return p.returncode, (p.stderr or "").strip()


def run_candidate(track: str, data: Path, out: Path) -> tuple[int, str]:
    out.mkdir(parents=True, exist_ok=True)
    exe = ROOT / f"track-{track}" / "run"
    if not exe.is_file():
        exe = ROOT / f"track-{track}" / "run.exe"
    env = dict(os.environ, SD_DATA_DIR=str(data), SD_OUT_DIR=str(out))
    p = subprocess.run([str(exe)], capture_output=True, text=True, env=env)
    return p.returncode, (p.stderr or "").strip().splitlines()[0] if p.stderr else ""


def sha(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def first_divergence(a: Path, b: Path) -> str:
    """Describe where two output files first differ, with context."""
    if not a.is_file() or not b.is_file():
        return f"missing output (expected={a.is_file()}, actual={b.is_file()})"
    ab, bb = a.read_bytes(), b.read_bytes()
    if ab == bb:
        return ""
    n = min(len(ab), len(bb))
    i = next((k for k in range(n) if ab[k] != bb[k]), n)
    la = ab[:i].count(b"\n") + 1
    lb = bb[:i].count(b"\n") + 1
    ctx_a = ab[max(0, i - 40):i + 40]
    ctx_b = bb[max(0, i - 40):i + 40]
    return (f"first difference at byte {i} (line {la} vs {lb}), "
            f"sizes {len(ab)} vs {len(bb)}\n"
            f"    reference: {ctx_a!r}\n"
            f"    candidate: {ctx_b!r}")


def isolate(data: Path) -> tuple[Path, str]:
    """Copy the two/three input files a track needs into a scratch dir."""
    names = {"a": ["requests.log"],
             "b": ["docs.txt", "queries.txt"],
             "c": ["records.csv", "regions.csv"],
             "d": ["graph.txt", "queries.txt"]}
    track = data.name.replace("track-", "")
    tmp = Path(tempfile.mkdtemp(prefix=f"conf-{track}-"))
    biggest = None
    for n in names.get(track, []):
        src = data / n
        if src.is_file():
            shutil.copy2(src, tmp / n)
            if biggest is None or src.stat().st_size > (tmp / biggest).stat().st_size:
                biggest = n
    return tmp, biggest or ""


def minimize(track: str, data: Path, workdir: Path) -> tuple[Path, int]:
    """Delta-debug the largest input file by deleting lines while the
    reference<->candidate mismatch persists. Returns (dir, lines_removed)."""
    scratch, biggest = isolate(data)
    if not biggest:
        return scratch, 0
    target = scratch / biggest
    lines = target.read_bytes().split(b"\n")
    # greedy chunk removal: try 1/2, 1/4, ... of the file, keep removals that
    # preserve the mismatch
    removed = 0
    chunk = max(1, len(lines) // 2)
    while chunk >= 1:
        i = 0
        while i < len(lines):
            trial = lines[:i] + lines[i + chunk:]
            if not trial:
                i += chunk
                continue
            target.write_bytes(b"\n".join(trial))
            r_out = Path(tempfile.mkdtemp(prefix="conf-ref-"))
            c_out = Path(tempfile.mkdtemp(prefix="conf-cand-"))
            rrc, _ = run_reference(track, scratch, r_out)
            crc, _ = run_candidate(track, scratch, c_out)
            same = False
            if rrc == 0 and crc == 0:
                same = (r_out / OUTPUT[track]).read_bytes() == (c_out / OUTPUT[track]).read_bytes()
            if rrc == 0 and crc == 0 and not same:
                lines = trial          # mismatch persists: keep the removal
                removed += chunk
            else:
                target.write_bytes(b"\n".join(lines))  # restore
                i += chunk
            shutil.rmtree(r_out, ignore_errors=True)
            shutil.rmtree(c_out, ignore_errors=True)
        chunk //= 2
    return scratch, removed


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--track", choices=[*TRACKS, "all"], default="all")
    ap.add_argument("--data", help="exact data dir (default verification/data/track-<t>)")
    ap.add_argument("--data-root", help="root holding track-<t>/ subdirs (e.g. verification/data-adversarial)")
    ap.add_argument("--oracle", action="store_true", help="run the reference only (fixture validity)")
    ap.add_argument("--diff", action="store_true", help="reference vs candidate byte diff")
    ap.add_argument("--build", action="store_true", help="build the candidate first")
    ap.add_argument("--minimize", action="store_true", help="shrink a divergent input")
    ap.add_argument("--save-expected", metavar="DIR", help="copy the reference output here")
    args = ap.parse_args(argv)

    tracks = list(TRACKS) if args.track == "all" else [args.track]
    if not (args.oracle or args.diff):
        args.oracle = True
    failures = 0

    for t in tracks:
        if args.data:
            data = Path(args.data)
        elif args.data_root:
            data = Path(args.data_root) / f"track-{t}"
        else:
            data = ROOT / "verification" / "data" / f"track-{t}"
        print(f"[track {t}] data={data}")
        if not data.is_dir():
            print(f"  FAIL no data dir {data}")
            failures += 1
            continue

        if args.build:
            rc, err = build_candidate(t)
            if rc != 0:
                print(f"  FAIL candidate build: {err}")
                failures += 1
                continue

        r_out = Path(tempfile.mkdtemp(prefix=f"conf-ref-{t}-"))
        rrc, rerr = run_reference(t, data, r_out)
        exp = r_out / OUTPUT[t]
        if rrc != 0 or not exp.is_file():
            print(f"  FAIL reference rejected the data (rc={rrc}): {rerr}")
            print("       -> the data dir is outside the contract; fix the generator")
            failures += 1
            shutil.rmtree(r_out, ignore_errors=True)
            continue
        print(f"  ok   reference accepted the data, wrote {OUTPUT[t]} "
              f"({exp.stat().st_size} B, sha {sha(exp)[:12]}...)")
        if args.save_expected:
            dest = Path(args.save_expected)
            dest.mkdir(parents=True, exist_ok=True)
            shutil.copy2(exp, dest / OUTPUT[t])
            print(f"       saved expected output -> {dest / OUTPUT[t]}")

        if args.diff:
            c_out = Path(tempfile.mkdtemp(prefix=f"conf-cand-{t}-"))
            crc, cerr = run_candidate(t, data, c_out)
            act = c_out / OUTPUT[t]
            if crc != 0 or not act.is_file():
                print(f"  FAIL candidate produced no {OUTPUT[t]} (rc={crc}) {cerr}")
                failures += 1
            else:
                diff = first_divergence(exp, act)
                if diff:
                    print(f"  FAIL outputs differ — {diff}")
                    failures += 1
                    if args.minimize:
                        print("       minimizing ...")
                        small, removed = minimize(t, data, ROOT)
                        print(f"       minimized repro: {small} "
                              f"({removed} lines removed; mismatch persists)")
                else:
                    print(f"  ok   outputs are byte-identical (sha {sha(act)[:12]}...)")
            shutil.rmtree(c_out, ignore_errors=True)
        shutil.rmtree(r_out, ignore_errors=True)

    print()
    if failures:
        print(f"conformance: {failures} FAILED", file=sys.stderr)
        return 1
    print("conformance: the oracle accepts every data dir checked")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())