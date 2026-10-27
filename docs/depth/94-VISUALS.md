# VISUALS — the picture pack (diagrams, charts, budgets)

> **Design only, no solution code (rules 04/05).** ASCII on purpose: it renders
> in PERF.md, in a terminal, in a diff, and in the judges' browser without a
> binary asset in the track folder (an image in `track-*/` would count toward S
> and enter the image). Paste any of these straight into PERF.md / README.md.

---

## 1. The four pipelines at a glance

```
TRACK A · INGEST (ref 32.05 s / 228 MB)          TRACK C · JOIN (ref 14.44 s)

  /data/requests.log                               /data/records.csv  /data/regions.csv
        |                                                |                  |
   [chunk 1 MiB ×12, snapped to \n]                 [quote-parity prepass → cuts]
        |                                                |                  |
   parse ts|method|endpoint|status|latency          split at record boundaries
        |           |        |                         |                  |
   hour = bytes[0..13]  intern endpoint            csv 6-state oracle   regions map
        \__________ __________/                         \______ join ______/
                   |                                            |
        (hour, endpoint) → bump chunks                    per-country aggregate
                   |                                    (count,id_sum,sum,min,max,
        nearest-rank select ×3                           distinct names, bytes)
                   |                                            |
        sort by (hour, endpoint) bytes                    sort by country bytes
                   |                                            |
             summary.txt  (one write_all)                  aggregates.txt (one write)


TRACK B · INDEX (ref 56.35 s)                    TRACK D · TRAVERSE (ref 72.69 s)

  /data/docs.txt (id<TAB>text)                     /data/graph.txt  /data/queries.txt
        |                                                |                  |
   [doc-aligned chunks ×12]                         1 MiB chunks → SWAR u32 parse
        |                                                |                  |
   tokenize: [a-z0-9]+ after lower                 pass 1 deg[u]++; prefix sum
        |                                                |                  |
   term ids by SORTED bytes  ──► postings + positions  CSR: off[n+1] + (w<<32)|v
        |                                                |                  |
   /data/queries.txt ──► BOOL left-fold set ops      per-query Dijkstra (heap)
                   └──► PHRASE contain-all + k-cursor      early exit at t; u64 dist
        |                                                |                  |
   count, sum(ids), 10 smallest                      -1 unreachable, 0 for s==t
        |                                                |                  |
             results.txt                                  distances.txt
```

## 2. Track C quoting oracle — the state machine (the DQ minefield)

```
                    ,                         ,            LF/CRLF
        ┌─────────► FS ─────────────────────► FS ─────────► FS   (E / Rec)
        │            │  "                    ▲   O
        │            ▼                      │   │
        │          IQ ◄──── "" ──── QC ─────┘   │
        │          │  └── any (incl , LF CR) ───┘  (all stay IQ: DATA)
        │          │
        │        " │
        │          ▼
        │         QC ── , ──► FS        (field ends)
        │          │ ── LF/CRLF ──► FS (record ends)
        │          └── O ──► AC
        │                     │  " or O: stay AC [lenient, per csv module]
        └─────────────────────┘
   CRP: CR outside quotes -> record ends (CRLF or bare CR)
   BOM : EF BB BF at a record start is stripped from the FIRST field only

   INVARIANT for the parallel split: "inside quotes" == odd count of " bytes
   (true while a quote never appears inside an unquoted field). Parallel cuts
   are the record ends at even parity — proven over chunk sizes 1..1 MiB by
   tooling/csv-boundary-check.py.
```

## 3. A-axis output anatomy (byte-exact, one line)

```
2026-10-30T18:00:00Z  /api/v2/products   18422  6381  41207  81573  7  380
└──────┬───────────┘  └──────┬───────┘   └─┬─┘  └─┬─┘ └─┬──┘ └─┬─┘ └┬┘ └─┬┘
   hour13 + ":00:00Z"      endpoint      count  p50   p95    p99  errs  ppm
                                                        (nearest rank)  = errs*1e6/count
   separator: single space · terminator: \n on EVERY line · sort: (hour, endpoint) bytes
```

## 4. Percentile rank intuition (why nearest-rank is not "the middle")

```
n = 20 sorted latencies  v1..v20      p50 → rank = ceil(50*20/100) = 10 → v10
                                      p95 → rank = ceil(95*20/100) = 19 → v19
                                      p99 → rank = ceil(99*20/100) = 20 → v20
   floor-index (WRONG here)            p95 → int(0.95*20) = 19 → v19? no: v[19] = v20
   The fixtures pin n=1/2/20/100 groups; a floor-index implementation
   disagrees at n=20 (20000 vs 19000) — that is a hash zero.
```

