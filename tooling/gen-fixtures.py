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

# The published sample rows from the events page, byte-verbatim. The H0
# oracle must reproduce the same aggregate on these bytes; they also exercise
# the unit trap (latency_us is MICROSECONDS) and the 5xx error class.
EVENTS_A_SAMPLE = (
    "2026-10-30T18:07:18.569411Z GET /api/v2/products 200 8177\n"
    "2026-10-30T18:07:21.136274Z GET /beta/regions 503 81573\n"
    "2026-10-30T18:07:27.963350Z PUT /v2/ab_tests 200 6381\n"
)

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
        # the published bytes first, so the sample is IN the corpus
        f.write(EVENTS_A_SAMPLE)
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
    print(f"  A: {rows} lines, {n_ep} distinct endpoints, published sample first -> {path}")


def gen_track_a_adversarial(out_dir: Path) -> None:
    """Tiny A corpus: the published sample plus the parsing traps and the
    MODEL-DERIVED fixture set (docs/depth/97-MODEL-OUTPUT-DIGEST.md §3):

    * unit trap + f32-round + u32-max latencies (999, 1000, 1001, 16777217,
      4294967295) — all in one endpoint/hour group;
    * nearest-rank vs floor-index groups n = 1, 2, 20, 100 with
      latency = 1000 * rank — nearest-rank gives p95=19000/p50=10000 at n=20,
      a floor-index reference would give 20000/11000;
    * the formatting tie case e = 1, t = 160 (1 error in 160 requests);
    * error classes: only 5xx counts (500, 599 yes; 400, 600 no).
    """
    path = out_dir / "access.log"
    rows = [
        EVENTS_A_SAMPLE,
        # boundary statuses (100..599, three digits), latency unit trap,
        # RFC3339 with +00:00 instead of Z, missing fractional digits
        "2026-10-30T18:07:28.000001Z GET /api/v1/health 100 0\n",
        "2026-10-30T18:07:29.000002Z POST /v2/checkout 599 1999999\n",
        "2026-10-30T18:07:30.000003+00:00 PUT /beta/regions 500 1\n",
        "2026-10-30T18:07:31.000Z HEAD /v3/items 301 0000001\n",
        # a maximal-length latency (u32 max: 4294967295 microseconds)
        "2026-10-30T18:07:32.000000Z DELETE /v3/items 200 4294967295\n",
    ]
    # ── model fixture: unit trap + f32 rounding + u32 max (one group) ─────
    for i, lat in enumerate((999, 1000, 1001, 16777217, 4294967295)):
        rows.append(f"2026-10-30T18:08:{i:02d}.000000Z GET /fx/units 200 {lat}\n")
    # ── model fixture: nearest-rank vs floor-index groups (lat = 1000*rank) ─
    for name, n in (("n1", 1), ("n2", 2), ("n20", 20), ("n100", 100)):
        for r in range(1, n + 1):
            rows.append(f"2026-10-30T18:09:00.000000Z GET /fx/{name} 200 {1000 * r}\n")
    # ── model fixture: formatting tie e=1, t=160 ──────────────────────────
    for i in range(160):
        status = 500 if i == 0 else 200
        rows.append(f"2026-10-30T18:10:00.000000Z GET /fx/tie {status} 100\n")
    # ── model fixture: error classes — only 5xx counts ────────────────────
    for i, status in enumerate((500, 599, 400, 600)):
        rows.append(f"2026-10-30T18:11:{i:02d}.000000Z GET /fx/err {status} 50\n")
    # ── a second hour so hour bucketing is exercised ──────────────────────
    rows.append("2026-10-30T19:00:00.000000Z GET /fx/hour2 200 1234\n")
    path.write_text("".join(rows), encoding="utf-8", newline="")
    print(f"  A: adversarial corpus ({len(rows)} records: published sample, "
          f"unit trap, nearest-rank groups, tie case, error classes) -> {path}")


# ── Track B ───────────────────────────────────────────────────────────────

# The published query lines from the events page (seed-1 bytes, verbatim).
EVENTS_B_QUERIES = (
    "BOOL bond NOT tebaickplea\n"
    'PHRASE "in the matter of trixclar"\n'
    "BOOL ceeproust AND drolslemdio\n"
    'PHRASE "fyslut cliceen surety"\n'
)
EVENTS_B_WORDS = ("bond", "tebaickplea", "in", "the", "matter", "of",
                  "trixclar", "ceeproust", "drolslemdio", "fyslut", "cliceen",
                  "surety")

