#!/usr/bin/env python3
"""difftest — when the SHA gate fails, find out WHY in one command.

The correctness gate is binary: the output must hash to exactly one value.
A wrong answer is therefore an expensive event — it costs the whole track.
The difference between a 5-minute and a 3-hour diagnosis is knowing *where*
the bytes diverge and *how*.

This tool compares a candidate output against a reference (or a pinned
expected hash) and reports:

  * the first differing byte, with offset, line, column, and hex context
  * the differing lines side by side
  * a diagnosis of the usual causes, in order of how often they are the
    actual cause:
        - a trailing newline present in one file and not the other
        - CRLF vs LF line endings
        - a UTF-8 BOM at the start
        - a different number of lines
        - identical after stripping trailing whitespace
        - identical after collapsing runs of whitespace
        - a floating-point value formatted differently (last 12 bytes
          differ but the lines agree to 9 significant digits)
  * a column-level diff for CSV-shaped output, so a wrong join key or a
    wrong aggregate is obvious rather than inferred

Usage:
    python tooling/difftest.py candidate.txt reference.txt
    python tooling/difftest.py candidate.txt --expect ca6311bbeb536fa55d85265bf6fd3d24cc9586f13de1ac73bd2a23ac3893aac3
    python tooling/difftest.py candidate.txt reference.txt --json
    python tooling/difftest.py candidate.txt reference.txt --cols 5
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")



def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def hexdump(b: bytes, start: int, end: int) -> str:
    out = []
    for off in range(start, end, 16):
        chunk = b[off : off + 16]
        hexpart = " ".join(f"{c:02x}" for c in chunk)
        asciipart = "".join(chr(c) if 32 <= c < 127 else "." for c in chunk)
        out.append(f"  {off:08x}  {hexpart:<47}  |{asciipart}|")
    return "\n".join(out)


def line_of_offset(data: bytes, off: int) -> tuple[int, int]:
    """1-based line number and 1-based column of a byte offset."""
    line = data.count(b"\n", 0, off) + 1
    line_start = data.rfind(b"\n", 0, off) + 1
    col = off - line_start + 1
    return line, col


def split_lines(data: bytes) -> list[bytes]:
    return data.split(b"\n")


def diagnose(cand: bytes, ref: bytes) -> list[str]:
    """The usual causes, most likely first."""
    notes: list[str] = []
    c, r = cand, ref

    if c == r:
        return ["byte-identical"]

    # 1. trailing newline
    c_strip = c.rstrip(b"\n")
    r_strip = r.rstrip(b"\n")
    if c_strip == r_strip:
        notes.append(
            "DIFFERENCE IS ONLY THE TRAILING NEWLINE: "
            f"candidate ends with {c[len(c_strip):]!r}, reference ends with {r[len(r_strip):]!r}. "
            "Most output formats require a final newline."
        )
        return notes

    # 2. CRLF vs LF
    if c_strip.replace(b"\r\n", b"\n") == r_strip.replace(b"\r\n", b"\n"):
        notes.append(
            "DIFFERENCE IS ONLY LINE ENDINGS: one file uses CRLF, the other LF. "
            "The reference emits LF."
        )
        return notes

    # 3. BOM
    if c.startswith(b"\xef\xbb\xbf") != r.startswith(b"\xef\xbb\xbf"):
        notes.append(
            "DIFFERENCE IS A LEADING BYTE-ORDER MARK: "
            f"candidate has BOM={c.startswith(b'\xef\xbb\xbf')}, reference has BOM={r.startswith(b'\xef\xbb\xbf')}. "
            "The reference emits no BOM (or does — match it exactly)."
        )

    # 4. line count
    cl, rl = split_lines(c), split_lines(r)
    if len(cl) != len(rl):
        notes.append(
            f"DIFFERENT LINE COUNTS: candidate {len(cl)} lines, reference {len(rl)} lines "
            f"(difference {len(cl) - len(rl):+d})."
        )
        # show the first line index where they diverge by position
        for i, (a, b) in enumerate(zip(cl, rl)):
            if a != b:
                notes.append(f"  first differing line index: {i} (line {i + 1})")
                break
        else:
            i = min(len(cl), len(rl))
            notes.append(f"  the first {i} lines are identical; the divergence is in the tail.")
    else:
        diff_idx = [i for i, (a, b) in enumerate(zip(cl, rl)) if a != b]
        notes.append(
            f"SAME LINE COUNT ({len(cl)}), but {len(diff_idx)} line(s) differ; "
            f"first at line {diff_idx[0] + 1}." if diff_idx else "same line count"
        )

    # 5. trailing whitespace
    if [x.rstrip() for x in cl] == [x.rstrip() for x in rl]:
        notes.append(
            "DIFFERENCE IS ONLY TRAILING WHITESPACE: the values agree once trailing "
            "spaces/tabs are stripped. Something is padding fields."
        )
        return notes

    # 6. whitespace runs
    if [b" ".join(x.split()) for x in cl] == [b" ".join(x.split()) for x in rl]:
        notes.append(
            "DIFFERENCE IS ONLY WHITESPACE RUNS: the tokens agree once runs of spaces "
            "are collapsed. The separator or the padding differs."
        )
        return notes

    # 7. float formatting
    if len(cl) == len(rl):
        floatish = 0
        for a, b in zip(cl, rl):
            if a == b:
                continue
            try:
                fa, fb = float(a), float(b)
            except ValueError:
                continue
            if fa == fb or (fa == 0 and fb == 0):
                floatish += 1
            elif abs(fa - fb) <= 1e-9 * max(1.0, abs(fa)):
                floatish += 1
        if floatish and floatish == len(cl) - sum(1 for a, b in zip(cl, rl) if a == b):
            notes.append(
                "THE VALUES ARE NUMERICALLY EQUAL but formatted differently: "
                f"{floatish} differing line(s) parse to the same number to within 1e-9. "
                "The reference uses a specific float format — copy it exactly "
                "(this is usually a precision or a rounding-mode difference)."
            )

    # 8. ordering
    if sorted(cl) == sorted(rl):
        notes.append(
            "THE SAME LINES ARE PRESENT BUT IN A DIFFERENT ORDER. "
            "The output ordering is part of the contract — the sort key and "
            "the tie-break are both specified."
        )
    return notes


def main() -> int:
    ap = argparse.ArgumentParser(description="Byte-level differential tester for the correctness gate")
    ap.add_argument("candidate", type=Path)
    ap.add_argument("reference", nargs="?", type=Path)
    ap.add_argument("--expect", help="expected SHA-256 instead of a reference file")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--cols", type=int, default=0,
                    help="treat the output as N column-separated and show a column-level diff")
    ap.add_argument("--max-lines", type=int, default=20,
                    help="how many differing lines to print (default 20)")
    args = ap.parse_args()

    if not args.candidate.is_file():
        print(f"candidate not found: {args.candidate}", file=sys.stderr)
        return 2

    cand = args.candidate.read_bytes()
    cand_hash = sha256_file(args.candidate)

    if args.expect:
        ok = cand_hash.lower() == args.expect.lower()
        if args.json:
            print(json.dumps({"candidate": str(args.candidate), "sha256": cand_hash,
                              "expected": args.expect, "match": ok}, indent=2))
        else:
            print(f"candidate : {args.candidate}")
            print(f"  sha256  : {cand_hash}")
            print(f"  expected : {args.expect}")
            print(f"  RESULT   : {'PASS' if ok else 'FAIL'}")
        return 0 if ok else 1

    if args.reference is None:
        print("need either a reference file or --expect <sha256>", file=sys.stderr)
        return 2
    if not args.reference.is_file():
        print(f"reference not found: {args.reference}", file=sys.stderr)
        return 2

    ref = args.reference.read_bytes()
    ref_hash = sha256_file(args.reference)
    identical = cand == ref

    notes = diagnose(cand, ref)
    result = {
        "candidate": str(args.candidate),
        "reference": str(args.reference),
        "candidate_sha256": cand_hash,
        "reference_sha256": ref_hash,
        "identical": identical,
        "candidate_bytes": len(cand),
        "reference_bytes": len(ref),
        "notes": notes,
    }

    if not identical:
        # First differing byte.
        n = min(len(cand), len(ref))
        off = next((i for i in range(n) if cand[i] != ref[i]), n)
        line, col = line_of_offset(cand, off)
        result["first_diff"] = {
            "offset": off,
            "line": line,
            "column": col,
            "candidate_byte": cand[off] if off < len(cand) else None,
            "reference_byte": ref[off] if off < len(ref) else None,
        }
        cl, rl = split_lines(cand), split_lines(ref)
        diffs = [(i, a, b) for i, (a, b) in enumerate(zip(cl, rl)) if a != b]
        result["differing_line_count"] = len(diffs)
        result["sample_lines"] = [
            {"index": i, "line": i + 1,
             "candidate": a.decode("utf-8", "replace"),
             "reference": b.decode("utf-8", "replace")}
            for i, a, b in diffs[: args.max_lines]
        ]
        if args.cols > 0:
            result["column_diffs"] = []
            for i, a, b in diffs[: args.max_lines]:
                ac = a.decode("utf-8", "replace").split(",")
                bc = b.decode("utf-8", "replace").split(",")
                cd = []
                for j in range(max(len(ac), len(bc))):
                    av = ac[j] if j < len(ac) else None
                    bv = bc[j] if j < len(bc) else None
                    if av != bv:
                        cd.append({"column": j, "candidate": av, "reference": bv})
                result["column_diffs"].append({"line": i + 1, "columns": cd})

    if args.json:
        print(json.dumps(result, indent=2))
        return 0 if identical else 1

    print("=" * 72)
    print("DIFFTEST")
    print("=" * 72)
    print(f"  candidate : {args.candidate}  ({len(cand)} bytes)")
    print(f"    sha256  : {cand_hash}")
    print(f"  reference : {args.reference}  ({len(ref)} bytes)")
    print(f"    sha256  : {ref_hash}")
    if identical:
        print("\n  RESULT: IDENTICAL")
        return 0

    print("\n  RESULT: DIFFERENT")
    fd = result.get("first_diff", {})
    print(f"\n  first difference at byte {fd.get('offset')} "
          f"(line {fd.get('line')}, column {fd.get('column')})")
    cb, rb = fd.get("candidate_byte"), fd.get("reference_byte")
    print(f"    candidate byte: {cb} ({chr(cb) if cb is not None and 32 <= (cb or 0) < 127 else '-'})")
    print(f"    reference byte: {rb} ({chr(rb) if rb is not None and 32 <= (rb or 0) < 127 else '-'})")

    lo = max(0, fd.get("offset", 0) - 16)
    print("\n  candidate bytes:")
    print(hexdump(cand, lo, min(len(cand), lo + 48)))
    print("  reference bytes:")
    print(hexdump(ref, lo, min(len(ref), lo + 48)))

    print("\n  DIAGNOSIS")
    for n in notes:
        print(f"    - {n}")

    samples = result.get("sample_lines", [])
    if samples:
        print(f"\n  DIFFERING LINES ({result.get('differing_line_count')} total, showing {len(samples)})")
        for s in samples:
            print(f"    line {s['line']}:")
            print(f"      candidate: {s['candidate'][:160]}")
            print(f"      reference: {s['reference'][:160]}")
            if args.cols > 0:
                for cd in result.get("column_diffs", []):
                    if cd["line"] == s["line"]:
                        for c in cd["columns"]:
                            print(f"        col {c['column']}: candidate={c['candidate']!r} reference={c['reference']!r}")

    print("\n  REMEMBER: the gate is a SHA-256 match, not 'close enough'. "
          "Near-misses score zero.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
