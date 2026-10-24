#!/usr/bin/env python3
"""Rule 05 guard — mechanically prevent a disqualification.

The hackathon rules say, in effect: *new code only. All project code is
written during the 72-hour window. Code committed before kickoff
disqualifies the submission.*

That is a rule about a **clock**, and clocks are exactly the thing a tired
participant forgets at hour 40. So this tool turns it into a pre-commit
check with a hard boundary:

  * Before H0 (Oct 30 2026 18:00 UTC) it scans the submission source for
    anything that looks like a *solution* to a track, and refuses the commit.
  * After H0 it stops scanning and instead reports that the window is open.

The scan is deliberately pattern-based and deliberately noisy about its own
limits: it cannot prove the absence of a solution, only the presence of one.
A false positive is cheap (allowlist it); a false negative is a
disqualification.

What counts as "solution code" here is any of the track-specific algorithms
that the rules clearly expect to be written inside the window:

  * a CSV field/quote state machine            (Track C)
  * an inverted index or posting lists         (Track B)
  * a graph traversal, Dijkstra/ALT/CH         (Track D)
  * an entropy coder (rANS/Huffman/LZ)          (Track E)
  * a percentile estimator                     (Track A)

What does NOT count, and is explicitly allowed: the harness, the tooling,
the verifier, the contract, the docs, and *generic* data structures in
`core/` (an arena, a byte scanner, a thread pool). Infrastructure and
design are what the rules explicitly permit before kickoff.

Usage:
    python tooling/rule05_guard.py            # check and report
    python tooling/rule05_guard.py --h0 2026-10-30T18:00:00Z
    python tooling/rule05_guard.py --list     # show the pattern set
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


REPO = Path(__file__).resolve().parent.parent

# The published kickoff. Overridable for rehearsals.
DEFAULT_H0 = "2026-10-30T18:00:00Z"

# The published freeze.
DEFAULT_FREEZE = "2026-11-02T18:00:00Z"


@dataclass
class Pattern:
    """One banned construct, with the reason it is banned."""

    name: str
    track: str
    regex: str
    why: str
    # Paths (relative to repo root) this pattern must NOT be scanned in.
    exempt_paths: tuple[str, ...] = ()
    flags: int = re.IGNORECASE


# Infrastructure and design are legal before kickoff. Only these directories
# are scanned for solution code.
SCAN_ROOTS = ("tracks", "core")

# Paths never scanned: the verification harness, the tooling, the docs.
SCAN_EXEMPT_PREFIXES = (
    "verification/harness",
    "tooling",
    "docs",
    ".git",
    "target",
)


PATTERNS: list[Pattern] = [
    # ── Track C: the CSV state machine ─────────────────────────────────────
    Pattern(
        "csv-state-machine",
        "C",
        r"\b(in_quote|quoted_field|unescape_quotes|parse_csv_field|"
        r"parse_csv_record|csv_quote|double_quote_escape|field_state)\b",
        "Track C's quoting rules are the deliverable; writing the field "
        "state machine before kickoff is writing the solution.",
    ),
    Pattern(
        "csv-record-parser",
        "C",
        r"\bfn\s+(parse_csv|parse_record|read_csv|parse_line_csv)\b",
        "A CSV record parser is Track C's core solution.",
    ),
    Pattern(
        "csv-bom-handling",
        "C",
        r"\b(strip_bom|mid_file_bom|bom_in_chunk)\b",
        "Mid-file BOM semantics are Track C's correctness contract.",
    ),
    # ── Track B: the inverted index ───────────────────────────────────────
    Pattern(
        "inverted-index",
        "B",
        r"\b(inverted_index|posting_list|postings|build_index|"
        r"term_freq|term_frequency|doc_frequency)\b",
        "An inverted index is Track B's solution.",
    ),
    Pattern(
        "boolean-query",
        "B",
        r"\b(intersect_postings|union_postings|phrase_query|"
        r"boolean_query|skip_list)\b",
        "Boolean/phrase query evaluation is Track B's solution.",
    ),
    Pattern(
        "streamvbyte",
        "B",
        r"\b(streamvbyte|varbyte_encode|varbyte_decode|pfordelta)\b",
        "Posting-list codecs are Track B's optimisation work.",
    ),
    # ── Track D: graph traversal ──────────────────────────────────────────
    Pattern(
        "shortest-path",
        "D",
        r"\b(dijkstra|astar|a_star|bidirectional_dijkstra|alt_graph|"
        r"contraction_hierarchy|ch_query|ch_preprocess)\b",
        "Shortest-path search is Track D's solution.",
    ),
    Pattern(
        "graph-preprocess",
        "D",
        r"\b(hl_degree|elimination_order|contract_node|hub_label|"
        r"bidirectional_search)\b",
        "Graph preprocessing is Track D's central decision and its solution.",
    ),
    # ── Track E: the entropy coder ────────────────────────────────────────
    Pattern(
        "entropy-coder",
        "E",
        r"\b(rans_encoder|rans_decoder|rans_sym|rans_freq|huffman_table|"
        r"huffman_encode|bit_packer|arith_decoder|arith_encoder)\b",
        "The entropy coder is Track E's entire deliverable.",
    ),
    Pattern(
        "lz-coder",
        "E",
        r"\b(lz77_encode|lz78_encode|match_finder|hash_chain|deflate_block|"
        r"lz4_compress)\b",
        "LZ-style match finding is Track E's optimisation work.",
    ),
    # ── Track A: the percentile estimator ─────────────────────────────────
    Pattern(
        "percentile-estimator",
        "A",
        r"\b(percentile_estimator|p99_histogram|radix_histogram|"
        r"exact_percentile|latency_percentile)\b",
        "The percentile estimator is Track A's core algorithmic work.",
    ),
    Pattern(
        "track-a-aggregation",
        "A",
        r"\b(error_rate_bucket|endpoint_aggregate|latency_histogram|"
        r"hour_bucket)\b",
        "Track A's aggregation shape is its solution.",
    ),
]


@dataclass
class Finding:
    path: str
    line: int
    text: str
    pattern: Pattern

    def __str__(self) -> str:
        return (
            f"{self.path}:{self.line}: [{self.pattern.track}/{self.pattern.name}] "
            f"{self.text.strip()[:100]}"
        )


@dataclass
class Report:
    mode: str
    h0: datetime
    now: datetime
    findings: list[Finding] = field(default_factory=list)
    scanned_files: int = 0
    allowlisted: list[str] = field(default_factory=list)


def parse_ts(s: str) -> datetime:
    s = s.strip()
    if s.endswith("Z"):
        s = s[:-1] + "+00:00"
    dt = datetime.fromisoformat(s)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc)


def load_allowlist() -> set[str]:
    """`<pattern-name>:<path-glob>` entries in verification/allowlist.json."""
    p = REPO / "verification" / "allowlist.json"
    if not p.is_file():
        return set()
    try:
        data = json.loads(p.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return set()
    out = set()
    for entry in data.get("allowlist", []):
        name = entry.get("pattern", "")
        path = entry.get("path", "")
        reason = entry.get("reason", "")
        out.add(f"{name}:{path}:{reason}")
    return out


def is_exempt(rel: str) -> bool:
    norm = rel.replace("\\", "/")
    return any(norm.startswith(prefix) for prefix in SCAN_EXEMPT_PREFIXES)


def scan(patterns: list[Pattern], allow: set[str]) -> tuple[list[Finding], int]:
    findings: list[Finding] = []
    count = 0
    for root in SCAN_ROOTS:
        base = REPO / root
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*")):
            if not path.is_file():
                continue
            if path.suffix not in (".rs", ".py", ".sh", ".c", ".cpp", ".h"):
                continue
            rel = path.relative_to(REPO).as_posix()
            if is_exempt(rel):
                continue
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            count += 1
            for pat in patterns:
                if rel.startswith(pat.exempt_paths):
                    continue
                for i, line in enumerate(text.splitlines(), 1):
                    if not re.search(pat.regex, line, pat.flags):
                        continue
                    key = f"{pat.name}:{rel}:"
                    if any(a.startswith(key) for a in allow):
                        continue
                    findings.append(Finding(rel, i, line, pat))
    return findings, count


def git_head() -> str:
    try:
        out = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=REPO,
            capture_output=True,
            text=True,
            timeout=20,
        )
        return out.stdout.strip() or "unknown"
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def main() -> int:
    ap = argparse.ArgumentParser(description="Rule 05 pre-kickoff compliance guard")
    ap.add_argument("--h0", default=os.environ.get("SD_H0", DEFAULT_H0),
                    help=f"kickoff instant (default {DEFAULT_H0})")
    ap.add_argument("--freeze", default=os.environ.get("SD_FREEZE", DEFAULT_FREEZE))
    ap.add_argument("--list", action="store_true", help="list the pattern set and exit")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    if args.list:
        for p in PATTERNS:
            print(f"[{p.track}] {p.name}\n    {p.why}\n    regex: {p.regex}\n")
        return 0

    now = datetime.now(timezone.utc)
    h0 = parse_ts(args.h0)
    freeze = parse_ts(args.freeze)

    rep = Report(mode="", h0=h0, now=now)

    if now < h0:
        rep.mode = "PRE-KICKOFF"
    elif now < freeze:
        rep.mode = "WINDOW-OPEN"
    else:
        rep.mode = "POST-FREEZE"

    if rep.mode == "PRE-KICKOFF":
        allow = load_allowlist()
        findings, count = scan(PATTERNS, allow)
        rep.findings = findings
        rep.scanned_files = count
    else:
        rep.scanned_files = 0

    if args.json:
        print(json.dumps({
            "mode": rep.mode,
            "now": rep.now.isoformat(),
            "h0": rep.h0.isoformat(),
            "scanned_files": rep.scanned_files,
            "findings": [str(f) for f in rep.findings],
        }, indent=2))
    else:
        print("=" * 72)
        print("RULE 05 GUARD")
        print("=" * 72)
        print(f"  now      : {rep.now.isoformat()}")
        print(f"  kickoff  : {rep.h0.isoformat()}")
        print(f"  freeze   : {freeze.isoformat()}")
        print(f"  mode     : {rep.mode}")
        print(f"  commit   : {git_head()}")
        if rep.mode == "PRE-KICKOFF":
            print(f"  scanned  : {rep.scanned_files} source files under {', '.join(SCAN_ROOTS)}")
            if rep.findings:
                print()
                print(f"  BLOCKED: {len(rep.findings)} track-solution construct(s) found.")
                print("  Rule 05: all project code must be written inside the 72-hour window.")
                print()
                for f in rep.findings:
                    print(f"    {f}")
                print()
                print("  If a finding is legitimate, allowlist it in")
                print("  verification/allowlist.json with a one-line reason.")
            else:
                print()
                print("  CLEAR: no track-solution code in the tree. Infrastructure and design only.")
                print("  This is the state that must hold until kickoff.")
        elif rep.mode == "WINDOW-OPEN":
            print()
            print("  The 72-hour window is OPEN. Solution code is now permitted.")
            print("  The pre-kickoff scan is intentionally inactive here.")
        else:
            print()
            print("  Past the freeze. The scan is inactive.")

    if rep.findings:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
