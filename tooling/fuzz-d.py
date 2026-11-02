import sys
#!/usr/bin/env python3
"""Differential fuzz for Track D (keep OUTSIDE track-d/).

usage: fuzz_d.py BIN [TRIALS] [SEED]

BIN is the built Track D binary (track-d/run).  Each trial writes a random
graph.txt + queries.txt, runs BIN with SD_DATA_DIR / SD_OUT_DIR, and compares
distances.txt with a plain heapq Dijkstra.  The weights are chosen around the
point where the program moves a weight to its side list: for n nodes it uses
hb = bit_length(n | 1) bits per node id and treats w >= (2^32 - 1) >> hb as
heavy, so the generator emits big-1, big, big+1, 2^32-1 and 2^32-2 alongside
zeros and small values.  Sizes straddle the 16,384-node part boundary.
Exit status 0 only if every answer matches.
"""
import heapq, os, random, subprocess, sys, tempfile

# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
INF = float("inf")

def gen(r):
    n = r.choice([1, 2, 3, 5, 16, 100, 1000, 16383, 16384, 16385, 20000, 33000])
    m = r.randint(0, min(4 * n, 60000))
    hb = (n | 1).bit_length()
    big = ((1 << 32) - 1) >> hb
    pick = lambda: r.choice([0, 0, 1, 1, 2, 7, 50, max(big - 1, 0), big, big + 1,
                             (1 << 32) - 1, (1 << 32) - 2, r.randint(0, big * 2 + 5)]) & 0xFFFFFFFF
    edges = [(r.randrange(n), r.randrange(n), pick()) for _ in range(m)]
    if r.random() < 0.5 and n > 2:                       # a long chain so answers get large
        k = r.randint(2, min(n, 300)); nodes = r.sample(range(n), k)
        edges += [(nodes[i], nodes[i + 1], pick()) for i in range(k - 1)]
    qs = [(r.randrange(n), r.randrange(n)) for _ in range(150)]
    qs += [(x, x) for x in r.sample(range(n), min(n, 5))]
    return n, edges, qs

def oracle(n, edges, qs):
    adj = [[] for _ in range(n)]
    for u, v, w in edges:
        if u != v: adj[u].append((v, w))
    cache = {}
    def sssp(s):
        d = {s: 0}; h = [(0, s)]
        while h:
            du, u = heapq.heappop(h)
            if du > d.get(u, INF): continue
            for v, w in adj[u]:
                nd = du + w
                if nd < d.get(v, INF): d[v] = nd; heapq.heappush(h, (nd, v))
        return d
    out = []
    for s, t in qs:
        if s not in cache: cache[s] = sssp(s)
        out.append(cache[s].get(t, -1))
    return out

def main():
    binp = os.path.abspath(sys.argv[1])
    trials = int(sys.argv[2]) if len(sys.argv) > 2 else 60
    r = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
    bad = 0
    for k in range(trials):
        n, edges, qs = gen(r)
        with tempfile.TemporaryDirectory() as d:
            with open(os.path.join(d, "graph.txt"), "w", newline="\n") as f:
                f.write(f"{n} {len(edges)}\n")
                f.writelines(f"{u} {v} {w}\n" for u, v, w in edges)
            with open(os.path.join(d, "queries.txt"), "w", newline="\n") as f:
                f.writelines(f"{s} {t}\n" for s, t in qs)
            env = dict(os.environ, SD_DATA_DIR=d, SD_OUT_DIR=d)
            p = subprocess.run([binp], env=env, capture_output=True, text=True)
            want = oracle(n, edges, qs)
            try:
                got = [int(x) for x in open(os.path.join(d, "distances.txt")).read().split()]
            except Exception as e:
                got = None
            if p.returncode != 0 or got != want:
                bad += 1
                where = next((i for i, (a, b) in enumerate(zip(got or [], want)) if a != b), None)
                print(f"FAIL trial {k}: n={n} m={len(edges)} exit={p.returncode} first_diff_query={where}")
                if got is not None and where is not None:
                    print(f"   query {qs[where]}: got {got[where]} want {want[where]}")
                if p.stderr: print("   stderr:", p.stderr.strip().splitlines()[-1][:200])
    print(f"{trials - bad}/{trials} trials match")
    sys.exit(1 if bad else 0)

main()
