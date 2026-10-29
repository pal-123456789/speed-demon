#!/usr/bin/env python3
"""Add the UTF-8 stdio setup to every Python tool in tooling/.

Why this exists: on Windows the default stdout code page is cp1252, so any
non-ASCII character a tool prints becomes mojibake in the log. The tools print
em-dashes and box-drawing characters in their reports, which makes their own
output the largest remaining source of encoding corruption in the repository.
Every tool should force UTF-8 stdio at startup, and
tooling/check-consistency.py asserts that they all do.

Correctness note, because this tool has already been wrong once. It used to find
the insertion point with a line regex:

    r"^(?:import .*|from .* import .*)$"

For a parenthesised multi-line import, that matches the FIRST line only --
`from k_calc import (  # noqa: E402` -- so the snippet was spliced INSIDE the
parentheses and the result did not parse. It silently rewrote
tooling/k_whatif.py into a SyntaxError. Two guards now prevent that:

  * the insertion point comes from the AST (`end_lineno` of the last top-level
    import), which is exact for multi-line imports; and
  * the patched source must parse before it is written at all.

Usage:  python tooling/add_utf8_stdio.py      (idempotent; exit 1 if refused)
"""

import ast
import re
from pathlib import Path

SNIPPET = (
    "\n"
    "# Force UTF-8 on stdio. On Windows the default code page mangles every\n"
    "# non-ASCII character this tool prints, which corrupts logs and makes\n"
    "# mojibake indistinguishable from real output.\n"
    'if hasattr(sys.stdout, "reconfigure"):\n'
    '    sys.stdout.reconfigure(encoding="utf-8", errors="replace")\n'
    '    sys.stderr.reconfigure(encoding="utf-8", errors="replace")\n'
)


def insertion_point(src: str):
    """Character offset just after the leading block of import statements.

    Returns None when the file has no leading import block (nothing sensible to
    anchor to) or does not parse.
    """
    try:
        tree = ast.parse(src)
    except SyntaxError:
        return None
    last = None
    for node in tree.body:
        if isinstance(node, (ast.Import, ast.ImportFrom)):
            last = node
        elif last is not None:
            break  # only a LEADING run of imports counts
    if last is None:
        return None
    lines = src.splitlines(keepends=True)
    return sum(len(line) for line in lines[: last.end_lineno])


def main() -> int:
    tools = sorted(Path("tooling").glob("*.py"))
    changed = refused = 0
    for p in tools:
        s = p.read_text(encoding="utf-8")
        if "sys.stdout.reconfigure" in s:
            print(f"  already patched: {p.name}")
            continue
        idx = insertion_point(s)
        if idx is None:
            print(f"  no leading import block, skipped: {p.name}")
            continue
        new = s[:idx] + SNIPPET + s[idx:]
        if not re.search(r"^import sys$", new, re.M):
            new = "import sys\n" + new
        # Never write a file that does not parse. See the module docstring.
        try:
            ast.parse(new)
        except SyntaxError as exc:
            print(f"  REFUSED (would not parse): {p.name} — {exc.msg} at line {exc.lineno}")
            refused += 1
            continue
        p.write_text(new, encoding="utf-8", newline="\n")
        print(f"  patched: {p.name}")
        changed += 1
    tail = f", {refused} refused" if refused else ""
    print(f"{changed} tool(s) patched{tail}")
    return 1 if refused else 0


if __name__ == "__main__":
    raise SystemExit(main())
