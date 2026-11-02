# RULE 05 — pre-kickoff compliance, with evidence

Rule 05 is the one rule in this event whose penalty is **disqualification of the
submission** rather than a bad score. This file records the rule verbatim, the
official clarification that interprets it, and an inventory of every file that
exists before kickoff with the reason it is not solution code.

Nothing here is deleted from history. Every claim is checkable with a command
listed beside it.

---

## 1. The rule, verbatim

> **05 · New code only** · All of your own solution code is written during the
> 72-hour window. Reading up on algorithms, sketching designs, preparing your
> environment, and building on our public starter template are all fine and
> expected. Committing your own solution code before kickoff disqualifies the
> submission.

Source: `speeddemon.dev` full context document, section *"Rules — ten of them"*,
<https://speeddemon.dev/speed-demon-context.md>. Retrieved **2026-10-29 13:36
UTC**, re-read **2026-10-30 05:49 UTC**.

## 2. The clarification that defines the boundary

> **Does tooling written before kickoff count as project code?**
> No, as long as it stays outside the four track folders. Benchmark scripts and
> tools in the repo root, a tools folder or a separate repo are never scored and
> never count toward S. Code inside a track folder is project code: it must be
> written during the 72 hours and it counts toward S. A committed data file
> inside a track folder (for example a `.profdata` profile) is data, not code.

Source: the same document, *"Clarifications given to participants"*. Retrieved
2026-10-29, re-read 2026-10-30.

Two boundaries follow, and they are the only two that matter:

| Where the file lives | Before kickoff |
|---|---|
| **Outside** `track-a/` … `track-d/` | Allowed, never scored, never counts toward S. |
| **Inside** a track folder | It is project code. It must be written in the 72 hours. |

Also relevant, from the same clarifications: *"Making your repo from the
template, installing the board app, filling in the team form and preparing your
environment are all fine before kickoff."*

---

## 3. Inventory of every file that exists before kickoff

### 3a. Outside the track folders — allowed by the clarification

| Path | What it is | Why it is generic infrastructure, not solution code |
|---|---|---|
| `prep/bump.rs` | A typed bump arena (`push`/`reset`, no per-item allocation, no `unsafe`) | A data-structure primitive. It knows nothing about any track's algorithm, output format or data layout. It cannot compute any part of any answer. |
| `prep/lines.rs` | An iterator over `&[u8]` splitting on `\n` and `\r\n`, zero-copy | A byte-splitting primitive with no domain content. Both Track A and Track C split lines and they need opposite quoting rules; this file has no opinion about either. |
| `prep/parse_u32.rs` | A checked decimal `u32` parse returning value + bytes consumed | Generic numeric parsing. Every track uses decimal integers somewhere and none of them has a track-specific algorithm here. |
| `prep/itoa.rs` | `u32`/`u64` → decimal into a reused buffer | The inverse primitive. Output *formatting rules* (column order, `-0.00`, cents) are deliberately NOT here. |
| `tooling/*.py`, `tooling/*.sh`, `tooling/*.ps1` | Generators, gates, K calculator, conformance lab, this harness driver | Benchmark scripts and tools, explicitly named as allowed by the clarification. |
| `verification/harness/**` | The fsn1 replica: container run, cgroup `memory.peak`, trimmed mean, gate checks, JSONL ledger | Verification infrastructure. It measures submissions; it is not one. |
| `verification/cases/**`, `verification/contract.toml`, `verification/ledger.jsonl` | Tiny conformance corpora, the frozen-numbers contract, the measurement ledger | Test data and recorded facts. A committed data file is data, not code. |
| `reference/**` | The organizers' own specs, generators, references, harness and tests, vendored byte-identically | Their code, not ours. Rule 16 makes a byte-identical copy free. |
| `docs/**`, `README.md`, `Makefile`, `check.py`, `.gitattributes`, `.dockerignore` | Design, plans, this file, and the starter's own checker | Prose and configuration. |
| `track-*/.dockerignore`, `track-*/Dockerfile`, `track-*/build.sh` | The build contract each track must meet | Build plumbing derived from the organizer's published example, not an algorithm. |

`prep/` is deliberately outside every track folder for exactly this reason, and
`prep/README.md` says so at the top.

### 3b. Inside the track folders — the one grey area, stated plainly

Each `track-*/src/main.rs` and `src/io.rs` exists before kickoff. They are
**scaffolds, not solutions**, and the distinction is mechanical rather than a
matter of opinion:

- they compile and satisfy the Docker contract (read `/data`, write exactly one
  file to `/out`, exit 0), which is what lets the scored path be verified before
  the window opens;
- they compute **none** of the answer. Run on the real sample data they emit a
  fixed sentinel line, `#SCAFFOLD_NOT_IMPLEMENTED`, plus the line and byte count
  they read;
- that sentinel is detectable by machine, and the repository's own gate fails if
  it is present at freeze time (`verification/pre-freeze.ps1`, "no scaffold
  sentinel in source").

Verify with:

```sh
sh tooling/ci.sh                 # includes: pre-kickoff scaffolds are still scaffolds (rule 05)
python tooling/rule05_guard.py   # CLEAR: no track-solution code in the tree
```

If the organizers consider a compiling contract-shaped stub to be "solution
code", the remedy is to replace those four files with the starter's own
reference at kickoff, which the template already provides. We would rather ask
than assume (§4).

### 3c. What deliberately does NOT exist before kickoff

No parser, no index, no join, no shortest-path code, in any folder. No cached
outputs, no precomputed answers (rule 04). No `.profdata` — profiles are
generated in-window from real code. `git log` is the evidence: every commit
before kickoff changes docs, tooling, verification infrastructure or the
scaffold, and nothing else.

---

## 4. The question to the organizers

Drafted, **not yet sent**. Send it in Discord before doing anything that depends
on the answer:

> Rule 05 says all of our own solution code is written in the window, and the
> clarification says tooling outside the four track folders is never project
> code. We have four small generic Rust primitives (bump arena, byte-line
> splitter, decimal u32 parse, u32-to-decimal) in a `prep/` folder outside every
> track, to be copied into a track at H0 where they would then count toward S.
> They contain no track algorithm, no output format and no data layout. Is that
> within rule 05, or should we hold them until kickoff?

Two outcomes, both fine: either the answer is "yes, that is generic
infrastructure", or we copy them at H0 from a file that was never in a track
folder — which is what `prep/README.md` already instructs.

---

## 5. What is not covered here

`docs/SESSION-LOG.md` records the defects found in the tooling and the gates that
now catch them. This file is only about rule 05: what existed before the window,
where it lived, and why it is not a solution.
