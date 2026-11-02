# Scratch space: use D:, not C:

**C: had 6.3 GB free; D: has 171.9 GB.** The workstreams had accumulated 471 MB of
scratch in `%TEMP%` on C:, which is the drive that is short. It has been moved to
**`D:\sd-tmp`**, and C: is back to 6.8 GB free.

## The rule

**Windows-side scratch goes under `D:\sd-tmp`.** That means:

```
$env:TEMP = 'D:\sd-tmp'          # for a PowerShell session
C:\...\Temp\...  ->  D:\sd-tmp\...   # for anything already written
```

Everything in the moved set was ours and identifiable by prefix or extension —
`a_*`, `b_*`, `c_*`, `d_*`, `sd-*`, `main*.rs`, `track-*`, `*.exe`, `*.rs`, `*.jsonl`,
the frozen A/B binaries and the workstream master copies. Other applications' temp files
were left alone.

## The WSL side is fine as it is

WSL's `/tmp` lives inside the Linux VM's virtual disk, **not on C:**, so
`tooling/measure-full.sh` and `run-once.sh` already taking `${TMPDIR:-/tmp}` are correct
and need no change. The pressure is entirely on the Windows side.

## Why this matters beyond disk space

Two of the tools this campaign relies on — `measure-full.sh` and `ab-track.sh` — write
their output directory into scratch while a track binary runs against it. **When scratch
collides, they report `WRONG` for a binary that is green**, which is exactly what happened
when every workstream shared a hardcoded `/tmp/sd-measure`: it made me revert two tracks
whose changes were correct. **Scratch isolation and scratch location are the same
concern** — a measurement tool is only trustworthy if its workspace is private and
present. Giving it a real, roomy location on D: is part of that, not a separate
housekeeping task.

## Before restarting the machine

The working tree is clean and everything verified is pushed:

```
HEAD              6d2b00e
track A           profile-guided build (verified through check.py: hash MATCH, 30 MB image)
track B           0c99d88's content restored — the board scores it best at 52.468
track C           69e76e0
track D           3f9a604 — peak 171.1 -> 78.7 MB, code lines 354 -> 338, 5/5 MATCH
```

`D:\sd-tmp` holds the frozen binaries and master copies the workstreams left behind, so
nothing in flight is lost by the reboot.
