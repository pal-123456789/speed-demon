#!/usr/bin/env python3
"""mutate — inject known bugs and report how many the correctness apparatus catches.

Why this exists. "The tests pass" is a statement about the tests, not about the
code. The only way to know the gates work is to break something on purpose and
watch them fail. This runs the tiny conformance corpora, corrupts them in ways
that correspond to the specific bugs each track spec warns about, re-runs the
ORACLE on the corrupted data, and checks that the stored expected output no
longer matches.

A mutation is CAUGHT when the reference's answer on the mutated data differs from
the answer stored beside the pristine corpus. That is exactly the correctness
gate: if the two matched, the corpus would be blind to that bug.

TWO CONTROLS are included deliberately. `method` is ignored by Track A and a
mid-file BOM is skipped by Track C's rules, so those two mutations MUST NOT be
caught. A harness that reports 12 of 12 on mutations including those is a harness
that is not measuring anything -- it would be scoring "any change at all". Both
directions are asserted.

Every mutation is anchored to a unique byte string and the harness ERRORS if an
anchor is missing, so a corpus change cannot silently turn a mutation into a
no-op.

    python tooling/mutate.py
    python tooling/mutate.py --keep        # leave the mutants on disk to inspect
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "verification" / "cases"
OUTPUT = {"a": "summary.txt", "b": "results.txt", "c": "aggregates.txt", "d": "distances.txt"}

# (track, name, bug class, [(file, old, new)], must_be_caught)
MUTATIONS = [
    # ── Track A ────────────────────────────────────────────────────────────
    ("a", "status-5xx", "error count ignores 5xx",
     [("requests.log", b" 200 2254", b" 503 2254")], True),
    ("a", "endpoint-byte", "group key compared with normalisation",
     [("requests.log", b"GET /v3/shipments/{date} ", b"GET /v3/shipments/{date}Z ")], True),
    ("a", "hour-boundary", "hour derived by truncation vs first 13 bytes",
     [("requests.log", b"2026-10-30T18:00:00.000000Z GET /v3/shipments",
       b"2026-10-30T19:00:00.000000Z GET /v3/shipments")], True),
    ("a", "duplicate-line", "duplicate lines counted once",
     [("requests.log", b"2026-10-30T18:01:17.883516Z GET /v1/audit-logs/{order_id} 200 4902\n",
       b"2026-10-30T18:01:17.883516Z GET /v1/audit-logs/{order_id} 200 4902\n"
       b"2026-10-30T18:01:17.883516Z GET /v1/audit-logs/{order_id} 200 4902\n")], True),
    ("a", "method-control", "CONTROL: method is ignored by the spec",
     [("requests.log", b"PUT /internal/v1/disputes/{id}/balances", b"PATCH /internal/v1/disputes/{id}/balances")], False),

    # ── Track C ────────────────────────────────────────────────────────────
    ("c", "amount-digit", "cents parsed as float",
     [("records.csv", b",6832.30", b",6832.31")], True),
    ("c", "trailing-space", "trailing space trimmed from a name",
     [("records.csv", b'"Nomo Keta"', b'"Nomo Keta "')], True),
    ("c", "unknown-region", "unknown region_id silently grouped",
     [("records.csv", b",1639,", b",9000001,")], True),
    ("c", "id-sum", "id_sum from the wrong column",
     [("records.csv", b'1,"Sume, Sufo"', b'3,"Sume, Sufo"')], True),
    ("c", "bom-control", "CONTROL: a mid-file BOM is skipped either way",
     # The BOM sits immediately before an id and is not data, so removing it
     # cannot change the parsed value. If this is ever "caught", the mutation is
     # landing somewhere it should not.
     [("records.csv", b"\xef\xbb\xbf", b"")], False),

    # ── Track D ────────────────────────────────────────────────────────────
    ("d", "duplicate-query", "duplicate query lines collapsed",
     [("queries.txt", b"310 168\n", b"310 168\n310 168\n")], True),
    ("d", "weight-zeroed", "weights read as 0 or not read",
     [("graph.txt", b"4294967295", b"4294967294")], True),

    # ── Track B ────────────────────────────────────────────────────────────
    ("b", "doc-id-shift", "document ids assumed to start at 1",
     [("docs.txt", b"0\t", b"0\tThe.\n1\t")], True),
]


def run_oracle(track: str, data: Path, out: Path) -> tuple[bool, str]:
    r = subprocess.run(
        [sys.executable, str(ROOT / "tooling" / "conformance.py"),
         "--track", track, "--oracle", "--data", str(data), "--save-expected", str(out)],
        capture_output=True, text=True, cwd=ROOT,
    )
    return r.returncode == 0, (r.stdout + r.stderr)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--keep", action="store_true", help="leave the mutants on disk")
    args = ap.parse_args(argv)

    caught, missed, wrong_way = [], [], []
    print("mutate: injecting known bugs into the tiny corpora, then re-running the oracle\n")

    for track, name, bug, edits, must in MUTATIONS:
        src = CASES / f"track-{track}"
        tmp = Path(tempfile.mkdtemp(prefix=f"sd-mut-{track}-{name}-"))
        try:
            for f in src.iterdir():
                if f.is_file():
                    shutil.copy2(f, tmp / f.name)

            ok = True
            for fname, old, new in edits:
                p = tmp / fname
                raw = p.read_bytes()
                n = raw.count(old)
                if n == 0:
                    print(f"  ERROR {track}/{name}: anchor not found in {fname}: {old[:40]!r}")
                    print("        (the corpus changed; fix the anchor rather than deleting the mutation)")
                    ok = False
                    break
                p.write_bytes(raw.replace(old, new, 1))
            if not ok:
                wrong_way.append(f"{track}/{name} (anchor missing)")
                continue

            out = tmp / "_ref"
            out.mkdir()
            ran, log = run_oracle(track, tmp, out)
            produced = out / OUTPUT[track]
            pristine = (src / f"expected-{OUTPUT[track]}").read_bytes()

            if not ran or not produced.is_file():
                # The reference REFUSED the mutated data (raised). That is also
                # detection: an input outside the contract cannot be silently
                # mis-answered.
                detected, detail = True, "the oracle rejected the mutated data"
            else:
                detected = produced.read_bytes() != pristine
                detail = ("the reference's answer changed" if detected
                          else "the reference's answer is IDENTICAL (blind)")

            if must and not detected:
                missed.append(f"{track}/{name} — {bug}")
                print(f"  MISS  [{track}] {name:18} {bug}")
                print(f"        {detail}")
            elif not must and detected:
                wrong_way.append(f"{track}/{name} — control fired")
                print(f"  BAD   [{track}] {name:18} CONTROL should not be caught")
                print(f"        {detail}")
            else:
                caught.append(f"{track}/{name}")
                tag = "caught" if must else "control ok"
                print(f"  ok    [{track}] {name:18} {tag}: {bug}")
        finally:
            if not args.keep:
                shutil.rmtree(tmp, ignore_errors=True)
            else:
                print(f"        mutant kept at {tmp}")

    required = sum(1 for m in MUTATIONS if m[4])
    controls = len(MUTATIONS) - required
    print()
    print(f"mutate: {len(caught)}/{len(MUTATIONS)} behaved correctly "
          f"({required - len(missed)}/{required} bugs caught, {controls} controls held)")
    if missed:
        print("  UNCAUGHT bugs — each needs a new test:")
        for m in missed:
            print(f"    {m}")
    if wrong_way:
        print("  HARNESS PROBLEMS:")
        for m in wrong_way:
            print(f"    {m}")
    if missed or wrong_way:
        return 1
    print("mutate: every injected bug was caught, and neither control fired")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
