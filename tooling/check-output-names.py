#!/usr/bin/env python3
"""Cross-check the OUTPUT/INPUT contract across every source of truth.

The correctness gate is a SHA-256 of ONE file with an exact name. A rename that
lands in one place but not another (a scaffold constant, the harness table, a
pin filename, the official check.py) is a guaranteed zero on the board, and it
is invisible to every other gate — the binary still runs, the pin still
"matches" its own filename, CI still passes.

This gate makes the four sources agree, per track:

  1. the organizers' own check.py                OUTPUT = {"a": "summary.txt", ...}
  2. the scaffold constant                       track-*/src/main.rs DEFAULT_OUTPUT
  3. the harness contract table                  verification/harness/src/contract.rs
  4. the pinned expected-hash filename           track-*/verification/expected/<name>.sha256

Input filenames are checked the same way where they are stated (scaffold
DEFAULT_INPUT vs the harness `input:` field).

Exit 0 all agree, 1 a mismatch, 2 the check itself could not run.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
TRACKS = ("a", "b", "c", "d")
TRACK_ID = {"a": "A", "b": "B", "c": "C", "d": "D"}


def from_check_py() -> dict[str, str]:
    """The official check.py OUTPUT dict — the authority."""
    p = ROOT / "check.py"
    if not p.is_file():
        raise SystemExit("check.py is missing (copied from the starter)")
    text = p.read_text(encoding="utf-8", errors="replace")
    m = re.search(r"OUTPUT\s*=\s*\{([^}]*)\}", text)
    if not m:
        raise SystemExit("check.py has no OUTPUT dict to read")
    out = {}
    for k, v in re.findall(r'"([a-d])"\s*:\s*"([^"]+)"', m.group(1)):
        out[k] = v
    if set(out) != set(TRACKS):
        raise SystemExit(f"check.py OUTPUT covers {sorted(out)}, expected a..d")
    return out


def from_scaffold(track: str) -> tuple[str, str]:
    p = ROOT / f"track-{track}" / "src" / "main.rs"
    if not p.is_file():
        return "", ""
    text = p.read_text(encoding="utf-8", errors="replace")
    i = re.search(r'DEFAULT_INPUT\s*:\s*&str\s*=\s*"([^"]+)"', text)
    o = re.search(r'DEFAULT_OUTPUT\s*:\s*&str\s*=\s*"([^"]+)"', text)
    return (i.group(1) if i else ""), (o.group(1) if o else "")


def from_harness() -> dict[str, tuple[str, str]]:
    p = ROOT / "verification" / "harness" / "src" / "contract.rs"
    text = p.read_text(encoding="utf-8", errors="replace")
    out: dict[str, tuple[str, str]] = {}
    for block in re.finditer(r"m\.insert\(\s*'([A-D])'.*?\n\s*\},?\s*\);", text, re.S):
        tid = block.group(1)
        body = block.group(0)
        i = re.search(r'input:\s*"([^"]+)"', body)
        o = re.search(r'output:\s*"([^"]+)"', body)
        out[tid] = (i.group(1) if i else "", o.group(1) if o else "")
    return out


def from_pins(track: str) -> list[str]:
    d = ROOT / f"track-{track}" / "verification" / "expected"
    return sorted(p.name for p in d.glob("*.sha256")) if d.is_dir() else []


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args(argv)

    official = from_check_py()
    harness = from_harness()
    failures: list[str] = []
    ok = 0

    for t in TRACKS:
        tid = TRACK_ID[t]
        want = official[t]
        sc_in, sc_out = from_scaffold(t)
        h_in, h_out = harness.get(tid, ("", ""))
        pins = from_pins(t)

        if sc_out == want:
            ok += 1
        else:
            failures.append(f"track-{t}: scaffold DEFAULT_OUTPUT={sc_out!r} != check.py {want!r}")
        if h_out == want:
            ok += 1
        else:
            failures.append(f"track-{t}: harness output={h_out!r} != check.py {want!r}")
        if pins:
            if pins == [f"{want}.sha256"]:
                ok += 1
            else:
                failures.append(
                    f"track-{t}: pin file(s) {pins} != ['{want}.sha256']"
                )
        else:
            failures.append(f"track-{t}: no pinned expected hash for {want}")
        # input name parity where both sides state it
        if sc_in and h_in and sc_in != h_in:
            failures.append(f"track-{t}: scaffold input={sc_in!r} != harness input={h_in!r}")
        elif sc_in and h_in:
            ok += 1

    if not args.quiet:
        print("check-output-names: output/input contract parity")
        for t in TRACKS:
            tid = TRACK_ID[t]
            print(f"  track-{t}  output={official[t]:<15} scaffold={from_scaffold(t)[1]:<15} "
                  f"harness={harness.get(tid, ('',''))[1]:<15} pin={','.join(from_pins(t)) or '-'}")
        print(f"  {ok} agreement(s), {len(failures)} failure(s)")
        print()
    if failures:
        for f in failures:
            print(f"  FAIL  {f}", file=sys.stderr)
        print(f"check-output-names: {len(failures)} MISMATCH(ES)", file=sys.stderr)
        return 1
    print("check-output-names: every source agrees with the official check.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())