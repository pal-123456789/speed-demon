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
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


ROOT = Path(__file__).resolve().parent.parent
BOM = b"\xef\xbb\xbf"


#: Never rewrite files under these trees: `.git` holds no working scripts and
#: `target/` is build output, so a blanket rglob would touch files that are
#: either not ours or not source.
SKIP_DIRS = {".git", "target", "node_modules"}


def main() -> int:
    changed = 0
    for p in sorted(ROOT.rglob("*.ps1")):
        if SKIP_DIRS & set(p.relative_to(ROOT).parts):
            continue
        raw = p.read_bytes()
        if raw.startswith(BOM):
            print(f"  has BOM : {p.relative_to(ROOT)}")
            continue
        # A BOM only helps if the bytes after it really are UTF-8. Blessing a
        # CP1252 file would turn every accented character into U+FFFD in both
        # PowerShell 5.1 and 7, which is worse than the unparseable state.
        try:
            raw.decode("utf-8")
        except UnicodeDecodeError as exc:
            print(f"  ERROR   : {p.relative_to(ROOT)} is not UTF-8: {exc}")
            return 1
        p.write_bytes(BOM + raw)
        print(f"  added   : {p.relative_to(ROOT)}")
        changed += 1
    print(f"{changed} file(s) updated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