WORDS = ("writ habeas corpus bail bond recognizance indemnity forfeiture "
         "surety matter nothing the of and or not in a to is was be by on "
         "for with from at as that this it").split()


def gen_track_b(docs: int, seed: int, out_dir: Path) -> None:
    """A text corpus + a queries file (BOOL / PHRASE prefixed)."""
    rng = random.Random(seed)
    path = out_dir / "corpus.txt"
    vocab = list(dict.fromkeys(WORDS)) + [f"term{i}" for i in range(500)]
    with path.open("w", encoding="utf-8", newline="\n") as f:
        # published terms first, so phrase/BOOL queries have real postings
        f.write(" ".join(EVENTS_B_WORDS) + "\n")
        for _ in range(docs):
            n = rng.randint(20, 120)
            f.write(" ".join(rng.choice(vocab) for _ in range(n)) + "\n")

    qpath = out_dir / "queries.txt"
    nq = 500
    with qpath.open("w", encoding="utf-8", newline="\n") as f:
        f.write(EVENTS_B_QUERIES)
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
    print(f"  B: {docs} docs, {nq} queries, published lines first -> {path}, {qpath}")


def gen_track_b_adversarial(out_dir: Path) -> None:
    """Tiny B corpus: the published query/corpus bytes plus semantic traps."""
    path = out_dir / "corpus.txt"
    # every published term appears; a doc where the 4 queries all match;
    # a doc with CRLF; a doc with 1-byte tokens and an all-stopword phrase.
    path.write_text(
        "bond bond bond tebaickplea\n"
        "in the matter of trixclar\n"
        "ceeproust drolslemdio\n"
        "fyslut cliceen surety\n"
        "a the of in\r\n"
        "x y z\n",
        encoding="utf-8", newline="",
    )
    qpath = out_dir / "queries.txt"
    qpath.write_text(EVENTS_B_QUERIES +
                     'PHRASE "the of"\n'
                     "BOOL NOT bond\n"
                     "BOOL x AND z\n"
                     'PHRASE "in the"\n',
                     encoding="utf-8", newline="")
    print(f"  B: adversarial corpus + queries (published bytes, NOT/AND edge "
          f"cases, stopword phrase) -> {path}, {qpath}")


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

# The published graph bytes from the events page (verbatim header+edges).
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


def gen_track_d_adversarial(out_dir: Path) -> None:
    """Tiny D graph: the published bytes + self-loops, parallel edges,
    zero weights, an unreachable node, and unsorted edges."""
    path = out_dir / "graph.txt"
    edges = [
        # the published edges, verbatim
        "1188446 437285 120\n",
        "64107 865467 170\n",
        "472413 359585 122\n",
        "429961 196139 82\n",
        # adversarial topology (small ids so the queries can reach them)
        "0 1 10\n",
        "1 0 10\n",          # reverse edge listed separately
        "0 1 5\n",           # parallel edge, cheaper
        "2 2 0\n",           # self-loop with zero weight
        "3 4 7\n",
        "4 5 100\n",
        "99 0 1\n",          # a far node (unreachable unless 99 is queried in)
    ]
    header = f"100 11\n"
    path.write_text(header + "".join(edges), encoding="utf-8", newline="")
    print(f"  D: adversarial graph (published bytes, self-loops, parallel "
          f"edges, zero weight, unsorted) -> {path}")


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
            if args.adversarial:
                gen_track_a_adversarial(out_dir)
            else:
                gen_track_a(args.rows, args.seed, out_dir)
        elif t == "B":
            if args.adversarial:
                gen_track_b_adversarial(out_dir)
            else:
                gen_track_b(args.docs, args.seed, out_dir)
        elif t == "C":
            if args.adversarial:
                gen_track_c_adversarial(out_dir)
            else:
                gen_track_c(args.rows, args.seed, out_dir)
        elif t == "D":
            if args.adversarial:
                gen_track_d_adversarial(out_dir)
            else:
                gen_track_d(args.nodes, args.edges, args.seed, out_dir)
    print("\nDone. These fixtures are NOT the sample data — they exist to "
          "catch overfitting.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
