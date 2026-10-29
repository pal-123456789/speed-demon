# Write-up evidence bank

Source material for the **write-up side quest**: two prizes of $100, judged on
insight, closing **November 10**.

The rules matter here: the post must live on a **public blog** (dev.to, Medium,
Hashnode or your own site). A README or a GitHub Pages repo does **not** count.
It must mention Hackathon Raptors and link the repository. Insight is what is
judged, not audience size.

This file is the bank, not the post. Every entry below is a real thing that
happened in this repository, with the measurement that proves it and — the part
that is actually worth reading — **why it stayed invisible**. That is the shape
the organizers describe: *"the profiler reading you misread, the rewrite that
made it slower, the two-line change that halved it."*

---

## 1. The three-byte bug that would have scored zero on every track

**What:** the repository had no `.gitattributes`, and this machine had
`core.autocrlf=true` set at the **system** level. It worked anyway, because a
local `core.autocrlf=false` sat in `.git/config` — which is never committed.

**Why that is a zero:** git decides line endings at **checkout** time from the
*cloner's* configuration, not from the committed bytes. Since the scored build
runs `RUN ./build.sh` on Linux, a clone that rewrites the script to CRLF turns
the shebang into `#!/bin/sh\r`.

**How it was proved rather than asserted.** A control repository, no
`.gitattributes`, `core.autocrlf=true`, commit an LF script, delete it, check it
out again:

```
$ git checkout -- build.sh
build.sh CR bytes = 3
first line bytes: 23 21 2F 62 69 6E 2F 73 68 0D 0A 73
                  #  !  /  b  i  n  /  s  h  CR LF  s
```

Three bytes. `bad interpreter: /bin/sh^M`. Zero on that track, and every local
check stays green, because the local config was the thing hiding it.

**The fix and the near-miss.** `.gitattributes` pinning `eol=lf` — plus two
deliberate exceptions. The near-miss is the interesting half: a blanket
CRLF→LF sweep would have "cleaned" the Track C tiny corpus, which mixes **489
CRLF and 651 bare-LF terminators behind a UTF-8 BOM** *on purpose*, because the
spec requires a parser to handle both, on the track the organizers call the
correctness minefield. Normalising it would have silently deleted that coverage.
So the sweep had to be taught that some bytes are data.

## 2. The compiler flag that warns, exits 0, and does nothing

`rustc -C profile-use=<a directory>` prints `warning: <dir>: is a directory` and
**exits 0**, compiling with no profile at all, while the build log says
`PGO profile found`. A typo in the profile path costs the entire profile-guided
optimisation and reports success.

There is a second, worse half: the track folder *is* the build context, so a
`.profdata` stored anywhere else is a profile the compiler can never read — and
the build still says it found one.

**Why it stayed invisible:** there is no error, no non-zero exit, and no missing
output. The only evidence is a warning nobody greps for, next to a log line
claiming the opposite. `tooling/check_pgo_wiring.sh` now fails on the old form.

## 3. `git` ignore is not `docker` ignore

The track folders each contained **1.49 MB** of host build output — `run` and
`run.pdb` — against **0.02 MB** of tracked content. They were correctly
`.gitignore`d, so every tracked-file audit passed, and the repository was clean.

They were still in the build context. The Dockerfile does `COPY . /src`, and the
*folder* is the context, not the repository. 1.51 MB went into every scored
build that any local tooling produced.

**Why it matters:** if the in-image compile ever failed to replace `run`, then
`COPY --from=build /src/run /app/run` ships a stale host binary of unknown
provenance to the scoring box — and the build succeeds.

**The fix:** a `.dockerignore` per track folder, and a gate that fails if any
forbidden file is on disk without a matching line. Verified that this changes
nothing about the output: the image is still 30 MB and the produced hash is
byte-identical, `0b2f2ea8…`.

## 4. Python wrote CRLF into ten committed files, on purpose

`Path.write_text()` inherits `newline=None`, and Python then translates every
`\n` into `os.linesep` — which is `\r\n` on Windows. Two tools here did exactly
that: the one that generates `PERF.md`/`README.md`, and the one that writes the
`.sha256` pins. That is where the CRLF in those files came from. It was not a
git accident; it was two missing keyword arguments.

