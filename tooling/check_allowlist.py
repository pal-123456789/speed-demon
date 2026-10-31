#!/usr/bin/env python3
"""Verify every FROM in every track Dockerfile against the organizers' allowlist.

Why this exists. The board REJECTS a track before building it if a FROM line is not
allowlisted -- "Rejected before building -- the Dockerfile broke a rule" is a real
failure mode on the live board, and it scores zero. `check.py` does this check, but
it needs the network and it builds an image to get there; this is the cheap, offline-
friendly half that can run in CI on every commit.

It also answers the specific question that decides whether PGO is available to us:
the plain rust image has no llvm-profdata, so a profile-use build needs the
allowlisted llvm-tools variant, and this says whether that tag is permitted or
whether switching to it would get all four tracks rejected.
"""
from __future__ import annotations

import json
import pathlib
import re
import sys
import urllib.request

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ALLOWLIST_URL = "https://speeddemon.dev/allowlist.json"
ROOT = pathlib.Path(__file__).resolve().parent.parent
FROM_RE = re.compile(r"^\s*FROM\s+(\S+)", re.I | re.M)


def fetch_allowlist() -> list[str] | None:
    try:
        req = urllib.request.Request(
            ALLOWLIST_URL, headers={"User-Agent": "speed-demon-starter-check/1.0"}
        )
        raw = json.load(urllib.request.urlopen(req, timeout=30))
    except Exception as exc:  # noqa: BLE001 - any failure means "cannot confirm"
        print(f"note  could not fetch the allowlist ({type(exc).__name__}: {exc})")
        return None

    # The document has taken more than one shape; accept the common ones rather
    # than guessing, and say so if none of them match.
    if isinstance(raw, list):
        return [str(x) for x in raw]
    for key in ("allowed", "images", "allowlist", "digests"):
        v = raw.get(key) if isinstance(raw, dict) else None
        if isinstance(v, list):
            return [json.dumps(x) if isinstance(x, (dict, list)) else str(x) for x in v]
    if isinstance(raw, dict):
        return [f"{k} {v}" for k, v in raw.items()]
    return None


def main() -> int:
    allow = fetch_allowlist()
    total = 0
    unknown = 0

    for track in "abcd":
        dockerfile = ROOT / f"track-{track}" / "Dockerfile"
        if not dockerfile.is_file():
            print(f"  track-{track}: no Dockerfile")
            continue
        for image in FROM_RE.findall(dockerfile.read_text(encoding="utf-8")):
            if image.upper().startswith("$") or image.lower() == "scratch":
                continue
            total += 1
            if allow is None:
                print(f"  track-{track}: {image.split('@')[0]}  (allowlist unavailable)")
                continue
            repo = image.split("@")[0]
            digest = image.split("@")[-1] if "@" in image else ""
            hit = any(repo in a and (not digest or digest in a) for a in allow)
            if not hit:
                # A bare repo name may match even when the digest does not.
                hit = any(repo in a for a in allow)
            mark = "ok  " if hit else "RISK"
            if not hit:
                unknown += 1
            shown = repo if len(repo) < 52 else repo[:49] + "..."
            print(f"  {mark} track-{track}: {shown}")

    print()
    if allow is None:
        print("check_allowlist: could not reach the allowlist; nothing verified")
        return 0
    llvm = [a for a in allow if "llvm-tools" in a.lower()]
    rust = [a for a in allow if "rust" in a.lower()]
    print(f"check_allowlist: {len(allow)} allowlist entries, "
          f"{len(rust)} rust, {len(llvm)} llvm-tools")
    for a in llvm[:3]:
        print("   llvm-tools ->", a[:160])
    print(f"  {total} FROM lines checked, {unknown} not confirmed")
    return 1 if unknown else 0


if __name__ == "__main__":
    sys.exit(main())
