# Measuring locally the way the board measures

Every local timing and memory number taken before this note came from a **Windows PE**. On
Windows, `map()` in all four tracks falls back to reading the whole input into a `Box`, because
`mmap` does not exist there. That fallback is a full copy of the input file, and the scoring box
never pays it. So a local build was measuring an artefact plus the algorithm, and several A/B
decisions in this repository were made on numbers that had that artefact baked in.

## WSL can build and run the real thing

    wsl -d Ubuntu -- bash -c "cd /mnt/d/SpeedDemon-Campaign && sh track-c/build.sh"

WSL Ubuntu has rustc 1.98, 12 cores, and sees the repository and the datasets. `build.sh`
compiles the Linux path: a real `mmap` over a spare zero page, `madvise`, `prctl`, and the
`#[target_feature]` dispatch. That binary is what the board runs.

## Stage the corpus on ext4, or the numbers are wrong

The datasets under `/mnt/d` are a Windows filesystem mount. Reading them from WSL costs roughly
**8x** what the same read costs on the native filesystem:

    /mnt/d/sd-data/track-c    C on full corpus:  1.35 - 1.40 s
    ~/sd-data-track-c (ext4)  C on full corpus:  0.15 - 0.17 s

Stage once, then measure:

    mkdir -p ~/sd-data-track-c
    cp /mnt/d/sd-data/track-c/{records.csv,regions.csv} ~/sd-data-track-c/
    export SD_DATA_DIR=~/sd-data-track-c SD_OUT_DIR=/tmp/sdc

An 8x shift in the input path is larger than most of the effects we are trying to measure, so a
comparison run on `/mnt/d` can invert a decision.

## Local RSS is NOT the board's M -- VERIFIED, not inferred

`/usr/bin/time -f %M` reports maximum RESIDENT SET SIZE, and that includes file-backed pages the
process has touched. The proof is to run the SAME BINARY on two input sizes and watch RSS follow
the input:

    full corpus (~202 MB):   peak RSS 208,916 KB   (~204 MB)
    2 MB slice:              peak RSS  13,904 KB   (~13.6 MB)

RSS tracks the input size almost exactly, and the small-input figure lands right beside the
board's **11.39 MB** for this track. So the board's `memory.peak` is charging the **anonymous**
side, and a local RSS reading is inflated by roughly the size of the mapped input.

To compare with the board, measure anonymous pages only:

    grep -E '^Anonymous:' /proc/<pid>/smaps_rollup     # while the run is in flight

or, more simply, run the same binary on a SMALL input: the anonymous side barely depends on input
size, so the small-input RSS is a good proxy for what the board will charge.

## The scale factor, so local numbers can be read at all

    track-c, same binary, same corpus on ext4:   local 0.15 - 0.17 s   board 0.0628 s

That is about **2.5x**, which is this box (Ryzen 5 4600H, 8 MB L3) against the scoring box
(i5-13500, 6 P-cores, 12 hardware threads). It means a local Linux A/B can be read for direction
**and** approximate magnitude, where the Windows build could only ever give direction.

TWO CAVEATS ON THAT NUMBER:

- It was taken on an idle box. The same command read **2.47 s** while other work was running, so
  an absolute local time is only meaningful with the run count and the load stated. Use the
  smallest of several runs, not the mean, and never compare across sessions.
- The scoring box is not emulated by WSL. WSL adds a hypervisor boundary, so treat the 2.5x as a
  rough anchor rather than a calibration.

## What this invalidates

- Any memory A/B taken by polling Windows working-set, without separating anonymous from
  file-backed pages.
- Any timing A/B taken on a Windows PE, because the input is copied rather than mapped.
- Any timing A/B taken from WSL with `SD_DATA_DIR` still pointing at `/mnt/d`.

What it does NOT invalidate: correctness. Every track reproduces its published hash on the full
corpus, the tiny hostile oracle and the official sample on both platforms, and that is the check
that decides whether a track is accepted at all.