**The compound failure, which is the interesting one.** Regenerating the docs
still printed `nothing to do`. The staleness check read the file with
`read_text()`, which uses **universal newlines** — folding CRLF back to LF — so a
file with the wrong line endings read back byte-identical to a correct one. The
tool compared its own normalised view against itself, called the file fresh, and
skipped it. A checker that normalises the exact property it is meant to check
will always report success.

Fixed at all three levels: `newline="\n"` on the writers, a byte-faithful read
for the comparison, and an `ast`-based gate that fails on any text-writing call
which omits `newline=` — so the cause is caught, not just the symptom.

## 5. A gate that blamed the generators for data that was simply absent

Running the full CI suite from a **fresh clone** produced:

```
FAIL a fixture generator emits data the reference rejects — the contract is broken
```

Nothing was wrong with any generator. The corpora are generated and gitignored,
so a clean checkout has none of them, and the gate counted "no data dir" as a
failure.

**Why it is a bad bug:** it is a wrong diagnosis of a non-event, and it points
at the one thing that was working. It sends you hunting for a broken generator,
on a gate whose entire job is to catch a broken generator. The suite was red on
a clean checkout while every number in the documentation said green.

Fixed by making the exit code carry the distinction — `1` means the oracle
looked at real data and disagreed, `2` means nothing was checkable — and turning
`2` into a skip that prints the command which fixes it.

## 6. The guard that guarded nothing

A status document asserted, in the present tense, that a specific script guarded
against the silent-PGO failure. The script did not exist. Nothing noticed,
because nothing checks prose against the filesystem.

Now something does: every `tooling/…` path named in first-party prose must
exist. Prose is where a deleted tool leaves a present-tense claim behind.

## 7. Reproducing the board's own dataset, twelve for twelve

The board runs every track at **full scale, seed 1** — not the sample scale — and
until this pass only tiny and sample corpora had ever been executed here. So the
one dataset the correctness gate actually compares against was unexercised.

Generating all four from the vendored generators and running the vendored
reference reproduced **all twelve published hashes**, with every byte count
exactly as published:

```
A  requests.log  238,836,234  -> summary.txt     495ca096…   ok
B  docs.txt      253,827,587  -> results.txt     f86973c2…   ok
   queries.txt     1,763,534
C  records.csv   201,935,059  -> aggregates.txt  d8088b1c…   ok
   regions.csv     2,177,799
D  graph.txt      91,950,247  -> distances.txt   a23dbdf0…   ok
   queries.txt        29,401
```

Different machine, different Python build from the organizers', so it also shows
the generators are byte-deterministic across machines at the size that counts.

## 8. Every "clever" fix has a control experiment

The pattern worth writing about: the repo's recurring bug class is not wrong
algorithms, it is **checks that normalise away the thing they check**. CRLF
folded by universal newlines. A build context that git cleans but docker does
not. A compiler flag that reports success while doing nothing. A gate that
cannot tell "absent" from "broken".

For each one the useful discipline was the same: reproduce the failure
deliberately, confirm the fix flips it, and then confirm the fix does not change
anything it should not have — the Docker output hash was byte-identical, the S
axis stayed at 141 lines, and the hostile corpus still had both terminator kinds.

---

## Suggested shape for the post

1. **Hook:** a repository that was green locally and would have scored zero.
2. **The three-byte bug** (§1) — with the byte dump. Concrete and visual.
3. **The pattern:** checks that normalise away what they check (§2, §4, §5).
4. **The one that is genuinely subtle** (§3): `git` ignore is not `docker` ignore.
5. **What it cost to find:** every one of these was found by testing the *right
   way* — a fresh clone, a control repository, the real dataset — not by reading
   the code harder.
6. **Close:** the fix for all of them is the same shape — turn the finding into
   a gate, then prove the gate bites by injecting the bug back.

Capture a screenshot of the failing control experiment if one is still
reproducible; a `#!/bin/sh\r` byte dump is the single most persuasive image in
this bank.
