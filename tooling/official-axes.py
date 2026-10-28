#!/usr/bin/env python3
"""official-axes — run the ORGANIZERS' D measurement on our track folders.

`reference/official/harness/dependencies.py` is the authority on D. This wrapper
calls it on all four folders and prints the basis, so a drift between our
declaration (.speed-demon.toml `dependency_count = 0`, no manifest) and their
measurement fails the gate instead of being discovered at scoring time.
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "reference" / "official" / "harness"))
import dependencies as d  # noqa: E402

bad = 0
for t in "abcd":
    td = ROOT / f"track-{t}"
    decl = {}
    tomlp = td / ".speed-demon.toml"
    if tomlp.is_file():
        try:
            import tomllib
            decl = tomllib.loads(tomlp.read_text(encoding="utf-8"))
        except Exception as exc:
            print(f"  track-{t}: cannot read {tomlp.name}: {exc}")
            bad += 1
            continue
    scan = d.find_direct_dependencies(str(td))
    weight, basis, notes = d.dependency_weight(scan, decl)
    deps = [x.name for x in getattr(scan, "dependencies", [])]
    print(f"  track-{t}: D = {weight}  basis = {basis}  deps = {deps or 'none'}"
          + (f"  notes = {notes}" if notes else ""))
    if weight != 1:
        bad += 1
sys.exit(1 if bad else 0)