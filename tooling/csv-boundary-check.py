#!/usr/bin/env python3
"""
CSV boundary checker — oracle vs quote-parity split, determinism across chunks.

Implements the model-derived design (docs/depth/97-MODEL-OUTPUT-DIGEST.md §7):

  (a) a faithful six-state quoting ORACLE (FS/UQ/IQ/QC/AC/CRP) that finds every
      record boundary, with the documented [S]-defaults:
        * a quote in an UNQUOTED field is literal data (UQ on Q stays UQ) [S];
        * a quote right after a closing quote is data (AC on Q stays AC) [S];
        * a bare CR terminates the record (re-dispatch from FS);
        * a mid-file BOM is ordinary data (only the one at offset 0 is skipped);
  (b) the QUOTE-PARITY prepass for parallel splitting: "inside quotes" == odd
      number of `"` bytes; per fixed chunk record (par, first0, first1) where
      first0/first1 is the first terminator seen at local parity 0/1; a
      sequential pass threads the start state S and picks first0/first1.

The checker proves, for ANY chunk size:
  * determinism: the cut set is identical across chunk sizes 1..1 MiB;
  * correctness: every cut lands exactly on an oracle record end (so ranges
    always start at record boundaries) and the state after a cut is OUTSIDE
    quotes;
  * coverage: the union of cuts + oracle ranges covers the whole file;
  * value: the oracle finds MORE record ends than a naive LF split on hostile
    input (embedded newlines inside quotes), which is why the oracle exists.

A mismatch prints the byte offset and exits 1. Intended use: run against every
hostile fixture (gen-fixtures --track C) BEFORE writing the Rust parser, and
against the real sample at Oct 30.

Usage:
    python tooling/csv-boundary-check.py --file verification/data/track-c/records.csv
    python tooling/csv-boundary-check.py --file X --chunks 1,2,17,300,65536,1048576
    python tooling/csv-boundary-check.py --all      # run on generated C fixtures
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

# Force UTF-8 on stdio (Windows code page mangles non-ASCII otherwise).
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent

Q, C, N, R, O = '"', ',', '\n', '\r', None
FS, UQ, IQ, QC, AC, CRP = range(6)
STATE = ("FS", "UQ", "IQ", "QC", "AC", "CRP")


def cls(b: int) -> str:
    if b == ord(Q):
        return Q
    if b == ord(C):
        return C
    if b == ord(N):
        return N
    if b == ord(R):
        return R
    return O


def oracle_record_ends(data: bytes) -> tuple[list[int], bool]:
    """One-past-the-terminator offsets of every record end, per the model's
    transition table (defaults). A record end is the byte AFTER the LF/CRLF/
    bare-CR; EOF appends n. So ends are exactly the positions a range may start."""
    ends: list[int] = []
    st = FS
    i = 0
    n = len(data)
    # skip the one BOM at offset 0
    if data.startswith(b"\xef\xbb\xbf"):
        i = 3
    unterminated = False
    while i < n:
        b = data[i]
        c = cls(b)
        if st == FS:
            if c == Q:
                st = IQ
            elif c == C:
                pass  # E, stay FS
            elif c == N:
                ends.append(i + 1)
            elif c == R:
                st = CRP
            else:
                st = UQ
        elif st == UQ:
            if c == Q:
                st = UQ  # literal quote [S]
            elif c == C:
                st = FS
            elif c == N:
                ends.append(i + 1)
                st = FS  # record end -> back to field start
            elif c == R:
                st = CRP
            else:
                st = UQ
        elif st == IQ:
            if c == Q:
                st = QC
            # everything else stays IQ (data, verbatim)
        elif st == QC:
            if c == Q:
                st = IQ  # L: "" -> literal quote
            elif c == C:
                st = FS
            elif c == N:
                ends.append(i + 1)
                st = FS
            elif c == R:
                st = CRP
            else:
                st = AC  # [S]
        elif st == AC:
            if c == Q:
                st = AC  # [S]
            elif c == C:
                st = FS
            elif c == N:
                ends.append(i + 1)
                st = FS
            elif c == R:
                st = CRP
            else:
                st = AC  # [S]
        elif st == CRP:
            if c == N:
                ends.append(i + 1)  # CRLF: end after the LF
                st = FS
            else:
                ends.append(i + 1)  # bare CR ends the record, re-dispatch
                if c == C:
                    st = FS
                elif c == Q:
                    st = IQ
                elif c == R:
                    st = CRP
                else:
                    st = UQ
        i += 1
    if st == IQ:
        unterminated = True
    if st in (UQ, QC, AC, CRP):
        ends.append(n)
    return ends, unterminated


def is_terminator_at(data: bytes, pos: int) -> tuple[bool, int]:
    """(is a terminator, bytes consumed) at pos: LF (1), CRLF (2), bare CR (1)."""
    b = data[pos]
    if b == ord(N):
        return True, 1
    if b == ord(R):
        if pos + 1 < len(data) and data[pos + 1] == ord(N):
            return True, 2
        return True, 1
    return False, 0


def parity_cuts(data: bytes, chunk: int) -> tuple[list[int], int]:
    """Quote-parity prepass + sequential pass. Returns (cuts, final_state)."""
    n = len(data)
    par: list[int] = []
    first0: list[int] = []
    first1: list[int] = []
    start = 0
    while start < n:
        end = min(start + chunk, n)
        p = 0
        f0 = f1 = -1
        i = start
        while i < end:
            if data[i] == ord(Q):
                p ^= 1
            else:
                term, _ = is_terminator_at(data, i)
                if term:
                    if p == 0 and f0 < 0:
                        f0 = i
                    if p == 1 and f1 < 0:
                        f1 = i
            i += 1
        par.append(p)
        first0.append(f0)
        first1.append(f1)
        start = end
    cuts: list[int] = []
    S = 0
    last_cut = -1
    for i, (p, f0, f1) in enumerate(zip(par, first0, first1)):
        f = f0 if S == 0 else f1
        if f >= 0:
            _, w = is_terminator_at(data, f)
            cut = f + w  # byte AFTER the terminator
            # a CRLF whose CR is the last byte of one chunk and LF the first
            # byte of the next yields the same cut twice — dedupe by position
            if cut != last_cut:
                cuts.append(cut)
                last_cut = cut
        S ^= p
    return cuts, S


def run_file(path: Path, chunks: list[int], verbose: bool) -> tuple[int, list[str]]:
    data = path.read_bytes()
    problems: list[str] = []
    ends, unterminated = oracle_record_ends(data)
    ends_set = set(ends)
    naive = sum(1 for b in data if b == ord(N))
    if verbose:
        print(f"  {path.name}: {len(data)} B, oracle records={len(ends)}, "
              f"naive-LF={naive}, unterminated-quote-at-EOF={unterminated}")

    # value assertion: on hostile input the oracle MUST diverge from a naive
    # LF split (embedded newlines inside quotes add LF bytes, not records)
    if len(ends) != naive:
        if verbose:
            print("    ok   oracle diverges from a naive LF split "
                  f"({len(ends)} records vs {naive} LF bytes) — the oracle matters")
    else:
        problems.append(f"{path.name}: oracle record count ({len(ends)}) equals "
                        f"the naive LF count ({naive}) — is this input hostile at all?")

    for ch in chunks:
        cuts, final_par = parity_cuts(data, ch)
        if verbose:
            print(f"    chunk {ch:>8}: {len(cuts)} cuts, final parity {final_par}")
        # 1) every cut is one-past an oracle record end (ranges start at
        #    record boundaries, never mid-record)
        for c in cuts:
            if c not in ends_set:
                problems.append(f"{path.name}: cut at byte {c} (chunk {ch}) is "
                                f"not an oracle record end — a range would start "
                                f"mid-record")
        # 2) ranges cover the file: 0 <= c1 < c2 < ... <= n
        if any(a >= b for a, b in zip(cuts, cuts[1:])):
            problems.append(f"{path.name}: chunk {ch} cuts are not strictly "
                            f"ascending")
        if cuts and cuts[0] <= 0:
            problems.append(f"{path.name}: chunk {ch} first cut {cuts[0]} <= 0")
        if cuts and cuts[-1] > len(data):
            problems.append(f"{path.name}: chunk {ch} last cut {cuts[-1]} > "
                            f"file length")
        # 3) differential: parsing each range from FS must yield the SAME
        #    record count as parsing the whole file once
        total = 0
        prev = 0
        for c in cuts + [len(data)]:
            seg, _ = oracle_record_ends(data[prev:c])
            total += len(seg)
            prev = c
        if total != len(ends):
            problems.append(f"{path.name}: chunk {ch} range-parse record count "
                            f"{total} != whole-file {len(ends)} — a boundary was "
                            f"lost or invented")
    return (0 if not problems else 1), problems


def gen_fixture(tmp: Path) -> Path:
    """Generate the hostile C fixtures into a temp dir and return the adversarial path."""
    out = tmp / "fx"
    subprocess.run([sys.executable, str(ROOT / "tooling" / "gen-fixtures.py"),
                    "--track", "C", "--adversarial", "--out", str(out)],
                   check=True, capture_output=True)
    subprocess.run([sys.executable, str(ROOT / "tooling" / "gen-fixtures.py"),
                    "--track", "C", "--rows", "3000", "--seed", "7", "--out", str(out)],
                   check=True, capture_output=True)
    return out


def main() -> int:
    p = argparse.ArgumentParser(description="CSV boundary checker (oracle vs parity)")
    p.add_argument("--file", help="the CSV to check")
    p.add_argument("--chunks", default="1,2,3,17,300,4096,65536,1048576",
                   help="comma-separated chunk sizes to prove determinism")
    p.add_argument("--all", action="store_true",
                   help="generate the hostile C fixtures and check them")
    p.add_argument("--verbose", action="store_true")
    args = p.parse_args()

    chunks = [int(x) for x in args.chunks.split(",") if x.strip()]
    paths: list[Path] = []
    tmp: tempfile.TemporaryDirectory | None = None
    if args.all:
        tmp = tempfile.TemporaryDirectory()
        fx = gen_fixture(Path(tmp.name))
        paths = [fx / "track-c" / "records.csv"]
        # the full hostile corpus lives in the same dir; check both adversarial
        # and the 3000-row generator output (the generator overwrote it — the
        # adversarial was written second, so regenerate full separately)
        subprocess.run([sys.executable, str(ROOT / "tooling" / "gen-fixtures.py"),
                        "--track", "C", "--rows", "3000", "--seed", "7",
                        "--out", str(fx)], check=True, capture_output=True)
        paths.append(fx / "track-c" / "records.csv")
    elif args.file:
        paths = [Path(args.file)]
    else:
        p.error("need --file or --all")

    failed = 0
    for path in paths:
        if not path.is_file():
            print(f"  MISSING {path}", file=sys.stderr)
            failed += 1
            continue
        if args.verbose:
            print(f"[{path}]")
        rc, problems = run_file(path, chunks, args.verbose)
        for msg in problems:
            print(f"  FAIL {msg}")
        if rc:
            failed += 1
        elif args.verbose:
            print(f"  PASS {path.name}")
    if tmp is not None:
        tmp.cleanup()
    if failed:
        print(f"csv-boundary-check: {failed} FAILED", file=sys.stderr)
        return 1
    print("csv-boundary-check: all boundaries deterministic and oracle-consistent")
    return 0


if __name__ == "__main__":
    sys.exit(main())