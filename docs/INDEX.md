# Documentation index

Where to look, by what you are trying to do. Everything here is prose, plans or
verification infrastructure — none of it is submission code, and all of it is
outside the four track folders.

If you only read one thing: **[KICKOFF-CARD.md](KICKOFF-CARD.md)**.

---

## By intent

| I want to… | Read |
|---|---|
| know the current state, and what is verified and how | [SESSION-LOG.md](SESSION-LOG.md) |
| start work at kickoff, in order | [KICKOFF-CARD.md](KICKOFF-CARD.md) — the canonical schedule is its first table |
| know what to build, per track, and in what order | [MODULE-PLAN.md](MODULE-PLAN.md) |
| work hour by hour through the window | [WINDOW-RUNBOOK.md](WINDOW-RUNBOOK.md) |
| understand the rules and the pre-kickoff boundary | [RULE-05.md](RULE-05.md), [RULE-CHANGES-oct28.md](RULE-CHANGES-oct28.md) |
| price a decision in K before taking it | [MODULE-PLAN.md](MODULE-PLAN.md) plus `python tooling/k_whatif.py --track A --help` |
| know what the judges score and how the video is built | [MODEL-OUTPUTS-ADOPTED.md](MODEL-OUTPUTS-ADOPTED.md) |
| see the operational dashboard and the contract of values | [CAMPAIGN-STATUS.md](CAMPAIGN-STATUS.md) |
| write the $100 side-quest post | [WRITEUP-EVIDENCE.md](WRITEUP-EVIDENCE.md) |
| recover context after a break | [SESSION-LOG.md](SESSION-LOG.md) then [HANDOVER-oct28.md](HANDOVER-oct28.md) |

## Reference material

| Path | What it is |
|---|---|
| [../verification/contract.toml](../verification/contract.toml) | **The authority for every number.** Reference axes, timeouts, published hashes, scale facts. If a document disagrees with this file, the file is right. |
| [../reference/official/](../reference/official/) | The organizers' specs, generators, references, harness and tests, vendored byte-identically. The highest authority of all. |
| [depth/00-INDEX.md](depth/00-INDEX.md) | The second-level deep dives: per-track feature inventories and ladders. |
| [specs/](specs/) | **SUPERSEDED** early design exploration. Read for reasoning, never for orders — each file's header lists what changed. |
| [../tooling/](../tooling/) | Every gate and generator. `sh tooling/ci.sh` runs them all. |

## Track briefs

One page per track, pointing into the deeper material:

[track-a-ingest.md](track-a-ingest.md) ·
[track-b-index.md](track-b-index.md) ·
[track-c-join.md](track-c-join.md) ·
[track-d-traverse.md](track-d-traverse.md)

## Presentation and prompts

| File | What it is |
|---|---|
| [DEMO-STORYBOARD.md](DEMO-STORYBOARD.md) | The video storyboard. |
| [SpeedDemon-Astra-prompt-pack.md](SpeedDemon-Astra-prompt-pack.md) | Raw prompt pack and model outputs, kept for provenance. Some of it is role-play transcript; treat it as an archive, not as a specification. |
| [depth/98-MODEL-PROMPTS.md](depth/98-MODEL-PROMPTS.md), [depth/99-PROMPT-PACK.md](depth/99-PROMPT-PACK.md) | Prompt material and the events-page pins. |

## Reading order for a new reader

1. [KICKOFF-CARD.md](KICKOFF-CARD.md) — schedule, then the command sequence.
2. [SESSION-LOG.md](SESSION-LOG.md) — what exists and what has been proven.
3. [MODULE-PLAN.md](MODULE-PLAN.md) — the architecture of record.
4. [../README.md](../README.md) — the repository in five minutes.
