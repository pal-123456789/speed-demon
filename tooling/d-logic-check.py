import sys
"""Line-by-line Python port of the search loop in track-d/src/main.rs, used only to test its LOGIC
against plain Dijkstra.  It does not test Rust syntax.

Two things this file has to get right, and the first version got the second one wrong:

  1. THE PORT MUST MATCH THE RUST.  It reproduces the smaller-side rule, the stale-entry skip, the
     `nd < best` pruning, the `nd < mine[v]` relaxation guard, and the no-in-edge shortcut, because
     those are exactly the decisions a corpus hash cannot isolate.

  2. IT MUST BE ABLE TO FAIL.  It printed a mismatch count but never set an exit status, so
     check-d-harnesses.sh read it as a PASS no matter what it found -- a check that cannot fail is
     not a check.  It now exits non-zero on any mismatch, and it runs a NEGATIVE CONTROL first:
     a deliberately broken variant of the port must produce mismatches.  If the control passes,
     the comparison itself is broken and the run fails, so a silent no-op cannot be mistaken for
     agreement.

usage: d-logic-check.py [TRIALS] [SEED]
"""
import heapq, random, sys

# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
INF = (1 << 64) - 1

def bidir(n, fwd, rev, s, t, broken=False):
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
                    # The negative control drops this meeting update, which is the one line that
                    # makes the two searches talk to each other. Without it the answer is never
                    # found and the port must disagree with Dijkstra.
                    if not broken:
                        other_d = other.get(v, INF)
                        best = min(best, nd + other_d if other_d != INF else INF)
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

def run(trials, seed, broken):
    r = random.Random(seed); bad = 0; total = 0
    for _ in range(trials):
        n, fwd, rev = rand_graph(r)
        for _ in range(12):
            s, t = r.randrange(n), r.randrange(n)
            a, b = bidir(n, fwd, rev, s, t, broken), dijkstra(n, fwd, s, t)
            total += 1
            if a != b:
                bad += 1
                # The control's disagreements are EXPECTED, so they are not printed as findings.
                if bad <= 3 and not broken:
                    print("   MISMATCH n=%d s=%d t=%d port=%s dijkstra=%s" % (n, s, t, a, b))
    return total, bad

if __name__ == "__main__":
    trials = int(sys.argv[1]) if len(sys.argv) > 1 else 4000
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 7

    # Negative control: the deliberately broken port MUST disagree, or the comparison is a no-op.
    ct, cbad = run(max(trials // 20, 40), seed + 1, broken=True)
    if cbad == 0:
        print(f"   NEGATIVE CONTROL FAILED: a deliberately broken port agreed on {ct} queries,")
        print( "   so this comparison cannot detect a defect. Refusing to report a pass.")
        sys.exit(1)
    print(f"   negative control: {cbad}/{ct} queries differ with the meeting update removed  OK")

    total, bad = run(trials, seed, broken=False)
    print(f"   {total} queries on {trials} random graphs: {bad} mismatches")
    sys.exit(1 if bad else 0)
