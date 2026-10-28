# DEMO.MP4 STORYBOARD (optional, unscored — but the good ones get published)

> The deliverables table lists `demo.mp4` as **optional, under 3 minutes, not
> scored** — "but the good ones end up in the results write-up and on the
> archive page." It is free visibility for the team and it costs nothing on any
> axis (it lives at the repo root, outside every track folder, so it never
> enters S). Record it in the last hour, from the already-green build.

## Rules of the format

- **≤ 3:00.** Aim for 2:15 — judges stop watching at 3.
- **No slides.** Screen capture + voice-over. Show the terminal, the profiler,
  the board.
- **Every number is one you measured.** Say "measured on the board run
  <id>", never "about".
- **One hard number per track.** The video is a trailer for PERF.md, not a
  replacement for it.

## The 8 shots (2:15 total)

| # | Time | Shot | What is on screen |
|---|---|---|---|
| 1 | 0:00–0:15 | **The hook** | The reference line `32.05 s` next to ours; one sentence: "same task, same data, same machine." |
| 2 | 0:15–0:35 | **The board** | speeddemon.dev board with our run IDs; name the K and the speed points. |
| 3 | 0:35–1:00 | **The pipeline (Track A)** | `docs/depth/94-VISUALS.md` §1 diagram while the built binary runs on the sample: `/usr/bin/time` line, output file appearing. |
| 4 | 1:00–1:20 | **The profiler** | `perf stat` (or `perf top`) on the hot loop; point at the one line that moved the number. |
| 5 | 1:20–1:40 | **The correctness spine** | `python check.py a --data verification/data/track-a --expected ca6311bbeb53...` printing the match; then the Track C quoting fixture (`"said ""no"" twice"`) to show the hostile case is handled. |
| 6 | 1:40–2:00 | **The trade we made** | PERF.md open at the board-run table: "step 4 cost 60 lines and bought 1%, so we reverted it" — the failed experiment is the interesting part. |
| 7 | 2:00–2:10 | **Determinism** | `sh tooling/determinism-check.sh all` finishing with "ALL RUNS BYTE-IDENTICAL". |
| 8 | 2:10–2:15 | **Close** | Repo URL, licence, "three tracks entered, best-three geomean" number. |

## Capture checklist (do this, not more)

- [ ] Terminal at a readable size, dark background, font ≥ 16 pt
- [ ] Commands **pre-typed** in a scratch file and pasted, so no typos on camera
- [ ] `asciinema`/OBS or a plain screen recorder; 1080p is plenty
- [ ] Audio: one take, spoken clearly; no music
- [ ] Verify every number against PERF.md **before** recording (judges check run IDs)
- [ ] Export H.264 ≤ 3:00; put it at the repo root as `demo.mp4`
- [ ] Do **not** add it inside a `track-*/` folder (it would enter the image and S)