#!/usr/bin/env python3
"""Regenerate the per-track Dockerfiles from the template.

The five per-track Dockerfiles used to be hand-written, and they drifted:
every one of them referenced `/app/build.sh` and `/app/run` while copying
the *track* directory as the build context, so neither file was ever in the
image and the build failed at the first `chmod`.

They are generated from `tooling/templates/Dockerfile.track.tpl` now, and
`tooling/ci.sh` checks that the generated files still match the template.
A hand-edited Dockerfile fails the CI step instead of failing the build on
the scoring box.

Usage:
    python tooling/gen_dockerfiles.py          # regenerate
    python tooling/gen_dockerfiles.py --check  # fail if out of date
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "tooling" / "templates" / "Dockerfile.track.tpl"

TRACKS = [
    ("A", "A-ingest"),
    ("B", "B-index"),
    ("C", "C-join"),
    ("D", "D-traverse"),
    ("E", "E-compress"),
]


def render(template: str, track: str, directory: str) -> str:
    return template.replace("{{TRACK}}", track).replace("{{DIR}}", directory)


def main() -> int:
    ap = argparse.ArgumentParser(description="regenerate the per-track Dockerfiles")
    ap.add_argument("--check", action="store_true",
                    help="do not write; exit non-zero if any file is out of date")
    args = ap.parse_args()

    if not TEMPLATE.is_file():
        print(f"missing template: {TEMPLATE}", file=sys.stderr)
        return 2
    template = TEMPLATE.read_text(encoding="utf-8")

    stale = []
    for track, directory in TRACKS:
        path = ROOT / "tracks" / directory / "Dockerfile"
        wanted = render(template, track, directory)
        current = path.read_text(encoding="utf-8") if path.is_file() else None
        if current == wanted:
            print(f"  up to date: {path.relative_to(ROOT)}")
            continue
        stale.append(path)
        if args.check:
            print(f"  OUT OF DATE: {path.relative_to(ROOT)}")
        else:
            path.write_text(wanted, encoding="utf-8", newline="\n")
            print(f"  regenerated: {path.relative_to(ROOT)}")

    if args.check and stale:
        print(f"{len(stale)} per-track Dockerfile(s) out of date — "
              f"run: python tooling/gen_dockerfiles.py")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
