# Rule changes before the Oct 28 freeze — what changed and what it breaks here

**Status: BINDING. This document supersedes any conflicting statement in
`docs/MASTER-PLAN.md`, `docs/CAMPAIGN-STATUS.md`, the per-track specs, the
per-track README/PERF files, `verification/contract.toml` comments and
`tooling/measure_axes.py`.**

Provenance: the organizers' answers to a competitor's five questions, published
in the hackathon channel ahead of the Oct 28 freeze, with the full list to
appear under "Changes before freeze" on speeddemon.dev. Where this document and
the event page disagree, the event page wins — but this is the machine-readable
record we code against until then.

The two axes that move are **S** and **D**. Both are *penalties only* from now
on. That is the single most important consequence: no amount of shrinking or
restructuring can score above 1.0 on either axis, so both can only cost.

---

## 1. S — source size: bytes → **code lines**, and capped at 1.0

> "Source size: it is now counted in code lines (by scc, after your language's
> formatter), and it can only cost you. Its factor is capped at 1.0, so it no
> longer rewards the tersest language. Optimised code is almost always longer
> than the naive reference, whatever the language, so nearly everyone pays
> something there."

| | Before (this repo) | Now |
|---|---|---|
| Unit | bytes, comments stripped, identifiers normalised (`measure_axes.py:6`) | **code lines, counted by `scc`, after the language's formatter** |
| Factor | `(ref/ours)^0.15`, no upper bound | `min(1.0, (ref/ours)^0.15)` — **can only cost** |
| Reference value | `ref_source_bytes = 3180` for A, DEFERRED for B–E | must be **reference code lines**, re-derived per track |

Consequences:
* `tooling/measure_axes.py` measured the wrong thing. Its comment-stripping and
  identifier-normalising logic is now irrelevant, and its `S` number was not
  the scored quantity.
* Every K row that assumed an S *win* is wrong. Audited: the published anchor
  rows already use S ratios below 1.0 (A Solid 0.62, Nightshade 0.5889), i.e.
  they already model S as a penalty, so **no published K value changes**. The
  cap must still be encoded, or a future row with S > 1 would invent points
  that the scorer does not award.
* `ref_source_bytes = 3180` for Track A is now meaningless as a denominator.
  It stays recorded, marked as superseded, until the reference line count is
  published.

## 2. D — dependencies: `1 + deps + vendored/50000` → **0.90 per direct dependency**

> "D counts direct dependencies declared in your manifest, so ordinary
> libraries cost a predictable amount: one dependency gives a factor of 0.90."
> (That 0.90 is the K factor for one dependency: `(D_ref/D)^0.15 = (1/2)^0.15
> ≈ 0.90`. The formula itself is `D = 1 + dependencies` — NOT `0.90^deps`.)

| | Before (this repo) | Now |
|---|---|---|
| Formula | `D = 1 + (non-stdlib runtime deps) + (vendored source bytes / 50000)` (`measure_axes.py` — REPLACED) | `D = 1 + direct non-stdlib runtime dependencies` (the vendored-bytes term is gone) |
| One dependency | ≥ 2.0 + vendor bytes → a *large* penalty | `D = 2.0`; on K a factor `(1/2)^0.15 ≈ 0.90` — small and predictable |
| Zero dependencies | 1.0 | **1.0** (the baseline; nothing to lose) |

Consequences:
* With zero dependencies `D = 1.0` exactly — every "D = 1.0000" row in this
  repo is correct, and adding ANY crate immediately costs `(1/2)^0.15 ≈ 0.90`
  per crate against the reference's `D_ref = 1.00`.
* The `declared_dependencies == 0` gate in `harness lockdown` is still the
  right *check* (we ship zero), and it is now justified directly by the Oct 28
  formula: `D = 1 + deps`, so zero deps ⇒ the best possible D factor.

## 3. S loophole closed — a library you wrote counts as source

> "S covers everything in your repo at the recorded commit, except registry
> packages first published before Oct 1 by people outside your team. A library
> you wrote counts as source wherever it lives. … The S factor can't exceed 1.0,
> so moving code around can't make it a win. … The final image must build from a
> published, digest-pinned toolchain image, so code can't hide in a custom base
> image."

