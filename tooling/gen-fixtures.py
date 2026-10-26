import sys
#!/usr/bin/env python3
"""
Speed Demon — grammar-preserving fixture generator (the anti-overfit guard).

WHY THIS EXISTS
  Final scoring runs on a HOLDOUT dataset generated from the same
  distribution at a seed you have never seen. Anything tuned to the specific
  bytes of the 10% sample is a liability, and "code that recognises the input
  and short-circuits" is DISQUALIFICATION from the whole event, not just a
  track.

  So every optimization is gated on data generated from the PUBLISHED GRAMMAR
  with different values, different cardinalities and different sizes. If your
  parser only passes on the sample, it fails here — and you find out at hour
  20 instead of hour 68.

WHAT IT DOES
  Track A  space-delimited log lines: ts method endpoint status latency_us
  Track B  a text corpus + a queries file (BOOL / PHRASE prefixed)
  Track C  a HOSTILE CSV: quoted commas, escaped quotes, embedded LF/CRLF,
           mixed line endings, a mid-file BOM, empty fields, trailing spaces
  Track D  "n m" header then "u v w" edges (unsorted)

  Every generator is seeded, so a failure is reproducible from its seed.
  Retain the failing seeds — they become regression tests.

Usage
-----
    python tooling/gen-fixtures.py --track C --rows 20000 --seed 7 --out verification/data/track-C
    python tooling/gen-fixtures.py --all --out verification/data
    python tooling/gen-fixtures.py --track C --adversarial   # tiny edge-case corpus
"""

import argparse
import json
import math
import os
import random
from pathlib import Path
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
STATUSES = [200, 201, 204, 301, 400, 404, 429, 500, 502, 503]
ERROR_STATUSES = {500, 502, 503}


# ── Track A ───────────────────────────────────────────────────────────────

