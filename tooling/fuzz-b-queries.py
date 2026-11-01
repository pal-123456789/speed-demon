#!/usr/bin/env python3
"""Adversarial query-set generator for Track B. Not solution code -- keep outside track-*/.

usage: fuzz-b-queries.py DOCS_TXT OUTDIR [SEED] [N_BOOL]

Writes OUTDIR/docs.txt (a copy) and OUTDIR/queries.txt, then you run two binaries on OUTDIR and
compare results.txt.

WHY IT OVERSAMPLES: the risk in a bitmap/list hybrid is not the common case, it is a MIXED chain --
a bitmap AND-ed with a list, or a list promoted to a bitmap by an OR and then demoted by an AND.
A query set drawn uniformly from the vocabulary almost never builds one. So this generator draws
from pools chosen for exactly those transitions:

  * the densest terms alone, and chains of only dense terms, which force the bitmap path;
  * a band around the classification threshold, where a term can fall on either side between one
    corpus and the next;
  * mid-frequency and rare terms, which are lists;
  * all three operators in every position, so the promotion and demotion transitions are hit;
  * real phrases cut out of documents, so the phrase path is exercised alongside the boolean one.

Pair it with check-b-variants.sh: that forces the classification to both extremes, and this makes
the queries that would notice if the extremes disagreed.
"""
import collections
import os
import random
import re
import shutil
import sys

# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    docs_path, outdir = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 11
    n_bool = int(sys.argv[4]) if len(sys.argv) > 4 else 6000

    rnd = random.Random(seed)
    os.makedirs(outdir, exist_ok=True)

    docs = open(docs_path, "rb").read().split(b"\n")
    # Sample document frequencies from the head of the corpus. The generator only needs the SHAPE
    # of the frequency curve to pick interesting pools; exact df is not the point.
    df = collections.Counter()
    for line in docs[:20000]:
        if b"\t" in line:
            df.update(set(re.findall(rb"[a-z0-9]+", line.split(b"\t", 1)[1].lower())))
    ranked = [w for w, _ in df.most_common()]
    if not ranked:
        print("no terms found in", docs_path)
        return 2

    # Five pools: two dense ones (bitmap side), a band around a plausible threshold, a mid
    # range, and the rarest terms (list side).
    pools = [p for p in (ranked[:60], ranked[:60], ranked[30:400],
                         ranked[200:1200], ranked[-3000:]) if p]
    dense = ranked[:60] or ranked

    out = []
    for w in dense[:40]:
        out.append(b"BOOL " + w)

    for _ in range(n_bool):
        k = rnd.randint(1, 5)
        ws = [rnd.choice(rnd.choice(pools)) for _ in range(k)]
        q = b"BOOL " + ws[0]
        for w in ws[1:]:
            q += b" " + rnd.choice([b"AND", b"OR", b"NOT"]) + b" " + w
        out.append(q)

    for _ in range(800):
        k = rnd.randint(2, 6)
        ws = [rnd.choice(dense) for _ in range(k)]
        q = b"BOOL " + ws[0]
        for w in ws[1:]:
            q += b" " + rnd.choice([b"AND", b"OR", b"NOT"]) + b" " + w
        out.append(q)

    for _ in range(300):
        d = rnd.choice(docs[:5000])
        if b"\t" not in d:
            continue
        ws = re.findall(rb"[a-z0-9]+", d.split(b"\t", 1)[1].lower())
        if len(ws) < 3:
            continue
        s = rnd.randrange(len(ws) - 2)
        out.append(b'PHRASE "' + b" ".join(ws[s:s + rnd.randint(2, 3)]) + b'"')

    rnd.shuffle(out)
    open(os.path.join(outdir, "queries.txt"), "wb").write(b"\n".join(out) + b"\n")
    if os.path.abspath(docs_path) != os.path.abspath(os.path.join(outdir, "docs.txt")):
        shutil.copy(docs_path, os.path.join(outdir, "docs.txt"))
    print(len(out), "queries written to", outdir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
