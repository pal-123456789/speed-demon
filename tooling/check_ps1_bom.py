#!/usr/bin/env python3
"""
Gate: every .ps1 in the tree must be UTF-8 *and* start with a UTF-8 BOM.

Windows PowerShell 5.1 decodes a .ps1 without a BOM using the system ANSI code
page. Any non-ASCII character (an em dash, a degree sign) then mis-decodes, and
because CP1252 maps one UTF-8 em-dash continuation byte to U+201D, which the
tokenizer treats as a string terminator, the whole file fails to parse with a
misleading "missing terminator" error. `verification/bench.ps1` and
`verification/pre-freeze.ps1` shipped that way and could not run at all.

This check is wired into `tooling/ci.sh`, so the encoding cannot regress
silently the next time someone edits a script with an editor that drops the BOM.

Usage:  python tooling/check_ps1_bom.py [file ...]
        (with no arguments, every .ps1 under the repo root is checked)
"""

import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
BOM = b"\xef\xbb\xbf"
SKIP_DIRS = {".git", "target", "node_modules"}


def ps1_files() -> list[Path]:
    return sorted(
        p for p in ROOT.rglob("*.ps1") if not SKIP_DIRS & set(p.relative_to(ROOT).parts)
    )


def main(argv: list[str]) -> int:
    files = [Path(a) for a in argv] or ps1_files()
    if not files:
        print("  FAIL: no .ps1 files found to check")
        return 1

    bad = 0
    for f in files:
        raw = f.read_bytes()
        has_bom = raw.startswith(BOM)
        try:
            (raw[len(BOM):] if has_bom else raw).decode("utf-8")
            utf8 = True
        except UnicodeDecodeError:
            utf8 = False
        nonascii = sum(1 for b in raw if b > 127)
        ok = has_bom and utf8
        print(
            f"  {'ok  ' if ok else 'FAIL'}: {f}  "
            f"BOM={has_bom} utf8={utf8} non_ascii_bytes={nonascii}"
        )
        if not ok:
            bad += 1

    if bad:
        print(f"  {bad} file(s) cannot run on Windows PowerShell 5.1")
        print("  fix: python tooling/fix_ps1_bom.py")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
