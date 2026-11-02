import sys
"""Line-by-line Python port of the search loop in main_d0baaf.rs (the user's own code),
used only to test its LOGIC against plain Dijkstra.  It does not test Rust syntax."""
import heapq, random

# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
INF = (1 << 64) - 1

def bidir(n, fwd, rev, s, t):
    if s == t: return 0
    if not rev[t]: return INF                       # no in-edge: unreachable
    best = INF
    dist = [{s: 0}, {t: 0}]
    q = [[(0, s)], [(0, t)]]
    while q[0] and q[1]:
        a, b = q[0][0][0], q[1][0][0]
        if a + b >= best: break
        k = 1 if len(q[1]) < len(q[0]) else 0
        mine, other = dist[k], dist[1 - k]
        d, u = heapq.heappop(q[k])
        if d > mine.get(u, INF): continue            # stale entry
        for v, w in (fwd if k == 0 else rev)[u]:
            nd = d + w
            if nd < best:
                if nd < mine.get(v, INF):
                    mine[v] = nd
                    heapq.heappush(q[k], (nd, v))
                    best = min(best, min(INF, nd + other.get(v, INF)))
    return best

def dijkstra(n, fwd, s, t):
    if s == t: return 0
    d = {s: 0}; h = [(0, s)]
    while h:
        du, u = heapq.heappop(h)
        if du > d.get(u, INF): continue
        if u == t: return du
        for v, w in fwd[u]:
            nd = du + w
            if nd < d.get(v, INF): d[v] = nd; heapq.heappush(h, (nd, v))
    return INF

def rand_graph(r):
    n = r.randint(1, 40); m = r.randint(0, 3 * n)
    wchoice = lambda: r.choice([0, 0, 1, 1, 2, 3, 5, 9, 100, (1 << 32) - 1, (1 << 32) - 2, 4294967295 - r.randint(0, 5)])
    fwd = [[] for _ in range(n)]; rev = [[] for _ in range(n)]
    for _ in range(m):
        u, v, w = r.randrange(n), r.randrange(n), wchoice()
        if u == v: continue                           # the Rust drops self-loops
        fwd[u].append((v, w)); rev[v].append((u, w))
    return n, fwd, rev

if __name__ == "__main__":
    r = random.Random(7); bad = 0; total = 0
    for trial in range(4000):
        n, fwd, rev = rand_graph(r)
        for _ in range(12):
            s, t = r.randrange(n), r.randrange(n)
            a, b = bidir(n, fwd, rev, s, t), dijkstra(n, fwd, s, t)
            total += 1
            if a != b:
                bad += 1
                if bad <= 3: print("MISMATCH", n, s, t, a, b)
    print(f"{total} queries on 4000 random graphs: {bad} mismatches")
