#!/usr/bin/env python3
"""Locate a POSIX shell on this host, portably.

Several tools need to run a track's `build.sh` (and other shell scripts). On
Linux `/bin/sh` and `sh` are always there; on Windows `sh` usually exists only
inside a Git for Windows installation and is NOT on PATH, so a bare
`subprocess.run(["sh", ...])` fails with WinError 2 — which is exactly what
happened the first time the documented H0 sequence was run from PowerShell.

Resolution order:
  1. $SD_SH                     (explicit override)
  2. sh / bash on PATH
  3. the standard Git for Windows install locations
  4. the shell running this process, when it is already a POSIX shell

`find_shell()` returns the path, or raises with an actionable message.
"""

from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path

_CANDIDATES = (
    r"C:\Program Files\Git\bin\sh.exe",
    r"C:\Program Files (x86)\Git\bin\sh.exe",
    r"C:\Program Files\Git\usr\bin\sh.exe",
)


def find_shell(required: bool = True) -> str | None:
    env = os.environ.get("SD_SH")
    if env and Path(env).is_file():
        return env
    if os.name == "nt":
        # On Windows prefer Git for Windows' sh: it handles native paths
        # (D:\...) directly. A bare `bash` on PATH is usually WSL's bash.exe,
        # which needs /mnt/d/... paths and may fail to start a session at all
        # (observed: "Failed to start the systemd user session").
        for cand in _CANDIDATES:
            if Path(cand).is_file():
                return cand
        p = shutil.which("sh")
        if p:
            return p
    else:
        for name in ("sh", "bash"):
            p = shutil.which(name)
            if p:
                return p
        for cand in ("/bin/sh", "/usr/bin/sh"):
            if Path(cand).is_file():
                return cand
    if os.name == "nt":
        p = shutil.which("bash")          # last resort: may be WSL
        if p:
            return p
    if required:
        raise SystemExit(
            "no POSIX shell found (needed to run build.sh).\n"
            "  fix: install Git for Windows, or set SD_SH to a shell, e.g.\n"
            r'       $env:SD_SH = "C:\Program Files\Git\bin\sh.exe"'
        )
    return None


def shell() -> str:
    """The shell path, or a clear error."""
    found = find_shell(required=True)
    assert found is not None
    return found


def run_build(track: str, root: Path) -> tuple[int, str]:
    """Run track-<t>/build.sh with the discovered shell. Returns (rc, stderr)."""
    import subprocess

    sh = shell()
    script = f"track-{track}/build.sh"
    p = subprocess.run([sh, script], cwd=root, capture_output=True, text=True)
    return p.returncode, (p.stderr or "").strip()


if __name__ == "__main__":
    print(shell())