Action required here: **every `FROM` must be digest-pinned.**
all four `track-*/Dockerfile` currently say `FROM
debian:13-slim`, which is a tag, not a digest. `tooling/check_base_pinned.py`
now enforces this and is wired into the pre-freeze checklist.

## 4. Build-time and image-size ceilings

> "Builds are capped at 10 minutes and images at 2 GB."

New hard gates, neither of which existed in this repo's checklist:
`build ≤ 600 s`, `final image ≤ 2048 MB`. The builder stage is not in the final
image, so the 2 GB applies to the runtime stage; the 10 minutes applies to the
whole `docker build`, PGO included.

## 5. Training runs are allowed (this vindicates the PGO work)

> "Yes, allowed. Build time isn't scored, so you can do a training run inside
> `docker build` … Two constraints: `/data` is not mounted at build time, so
> train on input you generate with the published generator inside the build. The
> image may not ship cached outputs."

Build-time training (PGO, a JVM AOT cache, native-image) is allowed on generated
fixtures; it is an in-window decision once real code exists. Two checks follow:
* the build must not read `/data` (fixtures are generated, never the dataset), and
* **the image must not ship cached outputs** — no track *output* file
  (`summary.txt`, `results.txt`, `aggregates.txt`, `distances.txt`) may be present in the
  final image.

## 6. M — unchanged, and already what we do

> "M is the kernel's peak counter for the run, which counts pages your process
> actually touches. Untouched reservations … don't count."

`harness` already reads cgroup v2 `memory.peak`, which is that counter. The one
divergence is `run_direct`'s RSS sampling fallback, which is *not* the kernel
peak counter; it must never be labelled as if it were, and any M number derived
from it must be marked unverifiable.

## 7. Things that did **not** change

* T is still end-to-end wall time, exec to exit: startup and warmup count.
* No language handicaps; the event page no longer claims the axes cancel
  language differences (this repo never made that claim — verified: no
  occurrence of "cancel" in `docs/`, `docs/specs/` or `README.md`).
* No I/O-bound track this edition (F is deferred); `io_uring` is blocked by
  Docker's default seccomp profile, `mmap` works. Nothing in this repo planned
  an I/O track or relied on `io_uring`.
* The scoring box is a dedicated **Hetzner EX44 (Intel i5-13500)** at
  **Falkenstein (fsn1)** — not Helsinki — running pinned to
  `--cpuset-cpus=0-11` with an **8 GB memory cap** and `no_turbo`. This
  matches the repo's `--cpuset-cpus=0-11 --memory=8g` flags in the
  verify/bench scripts and in the run commands shown in every PERF.md.

---

## Checklist of code/config that must match this document

| Item | File | State |
|---|---|---|
| S measured as code lines by `scc` | `tooling/measure_axes.py` | **changed** — counts code lines, `scc` when available |
| S factor capped at 1.0 | `verification/harness/src/contract.rs` | **changed** — `S_FACTOR_CAP` |
| D = 1 + direct deps | `verification/harness/src/contract.rs` (`d_value`), `tooling/measure_axes.py` | implements the Oct 28 formula (`D_REF + n_deps`); `0.90^deps` was never the rule |
| Machine-readable values | `verification/contract.toml` | **changed** — `[rules_oct28]` |
| Digest-pinned base images | `track-*/Dockerfile` — one per track, build + runtime stages | **enforced** by `tooling/check_base_pinned.py` against `tooling/base-image.txt`; both digests pinned (rust:1-slim-trixie, debian:13-slim) |
| Build ≤ 600 s, image ≤ 2048 MB | `verification/pre-freeze.ps1`, `harness lockdown` | **recorded** as gates; need the measured values on the scoring box |
| No cached outputs in the image | `.dockerignore` + `tooling/check_base_pinned.py` | **checked** |
| M from the kernel counter only | `verification/harness/src/cgroup.rs` | already correct; RSS fallback must stay labelled |