def gen_track_a(rows: int, seed: int, out_dir: Path) -> None:
    """Log lines: ts method endpoint status latency_us.

    Endpoints follow a heavy Zipf tail (1,842 distinct in the real set, but
    the count here is a parameter so we can prove the code is not tuned to a
    fixed cardinality).
    """
    rng = random.Random(seed)
    n_ep = rng.choice([37, 200, 1842, 5000])
    endpoints = []
    for i in range(n_ep):
        # Zipf-ish popularity is not encoded in the name; names are unique.
        endpoints.append(f"/v2/res{i}/item")
    # popularity weights ~ 1/(i+1)^1.1
    weights = [1.0 / ((i + 1) ** 1.1) for i in range(n_ep)]
    total = sum(weights)
    cum = []
    acc = 0.0
    for w in weights:
        acc += w / total
        cum.append(acc)

    def pick_ep() -> str:
        r = rng.random()
        lo, hi = 0, len(cum) - 1
        while lo < hi:
            mid = (lo + hi) // 2
            if cum[mid] < r:
                lo = mid + 1
            else:
                hi = mid
        return endpoints[lo]

    base = 1_767_225_600  # 2026-01-01T00:00:00Z
    hour_ms = 3_600_000_000
    path = out_dir / "access.log"
    with path.open("w", encoding="utf-8", newline="\n") as f:
        for i in range(rows):
            t = base + (i * 977) % (24 * hour_ms)
            secs = t // 1000
            micros = (t % 1000) * 1000 + (i % 1000)
            hh = (secs // 3600) % 24
            mm = (secs // 60) % 60
            ss = secs % 60
            status = rng.choice(STATUSES)
            # latencies: lognormal-ish, microseconds
            lat = int(min(2_000_000, max(1, rng.lognormvariate(8.0, 1.6))))
            f.write(
                f"2026-01-01T{hh:02d}:{mm:02d}:{ss:02d}.{micros:06d}Z "
                f"{rng.choice(METHODS)} {pick_ep()} {status} {lat}\n"
            )
    print(f"  A: {rows} lines, {n_ep} distinct endpoints -> {path}")


# ── Track B ───────────────────────────────────────────────────────────────

WORDS = ("writ habeas corpus bail bond recognizance indemnity forfeiture "
         "surety matter nothing the of and or not in a to is was be by on "
         "for with from at as that this it").split()


def gen_track_b(docs: int, seed: int, out_dir: Path) -> None:
    """A text corpus + a queries file (BOOL / PHRASE prefixed)."""
    rng = random.Random(seed)
    path = out_dir / "corpus.txt"
    vocab = list(dict.fromkeys(WORDS)) + [f"term{i}" for i in range(500)]
    with path.open("w", encoding="utf-8", newline="\n") as f:
        for _ in range(docs):
            n = rng.randint(20, 120)
            f.write(" ".join(rng.choice(vocab) for _ in range(n)) + "\n")

    qpath = out_dir / "queries.txt"
    nq = 500
    with qpath.open("w", encoding="utf-8", newline="\n") as f:
        for _ in range(nq):
            if rng.random() < 0.62:
                # BOOL — left-associative, no parentheses
                n = rng.randint(2, 5)
                terms = [rng.choice(vocab) for _ in range(n)]
                ops = [rng.choice(["AND", "OR", "NOT"]) for _ in range(n - 1)]
                s = terms[0]
                for op, t in zip(ops, terms[1:]):
                    s += f" {op} {t}"
                f.write(f"BOOL {s}\n")
            else:
                n = rng.randint(2, 4)
                terms = [rng.choice(vocab) for _ in range(n)]
                f.write(f'PHRASE "{" ".join(terms)}"\n')
    print(f"  B: {docs} docs, {nq} queries -> {path}, {qpath}")


# ── Track C ───────────────────────────────────────────────────────────────

NAMES = ["Kaphar, Titus", "Betts", "", "trailing space ", 'quote " inside',
         "comma, quote \"\" and newline\ntogether", "unicode é ü ß",
         "tab\there", "semi;colon"]
NOTES = ['said "no" twice', "a note that\nspans two lines", "", "plain",
         "multi\nline\r\nmixed", "  padded  ", "semi;colon and, comma"]


def gen_track_c(rows: int, seed: int, out_dir: Path) -> None:
    """A HOSTILE CSV exercising every documented edge case."""
    rng = random.Random(seed)
    path = out_dir / "records.csv"
    with path.open("wb") as f:
        # a BOM at the very start
        f.write(b"\xef\xbb\xbf")
        f.write(b"id,name,note,region_id,amount\r\n")
        for i in range(rows):
            name = rng.choice(NAMES)
            note = rng.choice(NOTES)
            region = rng.randint(1, 20)
            amount = rng.randint(-100000, 1000000) / 100.0
            eol = rng.choice(["\r\n", "\n"])

            def q(s: str) -> str:
                if any(ch in s for ch in ',"\n\r'):
                    return '"' + s.replace('"', '""') + '"'
                return s

            row = f"{i},{q(name)},{q(note)},{region},{amount:.2f}{eol}"
            f.write(row.encode("utf-8"))

            # a mid-file BOM at a chunk boundary (every ~1000 rows)
            if i > 0 and i % 1000 == 0:
                f.write(b"\xef\xbb\xbf")

        # a trailing record with an empty amount and an unquoted empty name
        f.write(f"{rows},,,\u0000,0.00\n".encode("utf-8", errors="replace"))
    print(f"  C: {rows} hostile rows (BOM, quoted commas, escaped quotes, "
          f"embedded LF/CRLF, mixed endings) -> {path}")


def gen_track_c_adversarial(out_dir: Path) -> None:
    """A tiny corpus of hand-crafted edge cases — the differential tester's
    ammunition. Every case here corresponds to a published sample feature."""
    path = out_dir / "records.csv"
    cases = [
        b"\xef\xbb\xbfid,name,note,region_id,amount\r\n",
        b'1,"Kaphar, Titus","said ""no"" twice",7,1420.00\r\n',
        b'2,"Betts","a note that\nspans two lines",7,88.50\n',
        b'3,,"",12,0.00\r\n',
        b'4,"trailing space ","comma, quote "" and newline\ntogether",3,-5.25\r\n',
        b'5,"",,0,-0.01\n',
        b'6,"only one quote \"" ,1,0.00\n',
        b'7,"crlf\r\ninside",2,1.00\n',
        b'\xef\xbb\xbf8,"mid-file bom",3,2.00\n',
        b'9,"unicode \xc3\xa9\xc3\xbc",4,3.00\n',
        b'10,"tab\there",5,4.00\r\n',
    ]
    path.write_bytes(b"".join(cases))
    print(f"  C: adversarial edge-case corpus ({len(cases)} cases) -> {path}")


# ── Track D ───────────────────────────────────────────────────────────────

def gen_track_d(nodes: int, edges: int, seed: int, out_dir: Path) -> None:
    """'n m' header then 'u v w' edges, NOT sorted by u."""
    rng = random.Random(seed)
    path = out_dir / "graph.txt"
    with path.open("w", encoding="utf-8", newline="\n") as f:
        f.write(f"{nodes} {edges}\n")
        for _ in range(edges):
            u = rng.randrange(nodes)
            v = rng.randrange(nodes)
            w = rng.randint(1, 10000)
            f.write(f"{u} {v} {w}\n")
    print(f"  D: {nodes} nodes, {edges} edges -> {path}")


# ── main ──────────────────────────────────────────────────────────────────

def main() -> int:
    p = argparse.ArgumentParser(description="Speed Demon fixture generator")
    p.add_argument("--track", choices=["A", "B", "C", "D", "all"])
    p.add_argument("--all", action="store_true")
    p.add_argument("--out", default="verification/data")
    p.add_argument("--rows", type=int, default=20000)
    p.add_argument("--docs", type=int, default=3000)
    p.add_argument("--nodes", type=int, default=5000)
    p.add_argument("--edges", type=int, default=20000)
    p.add_argument("--seed", type=int, default=1)
    p.add_argument("--adversarial", action="store_true",
                   help="Track C: emit the tiny hand-crafted edge-case corpus")
    args = p.parse_args()

    targets = ["A", "B", "C", "D"] if (args.all or args.track in (None, "all")) else [args.track]

    for t in targets:
        out_dir = Path(args.out) / f"track-{t}"
        out_dir.mkdir(parents=True, exist_ok=True)
        print(f"\n[track {t}]")
        if t == "A":
            gen_track_a(args.rows, args.seed, out_dir)
        elif t == "B":
            gen_track_b(args.docs, args.seed, out_dir)
        elif t == "C":
            if args.adversarial:
                gen_track_c_adversarial(out_dir)
            else:
                gen_track_c(args.rows, args.seed, out_dir)
        elif t == "D":
            gen_track_d(args.nodes, args.edges, args.seed, out_dir)
    print("\nDone. These fixtures are NOT the sample data — they exist to "
          "catch overfitting.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
