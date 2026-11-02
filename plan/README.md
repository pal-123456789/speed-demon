# Pre-Build Plan

> **This folder is the planning record.** Everything in here was written *before* the implementation
> existed. The finished programs are in [`track-a/`](../track-a) … [`track-d/`](../track-d), and the
> measured results are in the [top-level README](../README.md) and [docs/JOURNEY.md](../docs/JOURNEY.md).

It is kept because it shows how four tracks were scoped, costed and sequenced before any code was
written — and because the places where the plan was **wrong** are as informative as the places it was
right.

## What is here

| File | What it records |
|---|---|
| `KICKOFF-CARD.md` | the schedule table and the decision points |
| `KICKOFF-PLAN-pal.md` | the working plan for the build window |
| `MASTER-PLAN.md` | the overall approach across all four tracks |
| `MODULE-PLAN.md` | the module breakdown, acceptance tests and build order |
| `PLAN-B-C.md`, `PLAN-TO-NUMBER-ONE.md` | the plans for the two tracks that needed the most redesign |
| `WINDOW-RUNBOOK.md` | the hour-by-hour operating plan |
| `GAP-ANALYSIS.md` | what was missing and how it was ranked |
| `RULE-05.md`, `RULE-CHANGES-oct28.md` | the organizer-rule boundary and its changes |
| `CAMPAIGN-STATUS.md`, `SESSION-LOG.md`, `HANDOVER-oct28.md` | the operational state at points in time |
| `MEASURING-LOCALLY.md`, `VERIFY.md` | how things were to be measured and checked |
| `WRITEUP-EVIDENCE.md` | the findings considered worth writing up |
| `DEMO-STORYBOARD.md`, `depth/94-VISUALS.md`, `depth/99-EVENTS-PAGE-PINS.md` | the presentation plan |
| `SCRATCH-SPACE.md`, `INDEX.md`, `depth/00-INDEX.md` | navigation and working notes |
| `depth/95-GROUND-BLUEPRINTS.md`, `depth/BRIEFS-*.md` | the deep technical briefs |
| `design/` | the per-track design documents, written before implementation |

## What the plan missed

**It was directionally right and specifically wrong.** The plan correctly identified the four tracks'
core structures. It did not predict where the time actually was, and three of the four tracks had to
be re-attributed by measurement before they could be made faster:

- **Track A** — the plan was about memory throughout. Measurement showed **the parser is 50.8% of the
  run**, which no amount of memory work would have addressed.
- **Track B** — measurement showed **the corpus scan is 70%** of the run.
- **Track D** — the plan treated construction and search as comparable. Measurement showed
  **construction is 66% and the search only 33%**, which is why four search-side experiments lost.

The largest wins in the project all came from **deleting** code the plan had called for, and the
largest single reversal — `target-cpu=native` — was a build decision the plan made on reasoning that
measurement later contradicted.

**That gap between plan and measurement is the useful part of this folder.** See
[docs/JOURNEY.md](../docs/JOURNEY.md) for what actually happened.

---

<p align="center">
  <sub>
  <b>Speed Demon 2026</b> · four Rust programs · no dependencies · every answer exact<br/>
  <a href="../README.md">README</a> ·
  <a href="JOURNEY.md">Journey</a> ·
  <a href="TECHNIQUES.md">Techniques</a> ·
  <a href="MEASUREMENT.md">Measurement</a> ·
  <a href="../plan/README.md">Plan</a>
  </sub>
</p>
