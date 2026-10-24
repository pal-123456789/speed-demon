#!/usr/bin/env python3
"""Repair double-encoded UTF-8 (mojibake) in text files.

The failure mode
----------------
A PowerShell round-trip on a BOM-less UTF-8 file
(`Get-Content -Raw | Set-Content -Encoding utf8`) decodes the bytes as
Windows-1252, turning a UTF-8 em-dash (E2 80 94) into three characters,
then re-encodes those three characters as UTF-8. The result is a file
whose bytes are valid UTF-8 but whose *text* is wrong.

Doing it twice is not hypothetical: a repair script that itself round-tripped
through PowerShell would encode it a second time. This repository has
contained files at two levels of nesting, so the repair below is iterative.

The repair
----------
The inverse of the damage: decode the text as cp1252 to recover the original
bytes, then decode those bytes as UTF-8. Each pass is only accepted if it
strictly reduces the number of mojibake markers, so a correctly-encoded file
is left untouched and the loop terminates.

Why it is safe
--------------
The markers are written here as \\u escapes so that THIS file is pure ASCII
and contains no literal mojibake of its own. That matters: a repair tool
that cannot distinguish "text that is damaged" from "text that talks about
damage" is a repair tool that eventually corrupts a good file.

Usage
-----
    python tooling/fix_encoding.py FILE [FILE ...]     repair in place
    python tooling/fix_encoding.py --scan .            report only, no writes
    python tooling/fix_encoding.py --scan . --json     machine-readable
"""

import json
import sys
from pathlib import Path
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


# The mojibake markers, as escapes so this file stays pure ASCII.
MARKERS = ("\u00e2", "\u00c3", "\u00c2", "\u20ac", "\u00ef\u00bf\u00bd")

# Text files worth scanning.
SUFFIXES = {".rs", ".py", ".sh", ".md", ".toml", ".txt", ".json", ".yml", ".yaml", ""}
SKIP_DIRS = {".git", "target", "vendor", "node_modules", "__pycache__"}


def marker_count(text: str) -> int:
    return sum(text.count(m) for m in MARKERS)


def repair_once(text: str) -> tuple[str, bool]:
    """One inverse round trip, accepted only if it reduces the markers."""
    had_bom = text.startswith("\ufeff")
    if had_bom:
        text = text[1:]

    fixed = None
    for enc in ("cp1252", "latin-1"):
        try:
            candidate = text.encode(enc).decode("utf-8")
        except (UnicodeEncodeError, UnicodeDecodeError):
            continue
        if marker_count(candidate) < marker_count(text):
            fixed = candidate
            break

    if fixed is None:
        return text, False
    if had_bom:
        fixed = "\ufeff" + fixed
    return fixed, True


def repair(text: str, max_passes: int = 4) -> tuple[str, int]:
    """Repair until it stops improving. Returns (text, passes applied)."""
    passes = 0
    current = text
    for _ in range(max_passes):
        candidate, ok = repair_once(current)
        if not ok:
            break
        current = candidate
        passes += 1
    return current, passes


def should_scan(path: Path) -> bool:
    if path.suffix.lower() not in SUFFIXES:
        return False
    return not any(part in SKIP_DIRS for part in path.parts)


def iter_files(targets):
    for target in targets:
        p = Path(target)
        if p.is_file():
            yield p
        elif p.is_dir():
            for child in sorted(p.rglob("*")):
                if child.is_file() and should_scan(child):
                    yield child


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    scan_only = "--scan" in sys.argv
    as_json = "--json" in sys.argv

    if not args:
        print(__doc__)
        return 2

    findings = []
    for path in iter_files(args):
        raw = path.read_bytes()
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            continue
        before = marker_count(text)
        if before == 0:
            continue
        fixed, passes = repair(text)
        after = marker_count(fixed)
        if scan_only:
            findings.append({"file": str(path), "markers_before": before,
                             "markers_after": after, "passes": passes})
        else:
            path.write_text(fixed, encoding="utf-8", newline="")
            findings.append({"file": str(path), "markers_before": before,
                             "markers_after": after, "passes": passes,
                             "repaired": True})

    if as_json:
        print(json.dumps(findings, indent=2))
    else:
        for f in findings:
            if scan_only:
                print(f"  NEEDS REPAIR: {f['file']}  "
                      f"markers {f['markers_before']} -> {f['markers_after']} "
                      f"({f['passes']} pass(es))")
            else:
                print(f"  repaired: {f['file']}  "
                      f"markers {f['markers_before']} -> {f['markers_after']} "
                      f"({f['passes']} pass(es))")
        if not findings:
            print("  no mojibake found")
        print(f"{len(findings)} file(s) {'need repair' if scan_only else 'repaired'}")
    return 1 if (scan_only and findings) else 0


if __name__ == "__main__":
    raise SystemExit(main())
