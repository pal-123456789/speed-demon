"""Verify two audit claims:
1. ledger.jsonl has unparseable lines (unescaped JSON from ledger.rs).
2. gen_docs.py divides peak_mib by 1024*1024 a second time.
"""

import io
import json
import sys

print("=== CLAIM 1: ledger.jsonl parseability ===")
path = "verification/ledger.jsonl"
raw = io.open(path, encoding="utf-8").read()
lines = [ln for ln in raw.splitlines() if ln.strip()]
ok = 0
bad = []
for i, ln in enumerate(lines, 1):
    try:
        json.loads(ln)
        ok += 1
    except json.JSONDecodeError as e:
        bad.append((i, str(e), ln))
print(f"  total non-empty lines: {len(lines)}")
print(f"  parseable            : {ok}")
print(f"  MALFORMED            : {len(bad)}")
for i, err, ln in bad:
    print(f"    line {i}: {err}")
    # find the offending region
    col = err.split("column ")[-1] if "column" in err else "?"
    print(f"      ...{ln[max(0, int(col) - 40):int(col) + 20] if col.isdigit() else ln[:80]}...")

print()
print("=== CLAIM 2: gen_docs.py peak_mib handling ===")
src = io.open("tooling/gen_docs.py", encoding="utf-8").read()
for n, ln in enumerate(src.splitlines(), 1):
    if "1024" in ln or "peak_mib" in ln or "peaks" in ln:
        print(f"  L{n}: {ln.strip()}")
sys.exit(1 if bad else 0)
