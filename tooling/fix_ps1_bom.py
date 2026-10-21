#!/usr/bin/env python3
"""
Make the PowerShell scripts safe for Windows PowerShell 5.1.

Windows PowerShell 5.1 (which is what `powershell.exe` is, and this machine
has no `pwsh`) reads a .ps1 file as ANSI unless the file starts with a UTF-8
BOM. A script containing a non-ASCII character — an em dash in a comment, a
degree sign, a box-drawing character — is then decoded with the wrong code
page and the parser fails with a confusing "missing terminator" error.

PowerShell 7 (`pwsh`) defaults to UTF-8 and does not need the BOM, but the
BOM is harmless there.

This script adds a BOM to every .ps1 file that does not already have one.

Usage:  python tooling/fix_ps1_bom.py
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BOM = b"\xef\xbb\xbf"


def main() -> int:
    changed = 0
    for p in sorted(ROOT.rglob("*.ps1")):
        raw = p.read_bytes()
        if raw.startswith(BOM):
            print(f"  has BOM : {p.relative_to(ROOT)}")
            continue
        p.write_bytes(BOM + raw)
        print(f"  added   : {p.relative_to(ROOT)}")
        changed += 1
    print(f"{changed} file(s) updated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
