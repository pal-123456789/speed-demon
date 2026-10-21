#!/usr/bin/env python3
"""
Repair double-encoded UTF-8 (mojibake) in text files.

A PowerShell round-trip (Get-Content -Raw | Set-Content -Encoding utf8) on a
BOM-less UTF-8 file decodes the bytes as Windows-1252, which turns a UTF-8
em-dash (E2 80 94) into three characters, and then re-encodes those three
characters as UTF-8. The result is "â€”" instead of "—".

The repair is the inverse: decode the text as cp1252 to recover the original
UTF-8 bytes, then decode those bytes as UTF-8. It is safe to attempt because
the operation only succeeds on text that is genuinely double-encoded —
single-encoded text fails the decode and is left untouched.

Usage:  python tooling/fix_encoding.py FILE [FILE ...]
"""

import sys
from pathlib import Path


def repair(text: str) -> tuple[str, bool]:
    # A BOM is not part of the mojibake and is not representable in cp1252,
    # so strip it before the byte-level round trip and restore it after.
    had_bom = text.startswith("\ufeff")
    if had_bom:
        text = text[1:]

    fixed = None
    for enc in ("cp1252", "latin-1"):
        try:
            candidate = text.encode(enc).decode("utf-8")
        except (UnicodeEncodeError, UnicodeDecodeError):
            continue
        # Only accept the repair if it actually reduced mojibake markers.
        markers = ("â", "Ã", "Â", "€")
        before = sum(text.count(m) for m in markers)
        after = sum(candidate.count(m) for m in markers)
        if after < before:
            fixed = candidate
            break

    if fixed is None:
        return text, False
    if had_bom:
        fixed = "\ufeff" + fixed
    return fixed, True


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    changed = 0
    for arg in sys.argv[1:]:
        p = Path(arg)
        raw = p.read_bytes()
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            print(f"  skip (not utf-8): {p}")
            continue
        fixed, ok = repair(text)
        if ok:
            p.write_text(fixed, encoding="utf-8", newline="")
            print(f"  repaired: {p}")
            changed += 1
        else:
            print(f"  clean   : {p}")
    print(f"{changed} file(s) repaired")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
