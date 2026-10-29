# KICKOFF PLAN — Pal Ghevariya (solo) — 72 hours, non-stop

> The operating plan for Oct 30 15:00 UTC → Nov 2 15:00 UTC. Read together with
> `docs/HANDOVER-oct28.md` (state + verified assets) and
> `docs/WINDOW-RUNBOOK.md` (the hour-by-hour tool map) and
> `docs/MODULE-PLAN.md` (the adopted D1 architecture: modules, acceptance
> tests, build order, risks).
>
> **Rule of the campaign: every module is built at "insane level", never child
> level. A power house is only as strong as its weakest module.**

## 0. Identity (set, no longer open)

| Field | Value |
|---|---|
| Team member | **Pal Ghevariya** (solo, 1 of 1) |
| Team name on the board | **Pal Ghevariya** |
| GitHub handle | `pal-123456789` (from the noreply address) |
| Commit email | `172589442+pal-123456789@users.noreply.github.com` |
| Repo (to create at kickoff) | fresh, from the starter template, branch `speed-demon` |
| Tracks entered | **A · B · C · D** (all four → the overall title needs ≥3, best three count) |
| Language | Rust (std only, no crates → D = 1.00) |

The four `track-*/.speed-demon.toml` carry this name; the git identity for this
repository is set to the same name and address so every commit links to the
GitHub account (judges read the commit history as the evidence of the window).

## 1. The three days, hour by hour

| Window (UTC) | Phase | Output |
|---|---|---|
| **D1** Oct 30 15:00 → Oct 31 03:00 | **Bootstrap**: create the repo, push the skeleton (already committed here), confirm the board app + form, run `official-data.py`, first end-to-end green loop on all four tracks | repo live, board running, skeleton pushed |
| **D1** Oct 31 03:00 → Oct 31 15:00 | **Core modules**: each track's first real implementation — correctness first, measured, one module per commit with its verification evidence | 4 tracks SHA-green against the official expected hash |
| **D2** Oct 31 15:00 → Nov 1 15:00 | **Optimisation rounds**: the measured-first ranking (A timestamp, B build-side, C amount+aggregate, D search); a rung per commit, re-verified each time | score climbing, PERF.md per track rewritten per rung |
| **D3** Nov 1 15:00 → Nov 2 09:00 | **Comprehensive polish**: E2E, adversarial, determinism, memory ceilings, docs completeness (every feature written down), demo assets | frozen-quality tree |
| **D3** Nov 2 09:00 → 15:00 | **Wrap-up ONLY now**: final verification battery, docs audit, freeze (tag), video, submission form | submitted |

**No wrap-up before D3.** Days 1–2 are for building modules; the wrap-up is a
single scheduled phase at the end. If something looks finished early, the
answer is another optimisation rung or another verification pass — never an
early freeze.

## 2. The module loop (repeat until the last day)

Every module — a track's parser, its index, its join, its search, a tool, a
gate, a doc set — goes through the same five steps:

1. **Decompose**: split the track into modules with a written interface (inputs,
   outputs, invariants, which measurement proves it).
2. **Build with multi-agents**: fan the module out to subagents (one per
   sub-task) with a self-contained brief: the contract, the oracle, the
   acceptance test, the exact files it may touch.
3. **Verify the agents' work ourselves**: never trust the report — re-run the
   oracle diff, the tiny conformance corpus, the determinism matrix, the
   official expected hash, and the axis measurements on the integrated tree.
4. **Correct**: fix, or send the agent back with the failing evidence.
5. **Commit and push**: one module = one commit, containing the code **and its
   verification evidence and its documentation**. The commit message states the
   measured before/after numbers.

Push after every verified module, so the board re-runs and the judges can read
a truthful, incremental history of the 72 hours.

## 3. Documentation is part of the module, not an afterthought

The biggest avoidable loss is building something and never writing it down.
Every module ships with:

- the design decisions and the **measured** numbers that justified them,
  appended to the track's `PERF.md` and `README.md`;
- a note in `docs/depth/` when it is a technique (with the benchmark that
  proves it);
- the verification transcript (which gate, which hash, which measurement).

At D3 there is a dedicated **documentation audit**: every feature, tool, gate
and number must be findable in a readable `.md`/`.txt`. Anything undocumented is
treated as a defect.

## 4. The 5-minute video (built on D3)

**Structure — 4 min slides + 1 min output.**

1. **4 minutes: slides + voice.** We make the deck fast and **do not verify it**
   afterwards (no time). The narration is one ElevenLabs prompt (supplied at
   D3) covering: the problem, the four tracks, the scoring formula and the axes,
   what was built, the measured wins, the verification story (organizers' own
   tests, guardrails, expected hashes), and the results.
2. **1 minute: the output.** A single command sequence (supplied at D3) that
   shows the four tracks running on the official sample data and the expected
   hashes matching on screen. Silent, with a classical piece over it.

So the deliverables at D3 are: the `.pptx`, one audio prompt, and one output
command block. Everything else is already in the repo.

## 5. Working with high-level models

When a module needs more brainpower than is on hand, a single self-contained
prompt is written here (or in chat) for Pal to paste into a high-level model;
the answer comes back and is integrated, verified, and committed like any other
module. The prompt always carries: the exact contract, the constraints, the
acceptance test, and what "insane level" means for that module.

## 6. Standing constraints (never violated)

- **Rule 05**: no solution code before kickoff; everything in `track-*/src` at
  kickoff must be the scaffold. `rule05_guard.py` CLEAR.
- **Exactly one output file per track**, official name, exit 0, deterministic.
- **Offline builds**, digest-pinned allowlisted bases, ≤10 min build, ≤2 GB image.
- **D = 1.00** (no crates), S measured with the official rule (prose/data free),
  so every line of Rust is paid for — keep the implementation terse and prove
  each technique with a measurement.
- **Push after every verified module**; the commit history is the evidence.