## 5. Memory budget bars (MB, lower is better; cgroup `memory.peak`)

```
TRACK A   target ~15-25 MB                        reference 228 MB
  values (3.1M × u32)        ████████████████ 12.4
  chunk links + slack        █████ 3.5
  read buffers (12×256 KiB)  ██ 3.0
  endpoint tables            ██ 1.5
  merge scratch + output     ██ 1.5
  baseline + slack           ████ 3.0
  /data pages                0  (pre-warmed by the host, outside our cgroup)

TRACK B   target < 1 GB                           reference: unpublished
  postings (blocks/bitmaps)  ████████████ 40-50
  positions (phrases)        ████████████ 60-140
  per-thread scratch (12×)   █████ 20
  wave buffers               ████████ 25-40

TRACK C   target < 300 MB                         reference: unpublished
  per-worker aggregate tables ████ small
  regions map                 █ tiny
  read buffers (12×256 KiB)   ██ 3.0
  (the 204 MB CSV is NEVER retained — streamed once)

TRACK D   ~70 MB CSR (+~45 MB if a transpose is needed)
  forward CSR (off u32 + edge u64) ████████████████ 44
  reverse CSR (if built)           ████████████████ 44
  per-thread dist + touched (12×)  ████████ 12
  heap + scratch                   ███ 5
```

## 6. The K ladder (speed points vs T ratio, M/S/D = 1.0)

```
pts
85 |                                                             ●K=50
80 |
75 |
70 |
65 |                                        ●K=20
60 |
55 |
50 |                          ●K=10
45 |
40 |
35 |            ●K=5
30 |
25 |
20 |     ●K=2
15 |
10 |
 5 |
 0 ●K=1
   └────┬────────┬────────┬────────┬────────┬────────┬────────┬────►  T ratio (Tref/T)
        1x       2x       5x      10x      20x      50x     100x
                 (T = Tref/K²)

  Per track, K=10 (50 pts) needs:  A 0.321 s · B 0.564 s · C 0.144 s · D 0.727 s
  Board visibility cap: 3× the reference. Final gate: 10×.
```

## 7. Where the 65/35 split lands (why the write-up pays)

```
track total = 0.65 × speed_score + 0.35 × judge_score

  speed 40 / judge 82  → 26.0 + 28.7 = 54.7
  speed 50 / judge 82  → 32.5 + 28.7 = 61.2     (+6.5 for K 6.8→10)
  speed 50 / judge 92  → 32.5 + 32.2 = 64.7     (+3.5 from the write-up alone)

  overall title = geometric mean of the best THREE track totals
  → lifting the weakest of the three by 3 points beats lifting the best by 4.
```

## 8. The 72-hour shape (Gantt, ASCII)

```
H0     H2      H12            H24         H48      H50    H60      H72
|------|--------|--------------|-----------|--------|------|--------|
 intake  baselines   ── rung loop (1/total weighted) ──  harden  write-up
 specs   SHA-green   optimize → bench-rung → re-pin → tag → commit
 pins    ×3 tracks   re-solve waterfill every ~8h        P6+P7   freeze
 0-2     2-12        12-50                               50-60   60-72
        ↑ submit early if the platform allows           ↑ ≤3× board cap
        ↑ two 6h sleeps banked before H48               ↑ no new features after H60
```

## 9. The gate wall (what stands between a bug and a zero)

```
 rule05 ──► fmt ──► clippy ──► tests ──► offline build ──► per-track build
   │           │        │         │            │                 │
   └───────────┴────────┴─────────┴────────────┴─────────► context replay
                                                                  │
 image contract ──► name parity ──► folder audit ──► K self-check ─┤
                                                                  │
 CSV boundary ──► determinism matrix ──► S/D axes ──► e2e sandbox ─┤
                                                                  │
                          sentinels ──► pre-freeze lockdown ◄──────┘
                              │
                              ▼
                    the board's three gates: SHA · timeout · sandbox
```

## 10. Risk heat map (likelihood × impact, before mitigation)

```
impact
  ↑   ┌──────────────┬──────────────┬──────────────┐
  │   │ hash drift   │ nondetermin. │ wrong bytes  │
zero│   │ (name/format)│ (threads)    │ (quoting)    │
  │   ├──────────────┼──────────────┼──────────────┤
  │   │ build fail   │ timeout      │ sandbox      │
  │   │ (offline)    │ (10× / 3×)   │ violation    │
  │   ├──────────────┼──────────────┼──────────────┤
 low│   │ S bloat      │ M blow-up    │ dep added    │
  │   └──────────────┴──────────────┴──────────────┘
      rare             sometimes       likely   → likelihood

  Every cell above has a named gate or fixture in 95-GROUND-BLUEPRINTS §6.
```