#!/usr/bin/env python3
"""One-off helper: add UTF-8 stdio setup to every Python tool in tooling/.

Why this exists: on Windows the default stdout code page is cp1252, so any
non-ASCII character a tool prints becomes mojibake in the log. The tools
print em-dashes and box-drawing characters in their reports, which means
their own output was the largest remaining source of encoding corruption in
the repository. Every tool now forces UTF-8 stdio at startup.
"""

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


def main() -> int:
    tools = sorted(Path("tooling").glob("*.py"))
    changed = 0
    for p in tools:
        s = p.read_text(encoding="utf-8")
        if "sys.stdout.reconfigure" in s:
            print(f"  already patched: {p.name}")
            continue
        imports = list(re.finditer(r"^(?:import .*|from .* import .*)$", s, re.M))
        if not imports:
            print(f"  NO IMPORTS, skipped: {p.name}")
            continue
        idx = imports[-1].end()
        new = s[:idx] + SNIPPET + s[idx:]
        if not re.search(r"^import sys$", new, re.M):
            new = "import sys\n" + new
        p.write_text(new, encoding="utf-8", newline="\n")
        print(f"  patched: {p.name}")
        changed += 1
    print(f"{changed} tool(s) patched")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
