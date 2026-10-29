#!/usr/bin/env python3
"""spec-cases — the tiny conformance corpus and the instant feedback loop.

The specs say every documented edge case is injected "at least once in tiny".
So the `tiny` corpora under `verification/cases/` are a complete, oracle-verified
conformance suite: each is small enough to run in milliseconds and contains every
case the holdout will inject.

Two modes:

  --verify-corpus   re-derive each expected answer from the officials' reference
                    and prove the stored `expected-<output>` still matches.
                    (CI gate: proves the corpus is authoritative, not stale.)
  --run             run the CANDIDATE binary on each case and diff it against the
                    expected answer, printing the first divergence with context.
                    This is the H0 inner loop — seconds per iteration, and every
                    documented edge case is in the input.

Usage:
    python tooling/spec-cases.py --verify-corpus
    python tooling/spec-cases.py --run --track a
    python tooling/spec-cases.py --run --build          # build first
"""

from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
import tempfile
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _shell import shell  # noqa: E402  (portable sh discovery)

SENTINEL = b"#SCAFFOLD_NOT_IMPLEMENTED"

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "verification" / "cases"
REFERENCE = {"a": "a_ingest.py", "b": "b_index.py", "c": "c_join.py", "d": "d_traverse.py"}
OUTPUT = {"a": "summary.txt", "b": "results.txt", "c": "aggregates.txt", "d": "distances.txt"}
TRACKS = ("a", "b", "c", "d")
PY = sys.executable


def sha(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def diff_report(exp: Path, got: Path) -> str:
    if not got.is_file():
        return "candidate produced no output file"
    a, b = exp.read_bytes(), got.read_bytes()
    if a == b:
        return ""
    n = min(len(a), len(b))
    i = next((k for k in range(n) if a[k] != b[k]), n)
    ctx_a = a[max(0, i - 50):i + 50]
    ctx_b = b[max(0, i - 50):i + 50]
    return (f"first difference at byte {i} (line {a[:i].count(chr(10).encode()) + 1} vs "
            f"{b[:i].count(chr(10).encode()) + 1}), {len(a)} vs {len(b)} bytes\n"
            f"      expected: {ctx_a!r}\n"
            f"      candidate: {ctx_b!r}")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--track", choices=[*TRACKS, "all"], default="all")
    ap.add_argument("--verify-corpus", action="store_true",
                    help="prove the stored expected answers match the reference")
    ap.add_argument("--run", action="store_true", help="run the candidate against the corpus")
    ap.add_argument("--build", action="store_true", help="build the candidate first")
    args = ap.parse_args(argv)
    if not (args.verify_corpus or args.run):
        args.verify_corpus = True

    tracks = list(TRACKS) if args.track == "all" else [args.track]
    failures = 0

    for t in tracks:
        case = CASES / f"track-{t}"
        outname = OUTPUT[t]
        expected = case / f"expected-{outname}"
        if not case.is_dir() or not expected.is_file():
            print(f"[track {t}] no corpus at {case} — run: "
                  f"python tooling/official-data.py --track {t} --scale tiny "
                  f"--dest verification/cases --save-expected --no-pin")
            failures += 1
            continue
        print(f"[track {t}] corpus {case.name}: {expected.stat().st_size} B expected")

        if args.verify_corpus:
            tmp = Path(tempfile.mkdtemp(prefix=f"speccase-{t}-"))
            p = subprocess.run([PY, str(ROOT / "reference" / REFERENCE[t]),
                                "--data", str(case), "--out", str(tmp)],
                               capture_output=True, text=True)
            if p.returncode != 0:
                print(f"  FAIL the official reference rejects this case: "
                      f"{(p.stderr or '').strip()[-160:]}")
                failures += 1
            else:
                got = tmp / outname
                if got.is_file() and sha(got) == sha(expected):
                    print(f"  ok   the stored answer is what the official reference produces "
                          f"(sha {sha(expected)[:12]}...)")
                else:
                    print("  FAIL the stored answer differs from the reference's output")
                    failures += 1
            import shutil
            shutil.rmtree(tmp, ignore_errors=True)

        if args.run:
            if args.build:
                b = subprocess.run([shell(), f"track-{t}/build.sh"], cwd=ROOT,
                                   capture_output=True, text=True)
                if b.returncode != 0:
                    print(f"  FAIL candidate build: {(b.stderr or '').strip()[-160:]}")
                    failures += 1
                    continue
            exe = ROOT / f"track-{t}" / "run"
            if not exe.is_file():
                exe = ROOT / f"track-{t}" / "run.exe"
            out = Path(tempfile.mkdtemp(prefix=f"speccand-{t}-"))
            env = dict(os.environ, SD_DATA_DIR=str(case), SD_OUT_DIR=str(out))
            r = subprocess.run([str(exe)], capture_output=True, text=True, env=env)
            rep = diff_report(expected, out / outname) if r.returncode == 0 else \
                f"candidate exited {r.returncode}: {(r.stderr or '').strip()[:160]}"
            got = out / outname
            if got.is_file() and got.read_bytes().startswith(SENTINEL):
                print("  SCAFFOLD the candidate is still the pre-kickoff stub "
                      "(expected: it writes a sentinel, not the answer)")
            elif rep:
                print(f"  FAIL {rep}")
                failures += 1
            else:
                print("  ok   candidate matches the oracle on every documented edge case")
            import shutil
            shutil.rmtree(out, ignore_errors=True)

    print()
    if failures:
        print(f"spec-cases: {failures} FAILED", file=sys.stderr)
        return 1
    print("spec-cases: corpus verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())